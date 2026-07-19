use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::MetadataExt;
use std::sync::atomic::{AtomicU32, Ordering};

use super::{
    EndpointFamily, KernelSocketIdentity, ListenerHolder, ListenerIdentity, ListenerRecord,
};
use crate::service::process::{platform_start_identity, process_group};

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
const INET_DIAG_SKV6ONLY: u16 = 11;
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

#[derive(Debug, Default)]
struct ParsedDatagram {
    records: Vec<ListenerRecord>,
    done: bool,
}

pub(super) fn snapshot() -> Result<Vec<ListenerRecord>, String> {
    let mut records = dump_family(libc::AF_INET as u8)?;
    records.extend(dump_family(libc::AF_INET6 as u8)?);
    records.sort_by(|left, right| left.identity.cmp(&right.identity));
    Ok(records)
}

fn dump_family(family: u8) -> Result<Vec<ListenerRecord>, String> {
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

    let mut records = Vec::new();
    let mut buffer = vec![0_u8; RECEIVE_BUFFER_LEN];
    loop {
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
        let parsed = parse_datagram(&buffer[..received as usize], sequence, family)?;
        records.extend(parsed.records);
        if parsed.done {
            return Ok(records);
        }
    }
}

fn append_struct_bytes<T>(target: &mut Vec<u8>, value: &T) {
    // SAFETY: the request structs are repr(C), fully initialized, and are copied
    // immediately as an opaque native-endian netlink wire representation.
    let bytes = unsafe {
        std::slice::from_raw_parts((value as *const T).cast::<u8>(), std::mem::size_of::<T>())
    };
    target.extend_from_slice(bytes);
}

fn parse_datagram(bytes: &[u8], sequence: u32, family: u8) -> Result<ParsedDatagram, String> {
    let mut parsed = ParsedDatagram::default();
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
                parsed.done = true;
            }
            NLMSG_OVERRUN => {
                return Err("NETLINK_SOCK_DIAG reported receive overrun".to_string());
            }
            SOCK_DIAG_BY_FAMILY => parsed.records.push(parse_listener(payload, family)?),
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
    Ok(parsed)
}

fn parse_listener(payload: &[u8], requested_family: u8) -> Result<ListenerRecord, String> {
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
    let (family, address) = match family as libc::c_int {
        libc::AF_INET => (
            EndpointFamily::Ipv4,
            IpAddr::V4(Ipv4Addr::new(
                payload[8],
                payload[9],
                payload[10],
                payload[11],
            )),
        ),
        libc::AF_INET6 => {
            let octets: [u8; 16] = payload
                .get(8..24)
                .ok_or_else(|| "truncated IPv6 inet_diag address".to_string())?
                .try_into()
                .map_err(|_| "invalid IPv6 inet_diag address".to_string())?;
            (EndpointFamily::Ipv6, IpAddr::V6(Ipv6Addr::from(octets)))
        }
        other => return Err(format!("unsupported inet_diag_msg family {other}")),
    };
    let cookie = [read_u32_ne(payload, 44)?, read_u32_ne(payload, 48)?];
    let uid = read_u32_ne(payload, 64)?;
    let inode = read_u32_ne(payload, 68)?;
    let mut ipv6_only = None;
    let mut offset = INET_DIAG_MSG_LEN;
    while offset < payload.len() {
        if payload.len() - offset < 4 {
            return Err("truncated inet_diag attribute header".to_string());
        }
        let length = read_u16_ne(payload, offset)? as usize;
        let kind = read_u16_ne(payload, offset + 2)?;
        if length < 4 || length > payload.len() - offset {
            return Err(format!("invalid inet_diag attribute length {length}"));
        }
        if kind == INET_DIAG_SKV6ONLY {
            if length != 5 {
                return Err(format!(
                    "INET_DIAG_SKV6ONLY has invalid payload length {}",
                    length - 4
                ));
            }
            ipv6_only = Some(match payload[offset + 4] {
                0 => false,
                1 => true,
                other => {
                    return Err(format!("INET_DIAG_SKV6ONLY has invalid value {other}"));
                }
            });
        }
        let aligned = align4(length).ok_or_else(|| "attribute length overflow".to_string())?;
        if aligned > payload.len() - offset {
            if length != payload.len() - offset {
                return Err("truncated inet_diag attribute padding".to_string());
            }
            offset = payload.len();
        } else {
            offset += aligned;
        }
    }
    if matches!(family, EndpointFamily::Ipv6) && ipv6_only.is_none() {
        return Err("IPv6 listener omitted required INET_DIAG_SKV6ONLY".to_string());
    }
    Ok(ListenerRecord {
        identity: ListenerIdentity {
            family,
            address,
            port,
            kernel: KernelSocketIdentity::Linux { inode, cookie },
            uid,
            ipv6_only,
        },
        holders: Vec::new(),
        pid_hints: Vec::new(),
    })
}

