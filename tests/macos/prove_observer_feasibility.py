#!/usr/bin/env python3
"""Read-only macOS feasibility probe for cross-UID TCP listener observation.

Run once normally, then from a local terminal with:
  sudo -v
  /usr/bin/python3 tests/macos/prove_observer_feasibility.py --sudo-sysctl

For the separate cross-UID FD and fileport test, inspect this file and run:
  sudo /usr/bin/python3 tests/macos/prove_observer_feasibility.py --as-root

In --sudo-sysctl mode only Apple's sysctl binary is elevated. In --as-root
mode the parent Python harness is elevated and its children drop to the
invoking UID. The probe never modifies system configuration and binds only
ephemeral loopback ports.
"""

import ctypes
from collections import Counter
import json
import os
import socket
import struct
import subprocess
import sys
import time


LIBC = ctypes.CDLL("/usr/lib/libSystem.B.dylib", use_errno=True)
NAME = b"net.inet.tcp.pcblist_n"
KINDS = (16, 1, 2, 4, 8, 32)
PROC_PIDLISTFDS = 1
PROC_PIDFDSOCKETINFO = 3
PROC_PIDLISTFILEPORTS = 14
PROC_PIDFILEPORTSOCKETINFO = 3
PROX_FDTYPE_SOCKET = 2


def listener():
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.bind(("127.0.0.1", 0))
    sock.listen()
    return sock


def raw_snapshot():
    required = ctypes.c_size_t()
    if LIBC.sysctlbyname(NAME, None, ctypes.byref(required), None, 0):
        raise OSError(ctypes.get_errno(), "PCB size query")
    data = ctypes.create_string_buffer(required.value)
    actual = ctypes.c_size_t(required.value)
    if LIBC.sysctlbyname(NAME, data, ctypes.byref(actual), None, 0):
        raise OSError(ctypes.get_errno(), "PCB read")
    return data.raw[: actual.value]


def snapshot(raw):
    if len(raw) < 48:
        raise ValueError("truncated xinpgen envelope")
    header = struct.unpack_from("=IQQ", raw, 4)
    trailer = struct.unpack_from("=IQQ", raw, len(raw) - 20)
    offset = 24
    decoded = 0
    listeners = {}
    while offset < len(raw) - 24:
        parts = []
        for kind in KINDS:
            if offset + 8 > len(raw) - 24:
                raise ValueError("truncated PCB subrecord")
            length, observed_kind = struct.unpack_from("=II", raw, offset)
            if observed_kind != kind or length < 8 or offset + length > len(raw) - 24:
                raise ValueError(f"invalid PCB subrecord at {offset}")
            parts.append(raw[offset : offset + length])
            offset += (length + 7) & ~7
        decoded += 1
        pcb, socket_info, _, _, _, tcp = parts
        if struct.unpack_from("=I", tcp, 36)[0] == 1:
            port = struct.unpack_from("!H", pcb, 18)[0]
            listeners[port] = {
                "socket_handle": struct.unpack_from("=Q", socket_info, 8)[0],
                "uid": struct.unpack_from("=I", socket_info, 64)[0],
            }
    if offset != len(raw) - 24:
        raise ValueError("PCB body did not end at trailer")
    return {"header": header, "trailer": trailer, "decoded": decoded, "listeners": listeners}


def child_socket_handles(pid):
    ctypes.set_errno(0)
    required = LIBC.proc_pidinfo(pid, PROC_PIDLISTFDS, 0, None, 0)
    if required == 0 and ctypes.get_errno() == 0:
        return []
    if required <= 0:
        raise OSError(ctypes.get_errno(), f"cannot list FDs of pid {pid}")
    capacity = required + 1024
    for _ in range(4):
        data = ctypes.create_string_buffer(capacity)
        ctypes.set_errno(0)
        actual = LIBC.proc_pidinfo(pid, PROC_PIDLISTFDS, 0, data, capacity)
        if actual == 0 and ctypes.get_errno() == 0:
            return []
        if actual <= 0:
            raise OSError(ctypes.get_errno(), f"cannot read FDs of pid {pid}")
        if actual >= capacity:
            capacity *= 2
            continue
        if actual % 8:
            raise ValueError(f"malformed FD list for pid {pid}: {actual} bytes")
        handles = []
        for offset in range(0, actual, 8):
            fd, kind = struct.unpack_from("=iI", data.raw, offset)
            if kind != PROX_FDTYPE_SOCKET:
                continue
            detail = ctypes.create_string_buffer(1024)
            size = LIBC.proc_pidfdinfo(pid, fd, PROC_PIDFDSOCKETINFO, detail, len(detail))
            if size <= 0:
                raise OSError(ctypes.get_errno(), f"cannot read pid {pid} fd {fd}")
            if size < 260:
                raise ValueError(f"short socket FD info for pid {pid} fd {fd}: {size}")
            handles.append(struct.unpack_from("=Q", detail.raw, 160)[0])
        return handles
    raise ValueError("FD list kept growing")


