"""Independent wire peers exercise the adapter helper, including its CLI boundary."""

import base64
from contextlib import contextmanager
import hashlib
import hmac
import importlib.util
from importlib.machinery import SourceFileLoader
import json
from pathlib import Path
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import unittest


HELPER = Path(sys.argv.pop(1)) if len(sys.argv) > 1 else Path(__file__).parent.parent / "adapters/reth-probe.py"
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_loader("reth_probe", SourceFileLoader("reth_probe", str(HELPER)))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


@contextmanager
def peer(handler, host="127.0.0.1"):
    errors = []
    with socket.socket(socket.AF_INET6 if ":" in host else socket.AF_INET) as listener:
        listener.bind((host, 0))
        listener.listen(1)
        listener.settimeout(5)

        def serve():
            try:
                with listener.accept()[0] as connection:
                    connection.settimeout(5)
                    handler(connection)
            except Exception as error:
                errors.append(error)

        thread = threading.Thread(target=serve)
        thread.start()
        try:
            yield listener.getsockname()[1]
        finally:
            thread.join(6)
            if thread.is_alive():
                raise AssertionError("probe peer did not settle")
            if errors:
                raise errors[0]


def request(connection):
    # Buffered reads preserve request bytes coalesced with the final header.
    stream = connection.makefile("rb")
    first = stream.readline().strip()
    headers = {}
    while (line := stream.readline()) != b"\r\n":
        if not line:
            raise AssertionError("incomplete request")
        name, value = line.split(b":", 1)
        headers[name.lower()] = value.strip()
    return stream, first, headers


def rpc_peer(body, status=200, inspect=lambda headers, body: None):
    def serve(connection):
        stream, first, headers = request(connection)
        assert first == b"POST / HTTP/1.1"
        raw = stream.read(int(headers[b"content-length"]))
        inspect(headers, json.loads(raw))
        raw = body if isinstance(body, bytes) else json.dumps(body).encode()
        connection.sendall(f"HTTP/1.1 {status} Test\r\nContent-Length: {len(raw)}\r\nConnection: close\r\n\r\n".encode() + raw)
        stream.close()
    return serve