pub(super) fn correlate(records: &mut [ListenerRecord]) -> Result<(), String> {
    let euid = unsafe { libc::geteuid() } as u32;
    let mut indexes_by_inode = BTreeMap::<u32, Vec<usize>>::new();
    for (index, record) in records.iter().enumerate() {
        // A different socket UID is already complete proof of an outside holder;
        // do not let an intentionally inaccessible foreign fd table weaken that
        // evidence into PORT_UNVERIFIABLE.
        if record.identity.uid != euid {
            continue;
        }
        let KernelSocketIdentity::Linux { inode, .. } = record.identity.kernel;
        indexes_by_inode.entry(inode).or_default().push(index);
    }
    if indexes_by_inode.is_empty() {
        return Ok(());
    }
    let target_inodes = indexes_by_inode.keys().copied().collect::<BTreeSet<_>>();
    let entries = std::fs::read_dir("/proc")
        .map_err(|error| format!("failed to inspect /proc for listener holders: {error}"))?;
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        if pid == 0 {
            continue;
        }
        let process_path = entry.path();
        let metadata = match std::fs::metadata(&process_path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.uid() != euid {
            continue;
        }
        let descriptors = match std::fs::read_dir(process_path.join("fd")) {
            Ok(descriptors) => descriptors,
            Err(_) => continue,
        };
        let mut held = BTreeSet::new();
        for descriptor in descriptors {
            let descriptor = match descriptor {
                Ok(descriptor) => descriptor,
                Err(_) => continue,
            };
            let target = match std::fs::read_link(descriptor.path()) {
                Ok(target) => target,
                Err(_) => continue,
            };
            let target = target.to_string_lossy();
            let Some(inode) = target
                .strip_prefix("socket:[")
                .and_then(|value| value.strip_suffix(']'))
                .and_then(|value| value.parse::<u32>().ok())
            else {
                continue;
            };
            if target_inodes.contains(&inode) {
                held.insert(inode);
            }
        }
        if held.is_empty() {
            continue;
        }
        let Some(pgid) = process_group(pid).map_err(|error| error.message)? else {
            continue;
        };
        let holder = ListenerHolder {
            pid,
            pgid,
            platform_start: platform_start_identity(pid),
        };
        for inode in held {
            for index in indexes_by_inode.get(&inode).into_iter().flatten() {
                records[*index].holders.push(holder.clone());
            }
        }
    }
    for record in records {
        record.holders.sort_by_key(|holder| holder.pid);
        record.holders.dedup_by_key(|holder| holder.pid);
    }
    Ok(())
}

fn read_u16_ne(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| "truncated u16 field".to_string())?;
    Ok(u16::from_ne_bytes(
        value
            .try_into()
            .map_err(|_| "invalid u16 field".to_string())?,
    ))
}

fn read_u32_ne(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| "truncated u32 field".to_string())?;
    Ok(u32::from_ne_bytes(
        value
            .try_into()
            .map_err(|_| "invalid u32 field".to_string())?,
    ))
}

fn read_i32_ne(bytes: &[u8], offset: usize) -> Result<i32, String> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| "truncated i32 field".to_string())?;
    Ok(i32::from_ne_bytes(
        value
            .try_into()
            .map_err(|_| "invalid i32 field".to_string())?,
    ))
}

fn read_u16_be(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| "truncated big-endian u16 field".to_string())?;
    Ok(u16::from_be_bytes(
        value
            .try_into()
            .map_err(|_| "invalid big-endian u16 field".to_string())?,
    ))
}

