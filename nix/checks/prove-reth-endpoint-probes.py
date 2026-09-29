#!/usr/bin/env python3
"""Standalone Reth endpoint-probe feasibility check, using Python stdlib only.

Usage: python3 nix/checks/prove-reth-endpoint-probes.py /nix/store/...-reth/bin/reth

Starts the pinned Reth binary with the same listener flags as the adapter,
checks HTTP, WebSocket, and authenticated Engine API responses, then stops it.
The JWT stays in a temporary private file and is never printed.
"""

import base64
import hashlib
import hmac
import json
import os
from pathlib import Path
import re
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import time
from urllib.error import HTTPError
from urllib.request import ProxyHandler, Request, build_opener


OPENER = build_opener(ProxyHandler({}))
RPC_ID = 1
HEX_QUANTITY = re.compile(r"^0x(?:0|[1-9a-fA-F][0-9a-fA-F]*)$")
WS_GUID = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11"


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


def rpc_request(method, params):
    return json.dumps(
        {"jsonrpc": "2.0", "method": method, "params": params, "id": RPC_ID},
        separators=(",", ":"),
    ).encode("ascii")


def checked_response(raw, expected):
    body = json.loads(raw)
    if not isinstance(body, dict) or body.get("jsonrpc") != "2.0" or body.get("id") != RPC_ID:
        raise AssertionError(f"invalid JSON-RPC envelope: {body!r}")
    if "error" in body or "result" not in body:
        raise AssertionError(f"JSON-RPC request failed: {body!r}")
    result = body["result"]
    if expected == "block-number" and (not isinstance(result, str) or not HEX_QUANTITY.fullmatch(result)):
        raise AssertionError(f"invalid eth_blockNumber result: {result!r}")
    if expected == "capabilities" and (
        not isinstance(result, list)
        or not result
        or not all(isinstance(item, str) and item.startswith("engine_") for item in result)
    ):
        raise AssertionError(f"invalid Engine API capabilities result: {result!r}")
    return result


def http_rpc(host, port, method, params, expected, token=None):
    headers = {"Content-Type": "application/json"}
    if token is not None:
        headers["Authorization"] = f"Bearer {token}"
    request = Request(
        f"http://{host}:{port}",
        data=rpc_request(method, params),
        headers=headers,
        method="POST",
    )
    with OPENER.open(request, timeout=2) as response:
        if response.status != 200:
            raise AssertionError(f"unexpected HTTP status: {response.status}")
        return checked_response(response.read(65537), expected)


def b64url(data):
    return base64.urlsafe_b64encode(data).rstrip(b"=")


def jwt_token(secret_hex):
    secret = bytes.fromhex(secret_hex)
    if len(secret) != 32:
        raise ValueError("JWT secret must contain 32 bytes")
    header = b64url(b'{"alg":"HS256","typ":"JWT"}')
    claims = b64url(json.dumps({"iat": int(time.time())}).encode("ascii"))
    signed = header + b"." + claims
    signature = b64url(hmac.new(secret, signed, hashlib.sha256).digest())
    return (signed + b"." + signature).decode("ascii")


def read_exact(connection, count, initial=b""):
    data = bytearray(initial)
    while len(data) < count:
        chunk = connection.recv(count - len(data))
        if not chunk:
            raise AssertionError("WebSocket closed during response")
        data.extend(chunk)
    return bytes(data[:count]), bytes(data[count:])


def websocket_rpc(host, port):
    with socket.create_connection((host, port), timeout=2) as connection:
        connection.settimeout(2)
        key = base64.b64encode(os.urandom(16))
        request = (
            f"GET / HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\n"
            "Connection: Upgrade\r\nSec-WebSocket-Version: 13\r\n"
            f"Sec-WebSocket-Key: {key.decode('ascii')}\r\n\r\n"
        ).encode("ascii")
        connection.sendall(request)
        headers = bytearray()
        while b"\r\n\r\n" not in headers:
            chunk = connection.recv(4096)
            if not chunk or len(headers) + len(chunk) > 16384:
                raise AssertionError("invalid WebSocket handshake")
            headers.extend(chunk)
        head, remaining = bytes(headers).split(b"\r\n\r\n", 1)
        lines = head.split(b"\r\n")
        fields = dict(line.split(b":", 1) for line in lines[1:])
        normalized = {name.lower(): value.strip() for name, value in fields.items()}
        expected_accept = base64.b64encode(hashlib.sha1(key + WS_GUID).digest())
        if not lines[0].startswith(b"HTTP/1.1 101 ") or normalized.get(b"sec-websocket-accept") != expected_accept:
            raise AssertionError(f"WebSocket upgrade failed: {lines[0]!r}")

        payload = rpc_request("eth_blockNumber", [])
        mask = os.urandom(4)
        if len(payload) >= 126:
            raise AssertionError("probe request unexpectedly large")
        frame = bytes((0x81, 0x80 | len(payload))) + mask + bytes(
            byte ^ mask[index % 4] for index, byte in enumerate(payload)
        )
        connection.sendall(frame)
        header, remaining = read_exact(connection, 2, remaining)
        if header[0] != 0x81 or header[1] & 0x80:
            raise AssertionError(f"unexpected WebSocket frame: {header!r}")
        length = header[1] & 0x7F
        if length == 126:
            extended, remaining = read_exact(connection, 2, remaining)
            length = struct.unpack("!H", extended)[0]
        elif length == 127:
            raise AssertionError("WebSocket response exceeds probe limit")
        if length > 65536:
            raise AssertionError("WebSocket response exceeds probe limit")
        response, _ = read_exact(connection, length, remaining)
        return checked_response(response, "block-number")


def unauthorized(host, port, token=None):
    try:
        http_rpc(host, port, "engine_exchangeCapabilities", [[]], "capabilities", token)
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
        except AssertionError:
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