def ws_peer(fault=None):
    def serve(connection):
        stream, first, headers = request(connection)
        assert first == b"GET / HTTP/1.1"
        assert headers[b"upgrade"] == b"websocket"
        assert headers[b"sec-websocket-version"] == b"13"
        key = headers[b"sec-websocket-key"]
        assert len(base64.b64decode(key, validate=True)) == 16
        accept = base64.b64encode(hashlib.sha1(key + b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11").digest())
        upgrade = b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: keep-alive, Upgrade\r\nSec-WebSocket-Accept: " + accept
        if fault == "accept":
            upgrade = upgrade.replace(accept, b"bad")
        elif fault == "upgrade":
            upgrade = upgrade.replace(b"Upgrade: websocket", b"Upgrade: h2c")
        elif fault == "connection":
            upgrade = upgrade.replace(b"keep-alive, Upgrade", b"close")
        elif fault == "duplicate":
            upgrade += b"\r\nSec-WebSocket-Accept: " + accept
        connection.sendall(upgrade + b"\r\n\r\n")
        if fault in ("accept", "upgrade", "connection", "duplicate"):
            stream.close()
            return
        header = stream.read(2)
        assert header[0] == 0x81 and header[1] & 0x80
        length = header[1] & 0x7f
        assert length < 126
        mask = stream.read(4)
        raw = stream.read(length)
        body = json.loads(bytes(byte ^ mask[i % 4] for i, byte in enumerate(raw)))
        assert body == {"jsonrpc": "2.0", "method": "eth_blockNumber", "params": [], "id": 1}
        raw = json.dumps({"jsonrpc": "2.0", "id": 2 if fault == "id" else 1, "result": "0x2a"}).encode()
        if fault == "extended":
            raw += b" " * 150
            frame = bytes([0x81, 126]) + struct.pack("!H", len(raw)) + raw
        else:
            frame = bytes([0x82 if fault == "binary" else 0x81, len(raw)]) + raw
        connection.sendall(frame)
        stream.close()
    return serve


class ProtocolTests(unittest.TestCase):
    def run_probe(self, mode, port, state=None, host="127.0.0.1"):
        command = [sys.executable, str(HELPER)] if HELPER.suffix == ".py" else [str(HELPER)]
        args = command + [mode, host, str(port)]
        if state is not None:
            args.append(str(state))
        return subprocess.run(args, capture_output=True, timeout=5)

    def assert_success(self, result):
        self.assertEqual((result.returncode, result.stdout, result.stderr), (0, b"", b""))

    def assert_failure(self, result):
        self.assertEqual((result.returncode, result.stdout, result.stderr),
                         (1, b"", b"Reth endpoint probe failed\n"))

    def test_http_targets_supplied_ipv4_and_ipv6_ports(self):
        def inspect(headers, body):
            self.assertNotIn(b"authorization", headers)
            self.assertEqual(body, {"jsonrpc": "2.0", "id": 1, "method": "eth_blockNumber", "params": []})
        for host in ("127.0.0.1", "::1"):
            with self.subTest(host=host), peer(rpc_peer({"jsonrpc": "2.0", "id": 1, "result": "0x0"}, inspect=inspect), host) as port:
                self.assert_success(self.run_probe("http", port, host=host))

    def test_http_rejects_error_envelopes_ids_shapes_and_oversize(self):
        invalid = [
            {"jsonrpc": "2.0", "id": 1, "error": {"message": "private remote bytes"}},
            {"jsonrpc": "2.0", "id": 1, "result": "0x1", "error": None},
            {"jsonrpc": "2.0", "id": 1},
            {"jsonrpc": "1.0", "id": 1, "result": "0x1"},
            *({"jsonrpc": "2.0", "id": value, "result": "0x1"} for value in (2, True, 1.0, "1", None)),
            *({"jsonrpc": "2.0", "id": 1, "result": value} for value in ("0x", "0x00", "0xzz", 1, None, [])),
            [], b"not JSON: private remote bytes", b" " * 65537,
        ]
        for body in invalid:
            with self.subTest(body=str(body)[:120]), peer(rpc_peer(body)) as port:
                self.assert_failure(self.run_probe("http", port))
        for status in (204, 301, 401, 403, 500):
            with self.subTest(status=status), peer(rpc_peer({}, status)) as port:
                self.assert_failure(self.run_probe("http", port))

    def test_websocket_upgrade_masked_exchange_and_rejections(self):
        for fault in (None, "extended", "accept", "upgrade", "connection", "duplicate", "id", "binary"):
            with self.subTest(fault=fault), peer(ws_peer(fault)) as port:
                result = self.run_probe("ws", port)
                (self.assert_success if fault in (None, "extended") else self.assert_failure)(result)

    def test_authentication_reads_state_secret_and_signs_fresh_jwt(self):
        secret = bytes(range(32))

        def inspect(headers, body):
            self.assertEqual(body, {"jsonrpc": "2.0", "id": 1, "method": "engine_exchangeCapabilities", "params": [[]]})
            token = headers[b"authorization"].removeprefix(b"Bearer ")
            head, claims, signature = token.split(b".")
            decode = lambda part: base64.urlsafe_b64decode(part + b"=" * (-len(part) % 4))
            self.assertEqual(json.loads(decode(head)), {"alg": "HS256", "typ": "JWT"})
            self.assertLessEqual(abs(time.time() - json.loads(decode(claims))["iat"]), 5)
            self.assertEqual(decode(signature), hmac.new(secret, head + b"." + claims, hashlib.sha256).digest())

        with tempfile.TemporaryDirectory() as state:
            path = Path(state) / "reth/config/jwt.hex"
            path.parent.mkdir(parents=True)
            path.write_text(secret.hex())
            for value in (["engine_newPayloadV1"], [], ["eth_blockNumber"], ["engine_"], [1], "private secret response"):
                with self.subTest(value=value), peer(rpc_peer({"jsonrpc": "2.0", "id": 1, "result": value}, inspect=inspect)) as port:
                    result = self.run_probe("authrpc", port, state)
                    (self.assert_success if value == ["engine_newPayloadV1"] else self.assert_failure)(result)
            path.write_text("invalid secret")
            self.assert_failure(self.run_probe("authrpc", 1, state))
            path.unlink()
            self.assert_failure(self.run_probe("authrpc", 1, state))

    def test_missing_and_wrong_jwt_refused_by_peer(self):
        def unauthorized(connection):
            stream, _, headers = request(connection)
            stream.read(int(headers[b"content-length"]))
            self.assertNotEqual(headers.get(b"authorization"), b"Bearer expected-token")
            connection.sendall(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n")
            stream.close()
        for token in (None, probe.jwt_token("11" * 32)):
            with peer(unauthorized) as port, self.assertRaises(ValueError):
                probe.http_rpc("127.0.0.1", port, "engine_exchangeCapabilities", [[]], "capabilities", token)


if __name__ == "__main__":
    unittest.main()
