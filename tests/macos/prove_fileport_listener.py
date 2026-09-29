#!/usr/bin/env python3
"""Demonstrate whether a Mach fileport can keep a TCP listener alive without an FD."""

import ctypes
import array
import json
import os
import socket
import struct
import sys

from prove_observer_feasibility import raw_snapshot, snapshot


LIBC = ctypes.CDLL("/usr/lib/libSystem.B.dylib", use_errno=True)
PROC_PIDLISTFDS = 1
PROC_PIDFDSOCKETINFO = 3
PROC_PIDLISTFILEPORTS = 14
PROC_PIDFILEPORTSOCKETINFO = 3
PROX_FDTYPE_SOCKET = 2


def call_info(function, *args):
    data = ctypes.create_string_buffer(4096)
    ctypes.set_errno(0)
    size = function(*args, data, len(data))
    if size < 0:
        raise OSError(ctypes.get_errno(), "libproc inspection failed")
    if size % 8 and function == LIBC.proc_pidinfo:
        raise ValueError("malformed libproc list")
    return data.raw[:size]


def socket_handle(data):
    if len(data) < 260:
        raise ValueError("truncated socket info")
    return struct.unpack_from("=Q", data, 160)[0]


def socket_fds(pid):
    fd_list = call_info(LIBC.proc_pidinfo, pid, PROC_PIDLISTFDS, 0)
    return [fd for fd, kind in struct.iter_unpack("=iI", fd_list) if kind == PROX_FDTYPE_SOCKET]


def fileport_sockets(pid):
    fileport_list = call_info(LIBC.proc_pidinfo, pid, PROC_PIDLISTFILEPORTS, 0)
    return [name for name, kind in struct.iter_unpack("=II", fileport_list)
            if kind == PROX_FDTYPE_SOCKET]


def matching_fd_handles(pid, identity):
    return [fd for fd in socket_fds(pid) if socket_handle(
        call_info(LIBC.proc_pidfdinfo, pid, fd, PROC_PIDFDSOCKETINFO)
    ) == identity]


def prove_fileport():
    held = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    held.bind(("127.0.0.1", 0))
    held.listen()
    port = held.getsockname()[1]
    before = call_info(LIBC.proc_pidfdinfo, os.getpid(), held.fileno(), PROC_PIDFDSOCKETINFO)
    original_handle = socket_handle(before)
    fileport = ctypes.c_uint32()
    if LIBC.fileport_makeport(held.fileno(), ctypes.byref(fileport)):
        raise OSError(ctypes.get_errno(), "fileport_makeport failed")
    held.close()
    pcb_record = snapshot(raw_snapshot())["listeners"].get(port)

    fd_sockets = socket_fds(os.getpid())
    listed_fileports = fileport_sockets(os.getpid())
    fileport_info = call_info(
        LIBC.proc_pidfileportinfo,
        os.getpid(), fileport.value, PROC_PIDFILEPORTSOCKETINFO,
    )
    restored_handle = socket_handle(fileport_info)
    with socket.create_connection(("127.0.0.1", port), timeout=2):
        connected = True
    outcome = {
        "port": port,
        "socket_fd_count_after_close": len(fd_sockets),
        "fileport_socket_count": len(listed_fileports),
        "fileport_listed": fileport.value in listed_fileports,
        "socket_identity_preserved": original_handle == restored_handle,
        "pcb_correlated": pcb_record is not None and pcb_record["socket_handle"] == original_handle,
        "connected_after_fd_close": connected,
    }
    print(json.dumps(outcome), flush=True)
    if not (outcome["fileport_listed"] and outcome["socket_identity_preserved"]
            and outcome["pcb_correlated"] and connected):
        raise SystemExit("fileport listener proof failed")


def prove_queued_rights():
    held = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    held.bind(("127.0.0.1", 0))
    held.listen()
    port = held.getsockname()[1]
    identity = socket_handle(call_info(
        LIBC.proc_pidfdinfo, os.getpid(), held.fileno(), PROC_PIDFDSOCKETINFO
    ))
    sender, receiver = socket.socketpair(socket.AF_UNIX, socket.SOCK_DGRAM)
    sender.sendmsg([b"x"], [(socket.SOL_SOCKET, socket.SCM_RIGHTS,
                             array.array("i", [held.fileno()]))])
    held.close()
    hidden_from_fds = not matching_fd_handles(os.getpid(), identity)
    hidden_from_fileports = not fileport_sockets(os.getpid())
    pcb_record = snapshot(raw_snapshot())["listeners"].get(port)
    with socket.create_connection(("127.0.0.1", port), timeout=2):
        connected = True
    message, ancdata, _, _ = receiver.recvmsg(1, socket.CMSG_SPACE(array.array("i").itemsize))
    received = array.array("i")
    for level, kind, data in ancdata:
        if level == socket.SOL_SOCKET and kind == socket.SCM_RIGHTS:
            received.frombytes(data[: received.itemsize])
    restored_identity = socket_handle(call_info(
        LIBC.proc_pidfdinfo, os.getpid(), received[0], PROC_PIDFDSOCKETINFO
    ))
    os.close(received[0])
    sender.close()
    receiver.close()
    outcome = {
        "port": port,
        "no_listener_fd_while_queued": hidden_from_fds,
        "no_fileport_while_queued": hidden_from_fileports,
        "connected_while_queued": connected,
        "pcb_correlated_while_queued": pcb_record is not None and pcb_record["socket_handle"] == identity,
        "socket_identity_preserved_after_receive": identity == restored_identity,
        "message_received": message == b"x",
    }
    print(json.dumps(outcome), flush=True)
    if not all(value for key, value in outcome.items() if key != "port"):
        raise SystemExit("queued rights listener proof failed")


if __name__ == "__main__":
    if sys.platform != "darwin":
        raise SystemExit("macOS only")
    if sys.argv[1:] == ["--queued-rights"]:
        prove_queued_rights()
    elif not sys.argv[1:]:
        prove_fileport()
    else:
        raise SystemExit("usage: prove_fileport_listener.py [--queued-rights]")
