use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddrV4, TcpListener, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const MARKER_TIMEOUT: Duration = Duration::from_secs(15);
const SURVIVOR_DELAY: Duration = Duration::from_secs(2);
static TERM_RECEIVED: AtomicBool = AtomicBool::new(false);

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
        "output" => output(args),
        "exit" => exit_with(args),
        "block" => block(args),
        "term-tree" => term_tree(args),
        "detached-listener" => detached_listener(args),
        "detached-sleeper" => detached_sleeper(args),
        "term-block" => term_block(args),
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
        return Err("listen expects ADDRESS PORT MODE [MODE-ARGUMENTS]".into());
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
        ("ready-on-marker", [bound, acknowledgement, ready]) => ready_on_marker(
            listener,
            Path::new(bound),
            Path::new(acknowledgement),
            Path::new(ready),
        ),
        _ => Err(format!("invalid listen mode or arguments: {mode:?}")),
    }
}

fn output(args: &[String]) -> Result<(), String> {
    match args {
        [mode, stdout, stderr] if mode == "literal" => {
            io::stdout()
                .write_all(stdout.as_bytes())
                .map_err(|error| format!("write stdout: {error}"))?;
            io::stderr()
                .write_all(stderr.as_bytes())
                .map_err(|error| format!("write stderr: {error}"))
        }
        [mode, stdout, stderr] if mode == "hex" => write_hex_output(stdout, stderr),
        [mode, stdout_byte, stdout_count, stderr_byte, stderr_count] if mode == "repeat" => {
            write_repeated_output(stdout_byte, stdout_count, stderr_byte, stderr_count)
        }
        [mode, stdout, stderr, code] if mode == "hex-exit" => {
            write_hex_output(stdout, stderr)?;
            exit_with(&[code.clone()])
        }
        [mode, stdout, stderr, marker] if mode == "hex-block" => {
            write_hex_output(stdout, stderr)?;
            touch(Path::new(marker))?;
            park_forever()
        }
        [mode, name] if mode == "env" => {
            let value = env::var_os(name)
                .ok_or_else(|| format!("environment variable {name:?} is not set"))?;
            io::stdout()
                .write_all(value.to_string_lossy().as_bytes())
                .map_err(|error| format!("write stdout: {error}"))
        }
        [mode] if mode == "environment" => {
            let mut variables = env::vars_os()
                .map(|(name, value)| {
                    format!("{}={}", name.to_string_lossy(), value.to_string_lossy())
                })
                .collect::<Vec<_>>();
            variables.sort();
            io::stdout()
                .write_all(variables.join(";").as_bytes())
                .map_err(|error| format!("write stdout: {error}"))
        }
        [mode] if mode == "stdin" => io::copy(&mut io::stdin().lock(), &mut io::stdout().lock())
            .map(|_| ())
            .map_err(|error| format!("copy stdin: {error}")),
        _ => Err("output expects literal, hex, repeat, env, environment, or stdin".into()),
    }
}

fn write_hex_output(stdout: &str, stderr: &str) -> Result<(), String> {
    let stdout = decode_hex(stdout)?;
    let stderr = decode_hex(stderr)?;
    let mut stdout_handle = io::stdout().lock();
    stdout_handle
        .write_all(&stdout)
        .map_err(|error| format!("write stdout: {error}"))?;
    stdout_handle
        .flush()
        .map_err(|error| format!("flush stdout: {error}"))?;
    let mut stderr_handle = io::stderr().lock();
    stderr_handle
        .write_all(&stderr)
        .map_err(|error| format!("write stderr: {error}"))?;
    stderr_handle
        .flush()
        .map_err(|error| format!("flush stderr: {error}"))
}

fn write_repeated_output(
    stdout_byte: &str,
    stdout_count: &str,
    stderr_byte: &str,
    stderr_count: &str,
) -> Result<(), String> {
    let stdout_byte = one_hex_byte(stdout_byte)?;
    let stderr_byte = one_hex_byte(stderr_byte)?;
    let stdout_count = stdout_count
        .parse::<usize>()
        .map_err(|error| format!("invalid stdout repeat count {stdout_count:?}: {error}"))?;
    let stderr_count = stderr_count
        .parse::<usize>()
        .map_err(|error| format!("invalid stderr repeat count {stderr_count:?}: {error}"))?;
    write_repeated(
        &mut io::stdout().lock(),
        stdout_byte,
        stdout_count,
        "stdout",
    )?;
    write_repeated(
        &mut io::stderr().lock(),
        stderr_byte,
        stderr_count,
        "stderr",
    )
}

