#!/usr/bin/env python3
"""Standalone Reth endpoint-probe feasibility check, using Python stdlib only.

Usage: python3 nix/checks/prove-reth-endpoint-probes.py /nix/store/...-reth/bin/reth

Starts the pinned Reth binary with the same listener flags as the adapter,
checks HTTP, WebSocket, and authenticated Engine API responses, then stops it.
The JWT stays in a temporary private file and is never printed.
"""

import json
import importlib.util
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import time
from urllib.error import HTTPError
from urllib.request import ProxyHandler, Request, build_opener


OPENER = build_opener(ProxyHandler({}))
# Exercise the production routines; the harness owns only process setup and
# independent negative exchanges against the pinned node.
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location(
    "reth_probe", Path(__file__).parent.parent / "adapters/reth-probe.py"
)
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)
http_rpc = probe.http_rpc
websocket_rpc = probe.websocket_rpc
jwt_token = probe.jwt_token
rpc_request = probe.rpc_request
checked_response = probe.checked_response


def free_ports(count):
    sockets = []
    try:
        for _ in range(count):
            sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            sock.bind(("127.0.0.1", 0))
            sockets.append(sock)
        return [sock.getsockname()[1] for sock in sockets]
    finally:
        for sock in sockets:
            sock.close()


def unauthorized(host, port, token=None):
    try:
        headers = {"Content-Type": "application/json"}
        if token is not None:
            headers["Authorization"] = f"Bearer {token}"
        request = Request(f"http://{host}:{port}",
                          data=rpc_request("engine_exchangeCapabilities", [[]]),
                          headers=headers, method="POST")
        with OPENER.open(request, timeout=2):
            pass
    except HTTPError as error:
        if error.code not in (401, 403):
            raise AssertionError(f"unexpected unauthenticated status: {error.code}") from error
        return error.code
    raise AssertionError("Engine API accepted a missing or invalid JWT")


def rpc_error_is_not_success(host, port):
    request = Request(
        f"http://{host}:{port}",
        data=rpc_request("nixfied_probe_nonexistent_method", []),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with OPENER.open(request, timeout=2) as response:
        raw = response.read(65537)
        if response.status != 200 or "error" not in json.loads(raw):
            raise AssertionError("expected HTTP 200 carrying a JSON-RPC error")
        try:
            checked_response(raw, "block-number")
        except ValueError:
            return response.status
        raise AssertionError("JSON-RPC error was accepted as successful readiness")


def prove(reth):
    with tempfile.TemporaryDirectory(prefix="nixfied-reth-probe-") as directory:
        root = Path(directory)
        secret_path = root / "jwt.hex"
        secret_path.write_text("0" * 64 + "\n", encoding="ascii")
        secret_path.chmod(0o600)
        http_port, ws_port, auth_port = free_ports(3)
        log_path = root / "reth.log"
        command = [
            str(reth), "node", "--datadir", str(root / "data"), "--ipcdisable",
            "--http", "--http.addr", "127.0.0.1", "--http.port", str(http_port),
            "--ws", "--ws.addr", "127.0.0.1", "--ws.port", str(ws_port),
            "--authrpc.addr", "127.0.0.1", "--authrpc.port", str(auth_port),
            "--authrpc.jwtsecret", str(secret_path), "--dev",
        ]
        with log_path.open("w", encoding="utf-8") as log:
            started = time.monotonic()
            node = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                deadline = time.monotonic() + 90
                while True:
                    if node.poll() is not None:
                        raise AssertionError(f"Reth exited during startup: {node.returncode}")
                    try:
                        block = http_rpc("127.0.0.1", http_port, "eth_blockNumber", [], "block-number")
                        break
                    except (OSError, HTTPError, AssertionError, ValueError):
                        if time.monotonic() >= deadline:
                            raise AssertionError("HTTP JSON-RPC did not become ready")
                        time.sleep(0.25)
                http_ready_seconds = time.monotonic() - started
                round_started = time.monotonic()
                ws_block = websocket_rpc("127.0.0.1", ws_port)
                rpc_error_status = rpc_error_is_not_success("127.0.0.1", http_port)
                no_auth_status = unauthorized("127.0.0.1", auth_port)
                bad_token = jwt_token("1" * 64)
                bad_auth_status = unauthorized("127.0.0.1", auth_port, bad_token)
                token = jwt_token(secret_path.read_text(encoding="ascii").strip())
                capabilities = http_rpc(
                    "127.0.0.1", auth_port, "engine_exchangeCapabilities", [[]],
                    "capabilities", token,
                )
                print(
                    json.dumps({
                        "reth": str(reth), "httpPort": http_port, "httpBlock": block,
                        "wsPort": ws_port, "wsBlock": ws_block,
                        "authPort": auth_port, "unauthenticatedStatus": no_auth_status,
                        "invalidJwtStatus": bad_auth_status,
                        "jsonRpcErrorHttpStatus": rpc_error_status,
                        "engineCapabilitiesCount": len(capabilities),
                        "httpReadySeconds": http_ready_seconds,
                        "remainingChecksSeconds": time.monotonic() - round_started,
                    }, sort_keys=True)
                )
            except Exception:
                log.flush()
                tail = log_path.read_text(errors="replace")[-5000:]
                for secret in (secret_path.read_text(encoding="ascii").strip(),
                               locals().get("bad_token"), locals().get("token")):
                    if secret:
                        tail = tail.replace(secret, "[redacted]")
                print("Reth log tail:\n" + tail, file=sys.stderr)
                raise
            finally:
                if node.poll() is None:
                    os.killpg(node.pid, signal.SIGTERM)
                    try:
                        node.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        os.killpg(node.pid, signal.SIGKILL)
                        node.wait(timeout=10)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: prove-reth-endpoint-probes.py /path/to/reth")
    prove(Path(sys.argv[1]))
