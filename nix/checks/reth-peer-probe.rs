// Independent HTTP responses, a native-command witness, and real Reth protocols.
#[cfg(not(test))]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let expected = std::env::var("RETH_EXPECTED_TARGET").unwrap();
    std::fs::write(std::env::var("RETH_WITNESS").unwrap(), args.join("\n")).unwrap();
    println!("private native response");
    eprintln!("private native error");
    assert_eq!(
        args,
        [
            "p2p",
            "rlpx",
            "ping",
            &expected,
            "--quiet",
            "--log.file.max-files",
            "0"
        ]
    );
    std::process::exit(if std::env::var_os("RETH_FAIL").is_some() {
        1
    } else {
        0
    });
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::process::{Child, Command, Output, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};

    const PROBE: &str = env!("NIXFIED_TEST_RETH_PROBE");
    const REAL_PROBE: &str = env!("NIXFIED_TEST_REAL_RETH_PROBE");
    const RETH: &str = env!("NIXFIED_TEST_RETH");
    const CURL: &str = env!("NIXFIED_TEST_CURL");
    const KEY: &str = "11111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111";

    struct State(PathBuf);

    impl State {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "nixfied-peer-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for State {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn command(probe: &str, args: &[&str]) -> Command {
        let mut command = Command::new(probe);
        command.env_clear().args(args).stdin(Stdio::null());
        // An inherited proxy must never redirect a probe request.
        command.env("http_proxy", "http://127.0.0.1:1");
        command
    }

    fn assert_result(output: Output, success: bool) {
        assert_eq!(output.status.code(), Some(if success { 0 } else { 1 }));
        assert!(output.stdout.is_empty());
        assert_eq!(
            output.stderr,
            if success {
                b"".as_slice()
            } else {
                b"Reth endpoint probe failed\n".as_slice()
            }
        );
    }

    fn identity_response(enode: &str) -> String {
        format!(r#"{{"jsonrpc":"2.0","id":1,"result":{{"enode":"{enode}"}}}}"#)
    }

    fn http_peer(host: &str, status: u16, body: String) -> (u16, thread::JoinHandle<()>) {
        rpc_peer(host, status, body, "admin_nodeInfo")
    }

    fn rpc_peer(
        host: &str,
        status: u16,
        body: String,
        method: &'static str,
    ) -> (u16, thread::JoinHandle<()>) {
        let response = format!(
            "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        rpc_peer_raw(host, response, method)
    }

    fn rpc_peer_raw(
        host: &str,
        response: String,
        method: &'static str,
    ) -> (u16, thread::JoinHandle<()>) {
        let listener = TcpListener::bind((host, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let thread = thread::spawn(move || {
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(10))
                    }
                    error => panic!("HTTP probe did not connect: {error:?}"),
                }
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            loop {
                let mut byte = [0];
                socket.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
                if request.ends_with(b"\r\n\r\n") {
                    break;
                }
                assert!(request.len() < 16384);
            }
            let headers = String::from_utf8(request).unwrap();
            if method == "websocket" {
                assert!(headers.starts_with("GET / HTTP/1.1\r\n"));
                socket.write_all(response.as_bytes()).unwrap();
                return;
            }
            assert!(headers.starts_with("POST / HTTP/1.1\r\n"));
            assert!(headers.contains("Content-Type: application/json\r\n"));
            let length: usize = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .unwrap()
                .parse()
                .unwrap();
            let mut request = vec![0; length];
            socket.read_exact(&mut request).unwrap();
            let params = if method == "engine_exchangeCapabilities" {
                "[[]]"
            } else {
                "[]"
            };
            assert_eq!(
                String::from_utf8(request).unwrap(),
                format!(r#"{{"jsonrpc":"2.0","id":1,"method":"{method}","params":{params}}}"#)
            );
            assert_eq!(
                headers.contains("Authorization: Bearer "),
                method == "engine_exchangeCapabilities"
            );
            // A size rejection can close the connection before consuming a body.
            let _ = socket.write_all(response.as_bytes());
        });
        (port, thread)
    }

    fn fixture_request(
        host: &str,
        status: u16,
        body: String,
        native_failure: bool,
        admitted: bool,
    ) {
        let state = State::new();
        let witness = state.0.join("native-args");
        let (http_port, thread) = http_peer(host, status, body);
        let authority = if host == "::1" { "[::1]" } else { host };
        let expected = format!("enode://{KEY}@{authority}:24567");
        let mut command = command(PROBE, &["peer", host, "24567", &http_port.to_string()]);
        command
            .env("RETH_EXPECTED_TARGET", expected)
            .env("RETH_WITNESS", &witness);
        if native_failure {
            command.env("RETH_FAIL", "1");
        }
        assert_result(command.output().unwrap(), admitted && !native_failure);
        thread.join().unwrap();
        assert_eq!(
            witness.exists(),
            admitted,
            "malformed HTTP must not invoke Reth"
        );
    }

    #[test]
    fn planned_ipv4_and_ipv6_targets_replace_advertised_address() {
        for host in ["127.0.0.1", "::1"] {
            fixture_request(
                host,
                200,
                identity_response(&format!("enode://{KEY}@203.0.113.9:30303?discport=30304")),
                false,
                true,
            );
        }
    }

    #[test]
    fn rejects_http_errors_and_malformed_identity_before_native_handshake() {
        for body in [
            "private remote bytes".to_owned(),
            r#"{"jsonrpc":"2.0","id":1,"error":{"message":"private error"}}"#.to_owned(),
            r#"{"jsonrpc":"2.0","id":2,"result":{"enode":"private identity"}}"#.to_owned(),
            r#"{"jsonrpc":"1.0","id":1,"result":{"enode":"private identity"}}"#.to_owned(),
            r#"{"jsonrpc":"2.0","id":true,"result":{"enode":"private identity"}}"#.to_owned(),
            r#"{"jsonrpc":"2.0","id":1,"result":{}}"#.to_owned(),
            identity_response("enode://abcd@127.0.0.1:1"),
            identity_response(&format!("enode://{}@127.0.0.1:1", "z".repeat(128))),
            identity_response(&format!("enode://{KEY}")),
            format!(
                "{}\n{{}}",
                identity_response(&format!("enode://{KEY}@127.0.0.1:1"))
            ),
            " ".repeat(65537),
        ] {
            fixture_request("127.0.0.1", 200, body, false, false);
        }
        for status in [301, 401, 500] {
            fixture_request(
                "127.0.0.1",
                status,
                identity_response(&format!("enode://{KEY}@127.0.0.1:1")),
                false,
                false,
            );
        }
    }

    #[test]
    fn native_failure_is_redaction_safe() {
        fixture_request(
            "127.0.0.1",
            200,
            identity_response(&format!("enode://{KEY}@127.0.0.1:1")),
            true,
            true,
        );
    }

    #[test]
    fn invalid_arguments_reject_before_http_or_native_effects() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port().to_string();
        let state = State::new();
        let witness = state.0.join("native-args");
        for args in [
            vec!["peer"],
            vec![],
            vec!["invalid", "127.0.0.1", &port],
            vec!["http", "localhost", &port],
            vec!["ws", "203.0.113.1", &port],
            vec!["http", "127.0.0.1", &port, "extra"],
            vec!["authrpc", "127.0.0.1", &port],
            vec!["peer", "localhost", "1234", &port],
            vec!["peer", "203.0.113.1", "1234", &port],
            vec!["peer", "127.256.0.1", "1234", &port],
            vec!["peer", "127.0.0.1", "0", &port],
            vec!["peer", "127.0.0.1", "65536", &port],
            vec!["peer", "127.0.0.1", "--quiet", &port],
            vec!["peer", "127.0.0.1", "1234", "0"],
            vec!["peer", "127.0.0.1", "1234", &port, "extra"],
        ] {
            assert_result(
                command(PROBE, &args)
                    .env("RETH_WITNESS", &witness)
                    .output()
                    .unwrap(),
                false,
            );
        }
        assert!(!witness.exists());
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn http_checks_rpc_semantics_at_supplied_ipv4_and_ipv6_endpoints() {
        for host in ["127.0.0.1", "::1"] {
            let (port, peer) = rpc_peer(
                host,
                200,
                r#"{"jsonrpc":"2.0","id":1,"result":"0x0"}"#.into(),
                "eth_blockNumber",
            );
            assert_result(
                command(PROBE, &["http", host, &port.to_string()])
                    .output()
                    .unwrap(),
                true,
            );
            peer.join().unwrap();
        }
        for body in [
            r#"{"jsonrpc":"2.0","id":1,"error":{"message":"private error"}}"#.to_owned(),
            r#"{"jsonrpc":"2.0","id":1,"result":"0x1","error":null}"#.to_owned(),
            r#"{"jsonrpc":"2.0","id":1}"#.to_owned(),
            r#"{"jsonrpc":"1.0","id":1,"result":"0x1"}"#.to_owned(),
            r#"{"jsonrpc":"2.0","id":2,"result":"0x1"}"#.to_owned(),
            r#"{"jsonrpc":"2.0","id":true,"result":"0x1"}"#.to_owned(),
            r#"{"jsonrpc":"2.0","id":"1","result":"0x1"}"#.to_owned(),
            r#"{"jsonrpc":"2.0","id":1,"result":"0x00"}"#.to_owned(),
            r#"{"jsonrpc":"2.0","id":1,"result":"0xzz"}"#.to_owned(),
            r#"{"jsonrpc":"2.0","id":1,"result":1}"#.to_owned(),
            r#"{"jsonrpc":"2.0","id":1,"result":"0x1"} {}"#.to_owned(),
            "[]".to_owned(),
            "private invalid JSON".to_owned(),
            " ".repeat(65537),
        ] {
            let (port, peer) = rpc_peer("127.0.0.1", 200, body, "eth_blockNumber");
            assert_result(
                command(PROBE, &["http", "127.0.0.1", &port.to_string()])
                    .output()
                    .unwrap(),
                false,
            );
            peer.join().unwrap();
        }
        for status in [204, 301, 401, 403, 500] {
            let (port, peer) = rpc_peer(
                "127.0.0.1",
                status,
                r#"{"jsonrpc":"2.0","id":1,"result":"0x0"}"#.into(),
                "eth_blockNumber",
            );
            assert_result(
                command(PROBE, &["http", "127.0.0.1", &port.to_string()])
                    .output()
                    .unwrap(),
                false,
            );
            peer.join().unwrap();
        }
    }

    #[test]
    fn curl_ignores_config_redirects_and_bounds_chunked_bodies() {
        let state = State::new();
        fs::write(state.0.join(".curlrc"), "url = http://127.0.0.1:1\n").unwrap();
        // Numeric JSON-RPC IDs compare by value; booleans and strings reject.
        let body = r#"{"jsonrpc":"2.0","id":1.0,"result":"0x0"}"#;
        let (port, peer) = rpc_peer("127.0.0.1", 200, body.into(), "eth_blockNumber");
        assert_result(
            command(PROBE, &["http", "127.0.0.1", &port.to_string()])
                .env("CURL_HOME", &state.0)
                .output()
                .unwrap(),
            true,
        );
        peer.join().unwrap();
        for (body, success) in [
            (body.to_owned(), true),
            (format!("{body}{}", " ".repeat(65536)), false),
        ] {
            let response = format!(
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n",
                body.len()
            );
            let (port, peer) = rpc_peer_raw("127.0.0.1", response, "eth_blockNumber");
            assert_result(
                command(PROBE, &["http", "127.0.0.1", &port.to_string()])
                    .output()
                    .unwrap(),
                success,
            );
            peer.join().unwrap();
        }
        let redirect = TcpListener::bind("127.0.0.1:0").unwrap();
        redirect.set_nonblocking(true).unwrap();
        let redirect_port = redirect.local_addr().unwrap().port();
        let response = format!(
            "HTTP/1.1 301 Moved\r\nLocation: http://127.0.0.1:{redirect_port}/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let (port, peer) = rpc_peer_raw("127.0.0.1", response, "eth_blockNumber");
        assert_result(
            command(PROBE, &["http", "127.0.0.1", &port.to_string()])
                .output()
                .unwrap(),
            false,
        );
        peer.join().unwrap();
        assert_eq!(
            redirect.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn websocket_http_response_is_not_a_successful_exchange() {
        let (port, peer) = rpc_peer(
            "127.0.0.1",
            200,
            r#"{"jsonrpc":"2.0","id":1,"result":"0x0"}"#.into(),
            "websocket",
        );
        assert_result(
            command(PROBE, &["ws", "127.0.0.1", &port.to_string()])
                .output()
                .unwrap(),
            false,
        );
        peer.join().unwrap();
    }

    #[test]
    fn engine_capabilities_and_secret_files_fail_closed() {
        let state = State::new();
        fs::create_dir_all(state.0.join("reth/config")).unwrap();
        let path = state.0.join("reth/config/jwt.hex");
        fs::write(&path, format!("{}\n", "01".repeat(32))).unwrap();
        for (value, success) in [
            (r#"["engine_newPayloadV1"]"#, true),
            ("[]", false),
            (r#"["eth_blockNumber"]"#, false),
            (r#"["engine_"]"#, false),
            ("[1]", false),
            (r#""private response""#, false),
        ] {
            let (port, peer) = rpc_peer(
                "127.0.0.1",
                200,
                format!(r#"{{"jsonrpc":"2.0","id":1,"result":{value}}}"#),
                "engine_exchangeCapabilities",
            );
            assert_result(
                command(
                    PROBE,
                    &[
                        "authrpc",
                        "127.0.0.1",
                        &port.to_string(),
                        state.0.to_str().unwrap(),
                    ],
                )
                .output()
                .unwrap(),
                success,
            );
            peer.join().unwrap();
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port().to_string();
        for content in [
            "not hex".to_owned(),
            "00".repeat(31),
            "00".repeat(33),
            format!("{}\n\n\n", "00".repeat(32)),
        ] {
            fs::write(&path, content).unwrap();
            assert_result(
                command(
                    PROBE,
                    &["authrpc", "127.0.0.1", &port, state.0.to_str().unwrap()],
                )
                .output()
                .unwrap(),
                false,
            );
        }
        fs::remove_file(path).unwrap();
        assert_result(
            command(
                PROBE,
                &["authrpc", "127.0.0.1", &port, state.0.to_str().unwrap()],
            )
            .output()
            .unwrap(),
            false,
        );
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    struct Node(Child);

    impl Drop for Node {
        fn drop(&mut self) {
            let _ = self.0.kill();
            self.0.wait().unwrap();
        }
    }

    #[test]
    fn real_reth_protocols_authentication_and_peer_handshake() {
        let state = State::new();
        let http = TcpListener::bind("127.0.0.1:0").unwrap();
        let peer = TcpListener::bind("127.0.0.1:0").unwrap();
        let http_port = http.local_addr().unwrap().port().to_string();
        let peer_port = peer.local_addr().unwrap().port().to_string();
        let ws = TcpListener::bind("127.0.0.1:0").unwrap();
        let auth = TcpListener::bind("127.0.0.1:0").unwrap();
        let ws_port = ws.local_addr().unwrap().port().to_string();
        let auth_port = auth.local_addr().unwrap().port().to_string();
        drop((http, peer, ws, auth));
        fs::create_dir_all(state.0.join("reth/config")).unwrap();
        let secret_path = state.0.join("reth/config/jwt.hex");
        let secret = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
        fs::write(&secret_path, secret).unwrap();
        let mut node = Node(
            Command::new(RETH)
                .env_clear()
                .args([
                    "node",
                    "--dev",
                    "--datadir",
                    state.0.to_str().unwrap(),
                    "--ipcdisable",
                    "--disable-discovery",
                    "--ws",
                    "--ws.addr",
                    "127.0.0.1",
                    "--ws.port",
                    &ws_port,
                    "--authrpc.addr",
                    "127.0.0.1",
                    "--authrpc.port",
                    &auth_port,
                    "--authrpc.jwtsecret",
                    secret_path.to_str().unwrap(),
                    "--addr",
                    "127.0.0.1",
                    "--port",
                    &peer_port,
                    "--max-inbound-peers",
                    "1",
                    "--max-outbound-peers",
                    "0",
                    "--http",
                    "--http.addr",
                    "127.0.0.1",
                    "--http.port",
                    &http_port,
                    "--http.api",
                    "eth,admin",
                    "--quiet",
                    "--log.file.max-files",
                    "0",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            assert!(
                node.0.try_wait().unwrap().is_none(),
                "Reth exited during startup"
            );
            if command(REAL_PROBE, &["http", "127.0.0.1", &http_port])
                .output()
                .unwrap()
                .status
                .success()
            {
                break;
            }
            assert!(Instant::now() < deadline, "Reth HTTP did not become ready");
            thread::sleep(Duration::from_millis(100));
        }
        assert_result(
            command(REAL_PROBE, &["ws", "127.0.0.1", &ws_port])
                .output()
                .unwrap(),
            true,
        );
        assert_result(
            command(
                REAL_PROBE,
                &[
                    "authrpc",
                    "127.0.0.1",
                    &auth_port,
                    state.0.to_str().unwrap(),
                ],
            )
            .output()
            .unwrap(),
            true,
        );
        // The actual node independently validates the signature and fresh iat.
        fs::write(&secret_path, "ff".repeat(32)).unwrap();
        assert_result(
            command(
                REAL_PROBE,
                &[
                    "authrpc",
                    "127.0.0.1",
                    &auth_port,
                    state.0.to_str().unwrap(),
                ],
            )
            .output()
            .unwrap(),
            false,
        );
        fs::write(&secret_path, secret).unwrap();
        let missing_auth = Command::new(CURL)
            .env_clear()
            .args([
                "-q",
                "--silent",
                "--noproxy",
                "*",
                "--max-time",
                "2",
                "--output",
                "/dev/null",
                "--write-out",
                "%{http_code}",
                "--header",
                "Content-Type: application/json",
                "--data",
                r#"{"jsonrpc":"2.0","id":1,"method":"engine_exchangeCapabilities","params":[[]]}"#,
                &format!("http://127.0.0.1:{auth_port}"),
            ])
            .output()
            .unwrap();
        assert!(missing_auth.status.success());
        assert!(matches!(missing_auth.stdout.as_slice(), b"401" | b"403"));
        for _ in 0..2 {
            assert_result(
                command(REAL_PROBE, &["peer", "127.0.0.1", &peer_port, &http_port])
                    .output()
                    .unwrap(),
                true,
            );
        }
        // HTTP still answers, but a TCP acceptor without RLPx must fail.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let bad_port = listener.local_addr().unwrap().port().to_string();
        let thread = thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        socket.write_all(b"private invalid handshake").unwrap();
                        break;
                    }
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(10))
                    }
                    error => panic!("Reth did not attempt handshake: {error:?}"),
                }
            }
        });
        assert_result(
            command(REAL_PROBE, &["peer", "127.0.0.1", &bad_port, &http_port])
                .output()
                .unwrap(),
            false,
        );
        thread.join().unwrap();
        // The peer listener stays up while the supplied identity endpoint is closed.
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let closed_port = closed.local_addr().unwrap().port().to_string();
        drop(closed);
        assert_result(
            command(REAL_PROBE, &["peer", "127.0.0.1", &peer_port, &closed_port])
                .output()
                .unwrap(),
            false,
        );
        assert!(TcpStream::connect(("127.0.0.1", peer_port.parse::<u16>().unwrap())).is_ok());
    }
}