fn write_repeated<W: Write>(
    writer: &mut W,
    byte: u8,
    count: usize,
    stream: &str,
) -> Result<(), String> {
    let chunk = [byte; 8192];
    let mut remaining = count;
    while remaining > 0 {
        let amount = remaining.min(chunk.len());
        writer
            .write_all(&chunk[..amount])
            .map_err(|error| format!("write {stream}: {error}"))?;
        remaining -= amount;
    }
    writer
        .flush()
        .map_err(|error| format!("flush {stream}: {error}"))
}

fn one_hex_byte(value: &str) -> Result<u8, String> {
    let bytes = decode_hex(value)?;
    bytes
        .first()
        .copied()
        .filter(|_| bytes.len() == 1)
        .ok_or_else(|| format!("expected one hex byte, got {value:?}"))
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    if !value.len().is_multiple_of(2) {
        return Err(format!("hex value has odd length: {value:?}"));
    }
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let high = hex_digit(pair[0])?;
        let low = hex_digit(pair[1])?;
        decoded.push((high << 4) | low);
    }
    Ok(decoded)
}

fn hex_digit(value: u8) -> Result<u8, String> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(format!("invalid hex digit: {value:?}")),
    }
}

fn exit_with(args: &[String]) -> Result<(), String> {
    let [code] = args else {
        return Err("exit expects CODE".into());
    };
    let code = code
        .parse::<u8>()
        .map_err(|error| format!("invalid exit code {code:?}: {error}"))?;
    std::process::exit(i32::from(code));
}

fn block(args: &[String]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("block expects no arguments".into());
    }
    park_forever()
}

fn term_tree(args: &[String]) -> Result<(), String> {
    let (started, survivor, pid_file) = match args {
        [started, survivor] => (started.as_str(), survivor.as_str(), None),
        [started, survivor, pid_file] => {
            (started.as_str(), survivor.as_str(), Some(pid_file.as_str()))
        }
        _ => return Err("term-tree expects STARTED SURVIVOR [PID-FILE]".into()),
    };
    let child = fork_process()?;
    if child == 0 {
        child_exit((|| {
            ignore_signal(libc::SIGTERM)?;
            if let Some(pid_file) = pid_file {
                write_pid(Path::new(pid_file))?;
            }
            touch(Path::new(started))?;
            thread::sleep(SURVIVOR_DELAY);
            touch(Path::new(survivor))?;
            park_forever()
        })());
    }
    wait_for_child(child)
}

fn detached_listener(args: &[String]) -> Result<(), String> {
    let [address, port, bound, exit_request, exiting] = args else {
        return Err("detached-listener expects ADDRESS PORT BOUND EXIT-REQUEST EXITING".into());
    };
    let address = parse_address(address)?;
    let port = parse_port(port)?;
    for marker in [bound, exit_request, exiting] {
        remove_if_present(Path::new(marker))?;
    }
    let child = fork_process()?;
    if child == 0 {
        child_exit((|| {
            create_session()?;
            let listener = reusable_listener(address, port)?;
            touch(Path::new(bound))?;
            accept_forever(listener, false)
        })());
    }
    wait_for_path(Path::new(bound), MARKER_TIMEOUT)?;
    wait_for_path(Path::new(exit_request), MARKER_TIMEOUT)?;
    touch(Path::new(exiting))
}