fn align4(value: usize) -> Option<usize> {
    value.checked_add(3).map(|value| value & !3)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(message_type: u16, flags: u16, sequence: u32, payload: &[u8]) -> Vec<u8> {
        let length = NLMSG_HEADER_LEN + payload.len();
        let mut bytes = Vec::with_capacity(align4(length).unwrap());
        bytes.extend_from_slice(&(length as u32).to_ne_bytes());
        bytes.extend_from_slice(&message_type.to_ne_bytes());
        bytes.extend_from_slice(&flags.to_ne_bytes());
        bytes.extend_from_slice(&sequence.to_ne_bytes());
        bytes.extend_from_slice(&0_u32.to_ne_bytes());
        bytes.extend_from_slice(payload);
        bytes.resize(align4(length).unwrap(), 0);
        bytes
    }

    fn diag_payload(family: u8, address: IpAddr, port: u16, v6only: Option<bool>) -> Vec<u8> {
        let mut bytes = vec![0_u8; INET_DIAG_MSG_LEN];
        bytes[0] = family;
        bytes[1] = TCP_LISTEN;
        bytes[4..6].copy_from_slice(&port.to_be_bytes());
        match address {
            IpAddr::V4(address) => bytes[8..12].copy_from_slice(&address.octets()),
            IpAddr::V6(address) => bytes[8..24].copy_from_slice(&address.octets()),
        }
        bytes[44..48].copy_from_slice(&7_u32.to_ne_bytes());
        bytes[48..52].copy_from_slice(&8_u32.to_ne_bytes());
        bytes[64..68].copy_from_slice(&1000_u32.to_ne_bytes());
        bytes[68..72].copy_from_slice(&42_u32.to_ne_bytes());
        if let Some(v6only) = v6only {
            bytes.extend_from_slice(&5_u16.to_ne_bytes());
            bytes.extend_from_slice(&INET_DIAG_SKV6ONLY.to_ne_bytes());
            bytes.push(u8::from(v6only));
            bytes.extend_from_slice(&[0; 3]);
        }
        bytes
    }

    #[test]
    fn parser_rejects_interrupted_dump() {
        let bytes = message(NLMSG_DONE, NLM_F_DUMP_INTR, 17, &[]);
        assert!(
            parse_datagram(&bytes, 17, libc::AF_INET as u8)
                .unwrap_err()
                .contains("interrupted")
        );
    }

    #[test]
    fn parser_requires_ipv6_only_evidence() {
        let payload = diag_payload(
            libc::AF_INET6 as u8,
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            23080,
            None,
        );
        let bytes = message(SOCK_DIAG_BY_FAMILY, 0, 19, &payload);
        assert!(
            parse_datagram(&bytes, 19, libc::AF_INET6 as u8)
                .unwrap_err()
                .contains("SKV6ONLY")
        );
    }

    #[test]
    fn parser_retains_listener_identity_and_ipv6_mode() {
        let payload = diag_payload(
            libc::AF_INET6 as u8,
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            23080,
            Some(false),
        );
        let bytes = message(SOCK_DIAG_BY_FAMILY, 0, 23, &payload);
        let record = parse_datagram(&bytes, 23, libc::AF_INET6 as u8)
            .unwrap()
            .records
            .pop()
            .unwrap();
        assert_eq!(record.identity.address, IpAddr::V6(Ipv6Addr::LOCALHOST));
        assert_eq!(record.identity.port, 23080);
        assert_eq!(record.identity.uid, 1000);
        assert_eq!(record.identity.ipv6_only, Some(false));
        assert_eq!(
            record.identity.kernel,
            KernelSocketIdentity::Linux {
                inode: 42,
                cookie: [7, 8]
            }
        );
    }

    #[test]
    fn parser_rejects_time_wait_as_listener_evidence() {
        let mut payload = diag_payload(
            libc::AF_INET as u8,
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            23080,
            None,
        );
        payload[1] = 6; // TCP_TIME_WAIT
        let bytes = message(SOCK_DIAG_BY_FAMILY, 0, 29, &payload);
        assert!(
            parse_datagram(&bytes, 29, libc::AF_INET as u8)
                .unwrap_err()
                .contains("non-listener")
        );
    }

    #[test]
    fn live_snapshot_excludes_non_listeners() {
        let records = snapshot().unwrap();
        assert!(records.iter().all(|record| record.identity.port != 0));
    }
}
