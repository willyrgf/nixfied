//! Native TCP socket operations shared by endpoint preflight and readiness.
use std::io;
use std::net::IpAddr;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::time::Duration;

pub(super) struct TcpSocket(OwnedFd);

pub(super) struct PendingConnect(TcpSocket);

pub(super) enum Connection {
    Pending(PendingConnect),
    Connected,
}

impl TcpSocket {
    pub(super) fn new(address: IpAddr) -> io::Result<Self> {
        let domain = if address.is_ipv4() {
            libc::AF_INET
        } else {
            libc::AF_INET6
        };
        #[cfg(target_os = "linux")]
        let result = Self::create(
            domain,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
        );
        #[cfg(target_os = "macos")]
        let result = crate::spawn::exclude_spawn(|| {
            let socket = Self::create(domain, libc::SOCK_STREAM)?;
            if unsafe { libc::fcntl(socket.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0
                || unsafe { libc::fcntl(socket.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) } < 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(socket)
        });
        result
    }

    fn create(domain: libc::c_int, kind: libc::c_int) -> io::Result<Self> {
        let raw = unsafe { libc::socket(domain, kind, libc::IPPROTO_TCP) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: socket returned a fresh descriptor owned here.
        Ok(Self(unsafe { OwnedFd::from_raw_fd(raw) }))
    }

    pub(super) fn bind(&self, address: IpAddr, port: u16) -> io::Result<()> {
        socket_address(address, port, |pointer, length| unsafe {
            libc::bind(self.as_raw_fd(), pointer, length)
        })
    }

    /// Exactly one connect call. EINTR leaves the same nonblocking attempt pending.
    pub(super) fn connect(self, address: IpAddr, port: u16) -> io::Result<Connection> {
        match socket_address(address, port, |pointer, length| unsafe {
            libc::connect(self.as_raw_fd(), pointer, length)
        }) {
            Ok(()) => Ok(Connection::Connected),
            Err(error) if matches!(error.raw_os_error(), Some(libc::EINPROGRESS | libc::EINTR)) => {
                Ok(Connection::Pending(PendingConnect(self)))
            }
            Err(error) => Err(error),
        }
    }
}

impl PendingConnect {
    /// Wait at most one observation quantum. Readiness of a descriptor alone is
    /// not proof of connection: SO_ERROR and peer association must both agree.
    pub(super) fn poll(self, remaining: Duration) -> io::Result<Connection> {
        let mut descriptor = libc::pollfd {
            fd: self.0.as_raw_fd(),
            events: libc::POLLOUT,
            revents: 0,
        };
        let timeout = remaining
            .min(super::OBSERVATION_INTERVAL)
            .as_millis()
            .max(1) as libc::c_int;
        let result = unsafe { libc::poll(&mut descriptor, 1, timeout) };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                return Ok(Connection::Pending(self));
            }
            return Err(error);
        }
        if result == 0 {
            return Ok(Connection::Pending(self));
        }
        if descriptor.revents & libc::POLLNVAL != 0 {
            return Err(io::Error::from_raw_os_error(libc::EBADF));
        }
        let mut error: libc::c_int = 0;
        let mut size = std::mem::size_of_val(&error) as libc::socklen_t;
        if unsafe {
            libc::getsockopt(
                self.0.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_ERROR,
                (&raw mut error).cast(),
                &mut size,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        if size as usize != std::mem::size_of_val(&error) {
            return Err(io::Error::other("unexpected socket error size"));
        }
        if error != 0 {
            return Err(io::Error::from_raw_os_error(error));
        }
        let mut peer = std::mem::MaybeUninit::<libc::sockaddr_storage>::zeroed();
        let mut size = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        if unsafe { libc::getpeername(self.0.as_raw_fd(), peer.as_mut_ptr().cast(), &mut size) }
            != 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Connection::Connected)
    }
}

impl AsRawFd for TcpSocket {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

fn socket_address(
    address: IpAddr,
    port: u16,
    call: impl FnOnce(*const libc::sockaddr, libc::socklen_t) -> libc::c_int,
) -> io::Result<()> {
    let result = match address {
        IpAddr::V4(address) => {
            // SAFETY: all-zero initialization is valid for sockaddr_in.
            let mut native: libc::sockaddr_in = unsafe { std::mem::zeroed() };
            #[cfg(target_os = "macos")]
            {
                native.sin_len = std::mem::size_of_val(&native) as u8;
            }
            native.sin_family = libc::AF_INET as libc::sa_family_t;
            native.sin_port = port.to_be();
            native.sin_addr.s_addr = u32::from_ne_bytes(address.octets());
            call(
                (&raw const native).cast(),
                std::mem::size_of_val(&native) as libc::socklen_t,
            )
        }
        IpAddr::V6(address) => {
            // Only admitted numeric IPs are used; flow and scope remain zero.
            let mut native: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
            #[cfg(target_os = "macos")]
            {
                native.sin6_len = std::mem::size_of_val(&native) as u8;
            }
            native.sin6_family = libc::AF_INET6 as libc::sa_family_t;
            native.sin6_port = port.to_be();
            native.sin6_addr.s6_addr = address.octets();
            call(
                (&raw const native).cast(),
                std::mem::size_of_val(&native) as libc::socklen_t,
            )
        }
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_socket_creation_and_spawn_do_not_inherit_sockets() {
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    for _ in 0..8 {
                        let socket = TcpSocket::new(std::net::Ipv4Addr::LOCALHOST.into()).unwrap();
                        let mut command = std::process::Command::new(
                            std::env::var("NIXFIED_TEST_CHILD").unwrap(),
                        );
                        command
                            .arg("assert-fd-closed")
                            .arg(socket.as_raw_fd().to_string());
                        assert!(
                            crate::spawn::command(&mut command)
                                .unwrap()
                                .wait()
                                .unwrap()
                                .success()
                        );
                    }
                });
            }
        });
    }
}