def child_fileport_socket_handles(pid):
    ctypes.set_errno(0)
    required = LIBC.proc_pidinfo(pid, PROC_PIDLISTFILEPORTS, 0, None, 0)
    if required == 0 and ctypes.get_errno() == 0:
        return []
    if required <= 0:
        raise OSError(ctypes.get_errno(), f"cannot list fileports of pid {pid}")
    capacity = required + 1024
    for _ in range(4):
        data = ctypes.create_string_buffer(capacity)
        ctypes.set_errno(0)
        actual = LIBC.proc_pidinfo(pid, PROC_PIDLISTFILEPORTS, 0, data, capacity)
        if actual == 0 and ctypes.get_errno() == 0:
            return []
        if actual <= 0:
            raise OSError(ctypes.get_errno(), f"cannot read fileports of pid {pid}")
        if actual >= capacity:
            capacity *= 2
            continue
        if actual % 8:
            raise ValueError(f"malformed fileport list for pid {pid}: {actual} bytes")
        handles = []
        for offset in range(0, actual, 8):
            port_name, kind = struct.unpack_from("=II", data.raw, offset)
            if kind != PROX_FDTYPE_SOCKET:
                continue
            detail = ctypes.create_string_buffer(1024)
            size = LIBC.proc_pidfileportinfo(
                pid, port_name, PROC_PIDFILEPORTSOCKETINFO, detail, len(detail)
            )
            if size <= 0:
                raise OSError(ctypes.get_errno(), f"cannot read pid {pid} fileport {port_name}")
            if size < 260:
                raise ValueError(f"short socket fileport info for pid {pid}: {size}")
            handles.append(struct.unpack_from("=Q", detail.raw, 160)[0])
        return handles
    raise ValueError("fileport list kept growing")


def all_pids():
    required = LIBC.proc_listallpids(None, 0)
    if required < 0:
        raise OSError(ctypes.get_errno(), "cannot size process list")
    capacity = required + 64
    for _ in range(4):
        values = (ctypes.c_int * capacity)()
        count = LIBC.proc_listallpids(values, ctypes.sizeof(values))
        if count < 0:
            raise OSError(ctypes.get_errno(), "cannot read process list")
        if count >= capacity:
            capacity *= 2
            continue
        return sorted(set(pid for pid in values[:count] if pid > 0))
    raise ValueError("process list kept growing")


def process_scan_summary():
    pids = all_pids()
    inspected = 0
    handles = 0
    uninspectable_live = []
    reasons = Counter()
    for pid in pids:
        try:
            found = child_socket_handles(pid)
            handles += len(found)
            inspected += 1
        except (OSError, ValueError) as error:
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                continue
            except PermissionError:
                pass
            uninspectable_live.append(pid)
            reasons[f"{type(error).__name__}:{getattr(error, 'errno', None)}"] += 1
    return {
        "listed_pids": len(pids),
        "inspected_pids": inspected,
        "socket_fds": handles,
        "uninspectable_live_count": len(uninspectable_live),
        "uninspectable_live_examples": uninspectable_live[:8],
        "uninspectable_reasons": dict(sorted(reasons.items())),
    }


