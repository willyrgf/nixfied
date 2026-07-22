use std::env;
use std::fs;
use std::io::{self, Read};
use std::net::{Ipv4Addr, Shutdown, SocketAddrV4, TcpListener, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

const MARKER_TIMEOUT: Duration = Duration::from_secs(15);

fn main() {
    if let Err(error) = run(env::args().skip(1).collect()) {
        eprintln!("nixfied-test-child: {error}");
        std::process::exit(70);
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let Some((command, args)) = args.split_first() else {
        return Err("missing command".into());
    };
    match command.as_str() {
        "prepare" => prepare(args),
        "listen" => listen(args),
        "connect" => connect(args),
        _ => Err(format!("unsupported command {command:?}")),
    }
}

fn prepare(args: &[String]) -> Result<(), String> {
    if !(1..=2).contains(&args.len()) {
        return Err("prepare expects SENTINEL [ACKNOWLEDGEMENT]".into());
    }
    let sentinel = &args[0];
    touch(Path::new(sentinel))?;
    if let Some(acknowledgement) = args.get(1) {
        wait_for_path(Path::new(acknowledgement), MARKER_TIMEOUT)?;
    }
    Ok(())
}

fn listen(args: &[String]) -> Result<(), String> {
    let [address, port, mode, tail @ ..] = args else {
        return Err(
            "listen expects ADDRESS PORT (hold|active-close|close-on-marker REQUEST CLOSED)".into(),
        );
    };
    let address = parse_address(address)?;
    let port = parse_port(port)?;
    let listener = reusable_listener(address, port)?;

    match (mode.as_str(), tail) {
        ("hold", []) => accept_forever(listener, false),
        ("active-close", []) => accept_forever(listener, true),
        ("close-on-marker", [request, closed]) => {
            close_on_marker(listener, Path::new(request), Path::new(closed))
        }
        _ => Err(format!("invalid listen mode or arguments: {mode:?}")),
    }
}

fn connect(args: &[String]) -> Result<(), String> {
    let [address, port, mode, tail @ ..] = args else {
        return Err(
            "connect expects ADDRESS PORT (close|wait-eof|close-and-signal REQUEST CLOSED)".into(),
        );
    };
    let address = SocketAddrV4::new(parse_address(address)?, parse_port(port)?);
    let mut stream = TcpStream::connect_timeout(&address.into(), Duration::from_secs(5))
        .map_err(|error| format!("connect {address}: {error}"))?;

    match (mode.as_str(), tail) {
        ("close", []) => Ok(()),
        ("wait-eof", []) => {
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .map_err(|error| format!("set read timeout: {error}"))?;
            let mut byte = [0_u8; 1];
            let read = stream
                .read(&mut byte)
                .map_err(|error| format!("wait for EOF: {error}"))?;
            if read == 0 {
                Ok(())
            } else {
                Err("expected EOF from active-closing listener".into())
            }
        }
        ("close-and-signal", [request, closed]) => {
            drop(stream);
            touch(Path::new(request))?;
            wait_for_path(Path::new(closed), MARKER_TIMEOUT)
        }
        _ => Err(format!("invalid connect mode or arguments: {mode:?}")),
    }
}

fn accept_forever(listener: TcpListener, active_close: bool) -> Result<(), String> {
    loop {
        let (mut stream, _) = listener
            .accept()
            .map_err(|error| format!("accept connection: {error}"))?;
        if active_close {
            stream
                .shutdown(Shutdown::Write)
                .map_err(|error| format!("active close: {error}"))?;
        }
        let mut byte = [0_u8; 1];
        stream
            .read(&mut byte)
            .map_err(|error| format!("read connection: {error}"))?;
    }
}

fn close_on_marker(listener: TcpListener, request: &Path, closed: &Path) -> Result<(), String> {
    remove_if_present(request)?;
    remove_if_present(closed)?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("set listener nonblocking: {error}"))?;
    loop {
        if request.exists() {
            drop(listener);
            touch(closed)?;
            loop {
                thread::park();
            }
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut byte = [0_u8; 1];
                stream
                    .read(&mut byte)
                    .map_err(|error| format!("read connection: {error}"))?;
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(format!("accept connection: {error}")),
        }
    }
}

fn remove_if_present(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("remove stale marker {}: {error}", path.display())),
    }
}

fn reusable_listener(address: Ipv4Addr, port: u16) -> Result<TcpListener, String> {
    let raw = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, libc::IPPROTO_TCP) };
    if raw < 0 {
        return Err(format!("create socket: {}", io::Error::last_os_error()));
    }
    let socket = unsafe { OwnedFd::from_raw_fd(raw) };
    let enabled: libc::c_int = 1;
    if unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_REUSEADDR,
            std::ptr::from_ref(&enabled).cast(),
            std::mem::size_of_val(&enabled) as libc::socklen_t,
        )
    } != 0
    {
        return Err(format!(
            "enable address reuse: {}",
            io::Error::last_os_error()
        ));
    }

    let mut raw_address: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    #[cfg(target_os = "macos")]
    {
        raw_address.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
    }
    raw_address.sin_family = libc::AF_INET as libc::sa_family_t;
    raw_address.sin_port = port.to_be();
    raw_address.sin_addr.s_addr = u32::from_ne_bytes(address.octets());
    if unsafe {
        libc::bind(
            socket.as_raw_fd(),
            std::ptr::from_ref(&raw_address).cast(),
            std::mem::size_of_val(&raw_address) as libc::socklen_t,
        )
    } != 0
    {
        return Err(format!(
            "bind {address}:{port}: {}",
            io::Error::last_os_error()
        ));
    }
    if unsafe { libc::listen(socket.as_raw_fd(), 16) } != 0 {
        return Err(format!("listen: {}", io::Error::last_os_error()));
    }
    Ok(TcpListener::from(socket))
}

fn parse_address(value: &str) -> Result<Ipv4Addr, String> {
    match value {
        "127.0.0.1" => Ok(Ipv4Addr::LOCALHOST),
        "0.0.0.0" => Ok(Ipv4Addr::UNSPECIFIED),
        _ => Err(format!("unsupported listen address {value:?}")),
    }
}

fn parse_port(value: &str) -> Result<u16, String> {
    value
        .parse()
        .map_err(|error| format!("invalid port {value:?}: {error}"))
}

fn touch(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create marker parent {}: {error}", parent.display()))?;
    }
    fs::write(path, []).map_err(|error| format!("create marker {}: {error}", path.display()))
}

fn wait_for_path(path: &Path, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(20));
    }
    Err(format!(
        "timed out waiting for marker {}",
        PathBuf::from(path).display()
    ))
}
