// Independent HTTP responses, a native-command witness, and a real RLPx peer.
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
        // A inherited proxy must never redirect the identity request.
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
            assert_eq!(
                request,
                br#"{"jsonrpc":"2.0","id":1,"method":"admin_nodeInfo","params":[]}"#
            );
            // A size rejection can close the connection before consuming a body.
            let _ = write!(
                socket,
                "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
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

    struct Node(Child);

    impl Drop for Node {
        fn drop(&mut self) {
            let _ = self.0.kill();
            self.0.wait().unwrap();
        }
    }

    #[test]
    fn real_reth_handshake_succeeds_and_tcp_listener_alone_fails() {
        let state = State::new();
        let http = TcpListener::bind("127.0.0.1:0").unwrap();
        let peer = TcpListener::bind("127.0.0.1:0").unwrap();
        let http_port = http.local_addr().unwrap().port().to_string();
        let peer_port = peer.local_addr().unwrap().port().to_string();
        drop((http, peer));
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
                    "--disable-auth-server",
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