def run():
    as_root = sys.argv[1:] == ["--as-root"]
    if as_root and (os.geteuid() != 0 or "SUDO_UID" not in os.environ):
        raise RuntimeError("--as-root requires sudo and SUDO_UID")
    own = listener()
    def drop_to_invoking_user():
        os.setgroups([])
        os.setgid(int(os.environ["SUDO_GID"]))
        os.setuid(int(os.environ["SUDO_UID"]))

    def start_child(mode):
        return subprocess.Popen(
            [sys.executable, os.path.abspath(__file__), mode],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
            preexec_fn=drop_to_invoking_user if as_root else None,
        )

    child = start_child("--child")
    fileport_child = start_child("--child-fileport") if as_root else None
    try:
        child_info = json.loads(child.stdout.readline())
        fileport_info = json.loads(fileport_child.stdout.readline()) if fileport_child else None
        own_port = own.getsockname()[1]
        child_port = child_info["port"]
        print(json.dumps({"observer_euid": os.geteuid(), "own_port": own_port,
                          "child": child_info, "fileport_child": fileport_info}), flush=True)
        for attempt in range(5):
            if sys.argv[1:] == ["--sudo-sysctl"]:
                result = subprocess.run(
                    ["/usr/bin/sudo", "-n", "/usr/sbin/sysctl", "-b", "net.inet.tcp.pcblist_n"],
                    capture_output=True,
                )
                if result.returncode:
                    raise RuntimeError(result.stderr.decode(errors="replace"))
                raw = result.stdout
            else:
                raw = raw_snapshot()
            observed = snapshot(raw)
            own_record = observed["listeners"].get(own_port)
            child_record = observed["listeners"].get(child_port)
            fileport_record = observed["listeners"].get(fileport_info["port"]) if fileport_info else None
            result = {
                "attempt": attempt,
                "header_count": observed["header"][0],
                "trailer_count": observed["trailer"][0],
                "generation_matches": observed["header"][1:] == observed["trailer"][1:],
                "decoded": observed["decoded"],
                "own_seen": own_record is not None,
                "child_seen": child_record is not None,
                "child_record_uid": None if child_record is None else child_record["uid"],
            }
            if fileport_info:
                result["fileport_child_seen"] = fileport_record is not None
                result["fileport_child_record_uid"] = (
                    None if fileport_record is None else fileport_record["uid"]
                )
                try:
                    result["fileport_child_fd_count"] = len(child_socket_handles(fileport_info["pid"]))
                    fileport_handles = child_fileport_socket_handles(fileport_info["pid"])
                    result["fileport_child_fileport_count"] = len(fileport_handles)
                    if fileport_record is not None:
                        result["fileport_child_correlated"] = (
                            fileport_record["socket_handle"] in fileport_handles
                        )
                except (OSError, ValueError) as error:
                    result["fileport_child_error"] = str(error)
            try:
                handles = child_socket_handles(child_info["pid"])
                result["child_socket_fd_count"] = len(handles)
                if child_record is not None:
                    result["child_fd_correlated"] = child_record["socket_handle"] in handles
            except (OSError, ValueError) as error:
                result["child_fd_error"] = str(error)
            print(json.dumps(result), flush=True)
            time.sleep(0.2)
        if as_root:
            print(json.dumps({"process_scan": process_scan_summary()}), flush=True)
    finally:
        child.stdin.close()
        child.wait(timeout=5)
        if fileport_child:
            fileport_child.stdin.close()
            fileport_child.wait(timeout=5)
        own.close()


if __name__ == "__main__":
    if sys.platform != "darwin":
        raise SystemExit("macOS only")
    if sys.argv[1:] == ["--child"]:
        held = listener()
        print(json.dumps({"pid": os.getpid(), "uid": os.getuid(), "port": held.getsockname()[1]}), flush=True)
        sys.stdin.read(1)
        held.close()
    elif sys.argv[1:] == ["--child-fileport"]:
        held = listener()
        port = held.getsockname()[1]
        fileport = ctypes.c_uint32()
        if LIBC.fileport_makeport(held.fileno(), ctypes.byref(fileport)):
            raise OSError(ctypes.get_errno(), "fileport_makeport failed")
        held.close()
        print(json.dumps({"pid": os.getpid(), "uid": os.getuid(), "port": port}), flush=True)
        sys.stdin.read(1)
    else:
        run()
