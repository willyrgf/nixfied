#!/usr/bin/env python3
"""Adapter-owned, short-lived Reth protocol probes; no response or secret output."""

import base64
import hashlib
import hmac
import http.client
import ipaddress
import json
import os
from pathlib import Path
import re
import socket
import struct
import sys
import time
RPC_ID = 1
RESPONSE_LIMIT = 65536
HEX_QUANTITY = re.compile(r"^0x(?:0|[1-9a-fA-F][0-9a-fA-F]*)$")
WS_GUID = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11"


def rpc_request(method, params):
    return json.dumps(
        {"jsonrpc": "2.0", "method": method, "params": params, "id": RPC_ID},
        separators=(",", ":"),
    ).encode("ascii")


def checked_response(raw, expected):
    if len(raw) > RESPONSE_LIMIT:
        raise ValueError("response exceeds probe limit")
    body = json.loads(raw)
    if (not isinstance(body, dict) or body.get("jsonrpc") != "2.0"
            or type(body.get("id")) is not int or body["id"] != RPC_ID):
        raise ValueError("invalid JSON-RPC envelope")
    if "error" in body or "result" not in body:
        raise ValueError("JSON-RPC request failed")
    result = body["result"]
    if expected == "block-number" and (not isinstance(result, str) or not HEX_QUANTITY.fullmatch(result)):
        raise ValueError("invalid eth_blockNumber result")
    if expected == "capabilities" and (
        not isinstance(result, list)
        or not result
        or not all(isinstance(item, str) and re.fullmatch(r"engine_[A-Za-z0-9]+", item) for item in result)
    ):
        raise ValueError("invalid Engine API capabilities result")
    return result


def http_rpc(host, port, method, params, expected, token=None):
    headers = {"Content-Type": "application/json"}
    if token is not None:
        headers["Authorization"] = f"Bearer {token}"
    # Direct connection: no inherited proxy, redirect, or token-bearing child.
    connection = http.client.HTTPConnection(host, port, timeout=2)
    try:
        connection.request("POST", "/", rpc_request(method, params), headers)
        response = connection.getresponse()
        if response.status != 200:
            raise ValueError("unexpected HTTP status")
        return checked_response(response.read(RESPONSE_LIMIT + 1), expected)
    finally:
        connection.close()


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
        authority = f"[{host}]" if ":" in host else host
        request = (
            f"GET / HTTP/1.1\r\nHost: {authority}:{port}\r\nUpgrade: websocket\r\n"
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
        normalized = {}
        for line in lines[1:]:
            name, value = line.split(b":", 1)
            name = name.lower()
            if name in normalized:
                raise ValueError("duplicate WebSocket header")
            normalized[name] = value.strip()
        expected_accept = base64.b64encode(hashlib.sha1(key + WS_GUID).digest())
        if (not lines[0].startswith(b"HTTP/1.1 101 ")
                or normalized.get(b"sec-websocket-accept") != expected_accept
                or normalized.get(b"upgrade", b"").lower() != b"websocket"
                or b"upgrade" not in [token.strip().lower() for token in normalized.get(b"connection", b"").split(b",")]
                or b"sec-websocket-extensions" in normalized
                or b"sec-websocket-protocol" in normalized):
            raise ValueError("WebSocket upgrade failed")

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
            raise ValueError("unexpected WebSocket frame")
        length = header[1] & 0x7F
        if length == 126:
            extended, remaining = read_exact(connection, 2, remaining)
            length = struct.unpack("!H", extended)[0]
            if length < 126:
                raise ValueError("noncanonical WebSocket frame length")
        elif length == 127:
            raise AssertionError("WebSocket response exceeds probe limit")
        if length > 65536:
            raise AssertionError("WebSocket response exceeds probe limit")
        response, _ = read_exact(connection, length, remaining)
        return checked_response(response, "block-number")


def main(args):
    if len(args) not in (3, 4) or args[0] not in ("http", "ws", "authrpc"):
        raise ValueError("invalid probe arguments")
    mode, host, port = args[:3]
    if len(args) != (4 if mode == "authrpc" else 3):
        raise ValueError("invalid probe arguments")
    if not ipaddress.ip_address(host).is_loopback or not 0 < int(port) <= 65535:
        raise ValueError("invalid endpoint")
    if mode == "ws":
        websocket_rpc(host, int(port))
    elif mode == "http":
        http_rpc(host, int(port), "eth_blockNumber", [], "block-number")
    else:
        secret = (Path(args[3]) / "reth/config/jwt.hex").read_text(encoding="ascii").strip()
        http_rpc(host, int(port), "engine_exchangeCapabilities", [[]],
                 "capabilities", jwt_token(secret))


if __name__ == "__main__":
    try:
        main(sys.argv[1:])
    except Exception:
        # Remote bytes, exception text, paths, and credentials never reach output.
        print("Reth endpoint probe failed", file=sys.stderr)
        sys.exit(1)
