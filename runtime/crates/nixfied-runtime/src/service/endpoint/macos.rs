use std::collections::BTreeMap;
use std::io;

use super::{KernelSocketIdentity, MAX_DESCRIPTORS, SelectedEndpoint, SocketRecord, SocketScan};

#[path = "macos_socket.rs"]
mod socket;

fn churn(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(libc::ESRCH | libc::ENOENT | libc::EBADF)
    )
}

fn socket_fds(pid: u32) -> io::Result<Vec<i32>> {
    read_socket_fds(|fds| {
        // Clear errno because an empty successful list may return zero.
        unsafe {
            *libc::__error() = 0;
        }
        let bytes = unsafe {
            libc::proc_pidinfo(
                pid as i32,
                libc::PROC_PIDLISTFDS,
                0,
                fds.as_mut_ptr().cast(),
                std::mem::size_of_val(fds) as i32,
            )
        };
        if bytes < 0 || (bytes == 0 && io::Error::last_os_error().raw_os_error() != Some(0)) {
            Err(io::Error::last_os_error())
        } else {
            Ok(bytes as usize)
        }
    })
}

fn read_socket_fds(
    mut read: impl FnMut(&mut [libc::proc_fdinfo]) -> io::Result<usize>,
) -> io::Result<Vec<i32>> {
    let mut capacity = 64;
    while capacity <= MAX_DESCRIPTORS {
        let mut fds = vec![
            libc::proc_fdinfo {
                proc_fd: 0,
                proc_fdtype: 0
            };
            capacity
        ];
        let size = std::mem::size_of_val(fds.as_slice());
        let bytes = read(&mut fds)?;
        if !bytes.is_multiple_of(std::mem::size_of::<libc::proc_fdinfo>()) {
            return Err(io::Error::other("malformed managed FD list"));
        }
        if bytes >= size {
            capacity *= 2;
            continue;
        }
        fds.truncate(bytes / std::mem::size_of::<libc::proc_fdinfo>());
        if fds.iter().any(|fd| fd.proc_fd < 0) {
            return Err(io::Error::other("invalid managed descriptor"));
        }
        return Ok(fds
            .into_iter()
            .filter(|fd| fd.proc_fdtype == libc::PROX_FDTYPE_SOCKET as u32)
            .map(|fd| fd.proc_fd)
            .collect());
    }
    Err(io::Error::other("managed FD list kept growing"))
}

pub(super) fn inspect(pid: u32, endpoints: &BTreeMap<String, &SelectedEndpoint>) -> SocketScan {
    let mut scan = SocketScan::default();
    let fds = match socket_fds(pid) {
        Ok(fds) => fds,
        Err(error) => {
            if !churn(&error) {
                scan.uncertain(format!("cannot list managed socket FDs: {error}"));
            }
            return scan;
        }
    };
    for fd in fds {
        let record = match socket::inspect(pid as i32, fd) {
            Ok(Some(record)) => record,
            Ok(None) => continue,
            Err(error) => {
                if !churn(&error) {
                    scan.uncertain(format!("cannot inspect managed socket FD: {error}"));
                }
                continue;
            }
        };
        if endpoints
            .values()
            .any(|endpoint| endpoint.host.ip() == record.address && endpoint.port == record.port)
        {
            scan.records.push(SocketRecord {
                address: record.address,
                port: record.port,
                identity: KernelSocketIdentity::Macos {
                    socket_handle: record.handle,
                    inpcb_generation: record.generation,
                },
            });
        }
    }
    scan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fd_list_growth_is_bounded_and_malformed_or_denied_reads_are_not_empty() {
        let mut reads = 0;
        let sockets = read_socket_fds(|fds| {
            reads += 1;
            if reads == 1 {
                return Ok(std::mem::size_of_val(fds));
            }
            fds[0] = libc::proc_fdinfo {
                proc_fd: 7,
                proc_fdtype: libc::PROX_FDTYPE_SOCKET as u32,
            };
            Ok(std::mem::size_of::<libc::proc_fdinfo>())
        })
        .unwrap();
        assert_eq!(sockets, [7]);
        assert_eq!(reads, 2);
        assert!(read_socket_fds(|_| Err(io::Error::from_raw_os_error(libc::EPERM))).is_err());
        assert!(read_socket_fds(|_| Ok(1)).is_err());
        assert!(
            read_socket_fds(|fds| {
                fds[0].proc_fd = -1;
                Ok(std::mem::size_of::<libc::proc_fdinfo>())
            })
            .is_err()
        );
        reads = 0;
        assert!(
            read_socket_fds(|fds| {
                reads += 1;
                Ok(std::mem::size_of_val(fds))
            })
            .is_err()
        );
        assert_eq!(reads, 11); // capacities 64 through 65536, inclusive
        assert!(read_socket_fds(|_| Ok(0)).unwrap().is_empty());
    }
}
