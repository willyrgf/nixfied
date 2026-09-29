use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicU32, Ordering};

use super::{KernelSocketIdentity, SocketRecord, SocketScan, read_array};

const NETLINK_SOCK_DIAG: libc::c_int = 4;
const SOCK_DIAG_BY_FAMILY: u16 = 20;
const NLM_F_REQUEST: u16 = 0x01;
const NLM_F_DUMP_INTR: u16 = 0x10;
const NLM_F_ROOT: u16 = 0x100;
const NLM_F_MATCH: u16 = 0x200;
const NLMSG_NOOP: u16 = 1;
const NLMSG_ERROR: u16 = 2;
const NLMSG_DONE: u16 = 3;
const NLMSG_OVERRUN: u16 = 4;
const TCP_LISTEN: u8 = 10;
const NLMSG_HEADER_LEN: usize = 16;
const INET_DIAG_MSG_LEN: usize = 72;
const RECEIVE_BUFFER_LEN: usize = 1024 * 1024;

static NEXT_SEQUENCE: AtomicU32 = AtomicU32::new(1);

#[repr(C)]
#[derive(Clone, Copy)]
struct InetDiagSockId {
    sport: u16,
    dport: u16,
    src: [u32; 4],
    dst: [u32; 4],
    interface: u32,
    cookie: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct InetDiagRequest {
    family: u8,
    protocol: u8,
    extensions: u8,
    pad: u8,
    states: u32,
    id: InetDiagSockId,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NetlinkHeader {
    length: u32,
    message_type: u16,
    flags: u16,
    sequence: u32,
    pid: u32,
}

/// Only managed PID descriptors are inspected. Kernel TCP records are used as
/// positive correlations; no unrelated process FD inventory is consulted.
pub(super) fn inspect(
    pid: u32,
    endpoints: &BTreeMap<String, &super::SelectedEndpoint>,
) -> SocketScan {
    let mut scan = SocketScan::default();
    let mut held = BTreeMap::new();
    let descriptors = match std::fs::read_dir(format!("/proc/{pid}/fd")) {
        Ok(entries) => entries,
        Err(error) => {
            if error.kind() != io::ErrorKind::NotFound {
                scan.uncertain(format!("cannot list managed socket FDs: {error}"));
            }
            return scan;
        }
    };
    for (index, descriptor) in descriptors.enumerate() {
        if index >= super::MAX_DESCRIPTORS {
            scan.uncertain("managed FD list exceeds bound");
            break;
        }
        let descriptor = match descriptor {
            Ok(fd) => fd,
            Err(error) => {
                scan.uncertain(error.to_string());
                continue;
            }
        };
        match std::fs::read_link(descriptor.path()) {
            Ok(target) => {
                let text = target.to_string_lossy();
                if let Some(raw) = text.strip_prefix("socket:[") {
                    match raw
                        .strip_suffix(']')
                        .and_then(|value| value.parse::<u32>().ok())
                        .filter(|inode| *inode != 0)
                    {
                        Some(inode) => {
                            held.insert(descriptor.path(), inode);
                        }
                        None => scan.uncertain("malformed managed socket inode"),
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => scan.uncertain(format!("cannot inspect managed socket FD: {error}")),
        }
    }
    if held.is_empty() {
        return scan;
    }
    let inodes = held.values().copied().collect::<BTreeSet<_>>();
    for family in [libc::AF_INET, libc::AF_INET6] {
        if !endpoints
            .values()
            .any(|e| e.host.ip().is_ipv4() == (family == libc::AF_INET))
        {
            continue;
        }
        if let Err(error) = dump_family(family as u8, &inodes, &mut scan.records) {
            scan.uncertain(error);
        }
    }
    // An inode in a previous FD table is insufficient: retain only sockets
    // still held by the same candidate descriptor after kernel observation.
    let mut still_held = BTreeSet::new();
    for (path, inode) in held {
        match std::fs::read_link(path) {
            Ok(target) if target.to_string_lossy() == format!("socket:[{inode}]") => {
                still_held.insert(inode);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => scan.uncertain(format!("cannot recheck managed socket FD: {error}")),
        }
    }
    scan.records.retain(|record| {
        let KernelSocketIdentity::Linux { inode, .. } = record.identity;
        still_held.contains(&inode)
            && endpoints
                .values()
                .any(|e| e.host.ip() == record.address && e.port == record.port)
    });
    scan
}

fn dump_family(
    family: u8,
    inodes: &BTreeSet<u32>,
    records: &mut Vec<SocketRecord>,
) -> Result<(), String> {
    let raw = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_RAW | libc::SOCK_CLOEXEC,
            NETLINK_SOCK_DIAG,
        )
    };
    if raw < 0 {
        return Err(format!(
            "failed to create NETLINK_SOCK_DIAG socket: {}",
            io::Error::last_os_error()
        ));
    }
    // SAFETY: socket returned a new descriptor owned by this scope.
    let socket = unsafe { OwnedFd::from_raw_fd(raw) };

    // SAFETY: zero is the required value for the padding and group fields.
    let mut local = unsafe { std::mem::zeroed::<libc::sockaddr_nl>() };
    local.nl_family = libc::AF_NETLINK as libc::sa_family_t;
    if unsafe {
        libc::bind(
            socket.as_raw_fd(),
            (&local as *const libc::sockaddr_nl).cast(),
            std::mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
        )
    } != 0
    {
        return Err(format!(
            "failed to bind NETLINK_SOCK_DIAG socket: {}",
            io::Error::last_os_error()
        ));
    }

    let timeout = libc::timeval {
        tv_sec: 1,
        tv_usec: 0,
    };
    if unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            (&raw const timeout).cast(),
            std::mem::size_of_val(&timeout) as libc::socklen_t,
        )
    } != 0
    {
        return Err("cannot bound socket observation".to_string());
    }
    let sequence = NEXT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let request = InetDiagRequest {
        family,
        protocol: libc::IPPROTO_TCP as u8,
        extensions: 0,
        pad: 0,
        states: 1_u32 << TCP_LISTEN,
        id: InetDiagSockId {
            sport: 0,
            dport: 0,
            src: [0; 4],
            dst: [0; 4],
            interface: 0,
            cookie: [u32::MAX; 2],
        },
    };
    let header = NetlinkHeader {
        length: (std::mem::size_of::<NetlinkHeader>() + std::mem::size_of::<InetDiagRequest>())
            as u32,
        message_type: SOCK_DIAG_BY_FAMILY,
        flags: NLM_F_REQUEST | NLM_F_ROOT | NLM_F_MATCH,
        sequence,
        pid: 0,
    };
    let mut request_bytes = Vec::with_capacity(header.length as usize);
    append_struct_bytes(&mut request_bytes, &header);
    append_struct_bytes(&mut request_bytes, &request);

    // SAFETY: zero is the kernel destination for a netlink request.
    let mut kernel = unsafe { std::mem::zeroed::<libc::sockaddr_nl>() };
    kernel.nl_family = libc::AF_NETLINK as libc::sa_family_t;
    let sent = unsafe {
        libc::sendto(
            socket.as_raw_fd(),
            request_bytes.as_ptr().cast(),
            request_bytes.len(),
            0,
            (&kernel as *const libc::sockaddr_nl).cast(),
            std::mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
        )
    };
    if sent < 0 || sent as usize != request_bytes.len() {
        return Err(format!(
            "failed to request NETLINK_SOCK_DIAG dump: {}",
            io::Error::last_os_error()
        ));
    }

    let mut buffer = vec![0_u8; RECEIVE_BUFFER_LEN];
    for _ in 0..64 {
        // SAFETY: zero initializes the receive address and message header.
        let mut sender = unsafe { std::mem::zeroed::<libc::sockaddr_nl>() };
        let mut iov = libc::iovec {
            iov_base: buffer.as_mut_ptr().cast(),
            iov_len: buffer.len(),
        };
        // SAFETY: zero initializes unused control fields.
        let mut message = unsafe { std::mem::zeroed::<libc::msghdr>() };
        message.msg_name = (&mut sender as *mut libc::sockaddr_nl).cast();
        message.msg_namelen = std::mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t;
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        let received = unsafe { libc::recvmsg(socket.as_raw_fd(), &mut message, libc::MSG_TRUNC) };
        if received < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(format!("failed to receive NETLINK_SOCK_DIAG dump: {error}"));
        }
        if received == 0 {
            return Err("NETLINK_SOCK_DIAG dump ended without NLMSG_DONE".to_string());
        }
        if received as usize > buffer.len() || message.msg_flags & libc::MSG_TRUNC != 0 {
            return Err("NETLINK_SOCK_DIAG datagram was truncated".to_string());
        }
        if sender.nl_pid != 0 {
            return Err(format!(
                "NETLINK_SOCK_DIAG response came from non-kernel port {}",
                sender.nl_pid
            ));
        }
        if parse_datagram(
            &buffer[..received as usize],
            sequence,
            family,
            inodes,
            records,
        )? {
            return Ok(());
        }
    }
    Err("socket observation exceeded datagram bound".to_string())
}

fn append_struct_bytes<T>(target: &mut Vec<u8>, value: &T) {
    // SAFETY: the request structs are repr(C), fully initialized, and are copied
    // immediately as an opaque native-endian netlink wire representation.
    let bytes = unsafe {
        std::slice::from_raw_parts((value as *const T).cast::<u8>(), std::mem::size_of::<T>())
    };
    target.extend_from_slice(bytes);
}

fn parse_datagram(
    bytes: &[u8],
    sequence: u32,
    family: u8,
    inodes: &BTreeSet<u32>,
    records: &mut Vec<SocketRecord>,
) -> Result<bool, String> {
    let mut done = false;
    let mut offset = 0_usize;
    while offset < bytes.len() {
        let remaining = bytes.len() - offset;
        if remaining < NLMSG_HEADER_LEN {
            return Err("truncated netlink message header".to_string());
        }
        let length = read_u32_ne(bytes, offset)? as usize;
        let message_type = read_u16_ne(bytes, offset + 4)?;
        let flags = read_u16_ne(bytes, offset + 6)?;
        let observed_sequence = read_u32_ne(bytes, offset + 8)?;
        if length < NLMSG_HEADER_LEN || length > remaining {
            return Err(format!("invalid netlink message length {length}"));
        }
        if observed_sequence != sequence {
            return Err(format!(
                "NETLINK_SOCK_DIAG response sequence {observed_sequence} did not match {sequence}"
            ));
        }
        if flags & NLM_F_DUMP_INTR != 0 {
            return Err("NETLINK_SOCK_DIAG dump was interrupted".to_string());
        }
        let payload = &bytes[offset + NLMSG_HEADER_LEN..offset + length];
        match message_type {
            NLMSG_NOOP => {}
            NLMSG_ERROR => {
                if payload.len() < 4 {
                    return Err("truncated NETLINK_SOCK_DIAG error".to_string());
                }
                let code = read_i32_ne(payload, 0)?;
                if code != 0 {
                    return Err(format!(
                        "NETLINK_SOCK_DIAG returned {}",
                        io::Error::from_raw_os_error(code.saturating_neg())
                    ));
                }
            }
            NLMSG_DONE => {
                if payload.len() >= 4 {
                    let code = read_i32_ne(payload, 0)?;
                    if code != 0 {
                        return Err(format!(
                            "NETLINK_SOCK_DIAG dump completed with {}",
                            io::Error::from_raw_os_error(code.saturating_neg())
                        ));
                    }
                }
                done = true;
            }
            NLMSG_OVERRUN => {
                return Err("NETLINK_SOCK_DIAG reported receive overrun".to_string());
            }
            SOCK_DIAG_BY_FAMILY => {
                if payload.len() < INET_DIAG_MSG_LEN {
                    return Err("truncated inet_diag_msg".to_string());
                }
                if inodes.contains(&read_u32_ne(payload, 68)?) {
                    records.push(parse_listener(payload, family)?);
                }
            }
            other => {
                return Err(format!("unexpected NETLINK_SOCK_DIAG message type {other}"));
            }
        }
        let aligned = align4(length).ok_or_else(|| "netlink length overflow".to_string())?;
        if aligned > remaining {
            if length != remaining {
                return Err("truncated netlink message padding".to_string());
            }
            offset = bytes.len();
        } else {
            offset += aligned;
        }
    }
    Ok(done)
}

fn parse_listener(payload: &[u8], requested_family: u8) -> Result<SocketRecord, String> {
    if payload.len() < INET_DIAG_MSG_LEN {
        return Err("truncated inet_diag_msg".to_string());
    }
    let family = payload[0];
    if family != requested_family {
        return Err(format!(
            "inet_diag_msg family {family} did not match requested family {requested_family}"
        ));
    }
    if payload[1] != TCP_LISTEN {
        return Err(format!(
            "NETLINK_SOCK_DIAG returned non-listener TCP state {}",
            payload[1]
        ));
    }
    let port = read_u16_be(payload, 4)?;
    let address = match family as libc::c_int {
        libc::AF_INET => IpAddr::V4(Ipv4Addr::new(
            payload[8],
            payload[9],
            payload[10],
            payload[11],
        )),
        libc::AF_INET6 => {
            let octets = read_array(payload, 8)
                .ok_or_else(|| "truncated IPv6 inet_diag address".to_string())?;
            IpAddr::V6(Ipv6Addr::from(octets))
        }
        other => return Err(format!("unsupported inet_diag_msg family {other}")),
    };
    let cookie = [read_u32_ne(payload, 44)?, read_u32_ne(payload, 48)?];
    let inode = read_u32_ne(payload, 68)?;
    if inode == 0 || cookie == [u32::MAX; 2] {
        return Err("listener has no usable kernel socket identity".to_string());
    }
    Ok(SocketRecord {
        address,
        port,
        identity: KernelSocketIdentity::Linux { inode, cookie },
    })
}

fn read_u16_ne(bytes: &[u8], offset: usize) -> Result<u16, String> {
    Ok(u16::from_ne_bytes(
        read_array(bytes, offset).ok_or_else(|| "truncated u16 field".to_string())?,
    ))
}

fn read_u32_ne(bytes: &[u8], offset: usize) -> Result<u32, String> {
    Ok(u32::from_ne_bytes(
        read_array(bytes, offset).ok_or_else(|| "truncated u32 field".to_string())?,
    ))
}

fn read_i32_ne(bytes: &[u8], offset: usize) -> Result<i32, String> {
    Ok(i32::from_ne_bytes(
        read_array(bytes, offset).ok_or_else(|| "truncated i32 field".to_string())?,
    ))
}

fn read_u16_be(bytes: &[u8], offset: usize) -> Result<u16, String> {
    Ok(u16::from_be_bytes(read_array(bytes, offset).ok_or_else(
        || "truncated big-endian u16 field".to_string(),
    )?))
}

fn align4(value: usize) -> Option<usize> {
    value.checked_add(3).map(|value| value & !3)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(family: u8) -> Vec<u8> {
        let mut bytes = vec![0; INET_DIAG_MSG_LEN];
        bytes[0] = family;
        bytes[1] = TCP_LISTEN;
        bytes[4..6].copy_from_slice(&23080_u16.to_be_bytes());
        if family == libc::AF_INET as u8 {
            bytes[8..12].copy_from_slice(&[127, 0, 0, 1]);
        } else {
            bytes[23] = 1;
        }
        bytes[44..48].copy_from_slice(&7_u32.to_ne_bytes());
        bytes[48..52].copy_from_slice(&8_u32.to_ne_bytes());
        bytes[68..72].copy_from_slice(&42_u32.to_ne_bytes());
        bytes
    }

    #[test]
    fn positive_decode_needs_exact_family_listen_state_and_usable_identity() {
        for family in [libc::AF_INET as u8, libc::AF_INET6 as u8] {
            let good = payload(family);
            let record = parse_listener(&good, family).unwrap();
            assert_eq!(record.port, 23080);
            assert_eq!(
                record.identity,
                KernelSocketIdentity::Linux {
                    inode: 42,
                    cookie: [7, 8]
                }
            );
            for mutation in 0..5 {
                let mut bytes = good.clone();
                match mutation {
                    0 => {
                        bytes.pop();
                    }
                    1 => bytes[1] = 6,
                    2 => bytes[0] = 0,
                    3 => bytes[68..72].fill(0),
                    _ => bytes[44..52].fill(255),
                }
                assert!(parse_listener(&bytes, family).is_err());
            }
        }
    }

    #[test]
    fn partial_stream_preserves_a_positive_sighting_without_certifying_absence() {
        let payload = payload(libc::AF_INET as u8);
        let header = NetlinkHeader {
            length: 88,
            message_type: SOCK_DIAG_BY_FAMILY,
            flags: 0,
            sequence: 17,
            pid: 0,
        };
        let mut bytes = Vec::new();
        append_struct_bytes(&mut bytes, &header);
        bytes.extend_from_slice(&payload);
        bytes.push(0); // a malformed later record cannot erase the observation
        let mut records = Vec::new();
        assert!(
            parse_datagram(
                &bytes,
                17,
                libc::AF_INET as u8,
                &BTreeSet::from([42]),
                &mut records
            )
            .is_err()
        );
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].address, IpAddr::V4(Ipv4Addr::LOCALHOST));
    }
}