fn detached_sleeper(args: &[String]) -> Result<(), String> {
    let Some((mode, args)) = args.split_first() else {
        return Err("detached-sleeper expects a mode".into());
    };
    match (mode.as_str(), args) {
        ("immediate", [detached]) => {
            spawn_detached_sleeper(Path::new(detached), None)?;
            park_forever()
        }
        ("immediate", [detached, pid_file]) => {
            spawn_detached_sleeper(Path::new(detached), Some(Path::new(pid_file)))?;
            park_forever()
        }
        ("after-marker", [request, armed, detached]) => {
            remove_if_present(Path::new(request))?;
            remove_if_present(Path::new(armed))?;
            remove_if_present(Path::new(detached))?;
            touch(Path::new(armed))?;
            wait_for_path(Path::new(request), MARKER_TIMEOUT)?;
            spawn_detached_sleeper(Path::new(detached), None)?;
            park_forever()
        }
        ("parent-exit", [child_ready]) => {
            remove_if_present(Path::new(child_ready))?;
            let child = fork_process()?;
            if child == 0 {
                child_exit((|| {
                    touch(Path::new(child_ready))?;
                    park_forever()
                })());
            }
            wait_for_path(Path::new(child_ready), MARKER_TIMEOUT)
        }
        _ => Err(format!(
            "invalid detached-sleeper mode or arguments: {mode:?}"
        )),
    }
}

fn term_block(args: &[String]) -> Result<(), String> {
    let [address, port, started, stopping] = args else {
        return Err("term-block expects ADDRESS PORT STARTED STOPPING".into());
    };
    remove_if_present(Path::new(started))?;
    remove_if_present(Path::new(stopping))?;
    TERM_RECEIVED.store(false, Ordering::SeqCst);
    install_term_handler()?;
    let listener = reusable_listener(parse_address(address)?, parse_port(port)?)?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("set listener nonblocking: {error}"))?;
    touch(Path::new(started))?;
    while !TERM_RECEIVED.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((_stream, _)) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(format!("accept connection: {error}")),
        }
    }
    touch(Path::new(stopping))?;
    park_forever()
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

fn ready_on_marker(
    listener: TcpListener,
    bound: &Path,
    acknowledgement: &Path,
    ready: &Path,
) -> Result<(), String> {
    remove_if_present(bound)?;
    remove_if_present(acknowledgement)?;
    remove_if_present(ready)?;
    touch(bound)?;
    wait_for_path(acknowledgement, MARKER_TIMEOUT)?;
    touch(ready)?;
    accept_forever(listener, false)
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

fn spawn_detached_sleeper(detached: &Path, pid_file: Option<&Path>) -> Result<(), String> {
    remove_if_present(detached)?;
    if let Some(pid_file) = pid_file {
        remove_if_present(pid_file)?;
    }
    let child = fork_process()?;
    if child == 0 {
        child_exit((|| {
            create_session()?;
            if let Some(pid_file) = pid_file {
                write_pid(pid_file)?;
            }
            touch(detached)?;
            park_forever()
        })());
    }
    Ok(())
}

fn fork_process() -> Result<libc::pid_t, String> {
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        Err(format!("fork: {}", io::Error::last_os_error()))
    } else {
        Ok(pid)
    }
}

fn create_session() -> Result<(), String> {
    if unsafe { libc::setsid() } < 0 {
        Err(format!("setsid: {}", io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

fn ignore_signal(signal: libc::c_int) -> Result<(), String> {
    if unsafe { libc::signal(signal, libc::SIG_IGN) } == libc::SIG_ERR {
        Err(format!(
            "ignore signal {signal}: {}",
            io::Error::last_os_error()
        ))
    } else {
        Ok(())
    }
}

extern "C" fn record_term(_: libc::c_int) {
    TERM_RECEIVED.store(true, Ordering::SeqCst);
}

fn install_term_handler() -> Result<(), String> {
    if unsafe {
        libc::signal(
            libc::SIGTERM,
            record_term as *const () as libc::sighandler_t,
        )
    } == libc::SIG_ERR
    {
        Err(format!(
            "install TERM handler: {}",
            io::Error::last_os_error()
        ))
    } else {
        Ok(())
    }
}

fn wait_for_child(pid: libc::pid_t) -> Result<(), String> {
    loop {
        let mut status = 0;
        let result = unsafe { libc::waitpid(pid, &mut status, 0) };
        if result == pid {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(format!("wait for child {pid}: {error}"));
        }
    }
}

fn child_exit(result: Result<(), String>) -> ! {
    let code = match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("nixfied-test-child: {error}");
            70
        }
    };
    unsafe { libc::_exit(code) }
}

fn write_pid(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create pid-file parent {}: {error}", parent.display()))?;
    }
    fs::write(path, std::process::id().to_string())
        .map_err(|error| format!("write pid file {}: {error}", path.display()))
}

fn park_forever() -> ! {
    loop {
        thread::park();
    }
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
