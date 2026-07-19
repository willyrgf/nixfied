use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use super::{
    EndpointFamily, KernelSocketIdentity, ListenerHolder, ListenerIdentity, ListenerRecord,
};
use crate::service::process::{macos_process_ids, platform_start_identity, process_group};

const XINPGEN_LEN: usize = 24;
const XINPCB_MIN_LEN: usize = 104;
const XSOCKET_MIN_LEN: usize = 104;
const XTCPCB_MIN_LEN: usize = 40;
const XSO_INPCB: u32 = 0x010;
const XSO_SOCKET: u32 = 0x001;
const XSO_RCVBUF: u32 = 0x002;
const XSO_SNDBUF: u32 = 0x004;
const XSO_STATS: u32 = 0x008;
const XSO_TCPCB: u32 = 0x020;
const INP_IPV4: u8 = 0x1;
const INP_IPV6: u8 = 0x2;
const INP_V4MAPPEDV6: u8 = 0x4;
const IN6P_IPV6_V6ONLY: u32 = 0x0000_8000;
const IN6P_BINDV6ONLY: u32 = 0x0100_0000;
const TCPS_LISTEN: u32 = 1;
const PROC_PIDLISTFDS: libc::c_int = 1;
const PROC_PIDFDSOCKETINFO: libc::c_int = 3;
const PROX_FDTYPE_SOCKET: u32 = 2;
const SOCKINFO_TCP: u32 = 2;
const SOCKET_FDINFO_SO_OFFSET: usize = 160;
const SOCKET_FDINFO_PROTOCOL_OFFSET: usize = 180;
const SOCKET_FDINFO_FAMILY_OFFSET: usize = 184;
const SOCKET_FDINFO_KIND_OFFSET: usize = 256;
const SOCKET_FDINFO_MIN_LEN: usize = SOCKET_FDINFO_KIND_OFFSET + 4;
const SNAPSHOT_ATTEMPTS: usize = 3;
const PCBLIST_READ_ATTEMPTS: usize = 3;

#[derive(Debug)]
enum ParseFailure {
    GenerationChanged,
    Invalid(String),
}

enum PcblistReadFailure {
    RetryableEnomem,
    Fatal(String),
}

pub(super) fn snapshot() -> Result<Vec<ListenerRecord>, String> {
    for attempt in 0..SNAPSHOT_ATTEMPTS {
        let bytes = read_pcblist()?;
        match parse_pcblist(&bytes) {
            Ok(records) => return Ok(records),
            Err(ParseFailure::GenerationChanged) if attempt + 1 < SNAPSHOT_ATTEMPTS => continue,
            Err(ParseFailure::GenerationChanged) => {
                return Err("macOS TCP PCB generation changed during every snapshot".to_string());
            }
            Err(ParseFailure::Invalid(message)) => return Err(message),
        }
    }
    Err("macOS TCP PCB snapshot retry exhausted".to_string())
}

fn read_pcblist() -> Result<Vec<u8>, String> {
    read_pcblist_bounded(read_pcblist_once)
}

fn read_pcblist_bounded(
    mut read_once: impl FnMut() -> Result<Vec<u8>, PcblistReadFailure>,
) -> Result<Vec<u8>, String> {
    for attempt in 0..PCBLIST_READ_ATTEMPTS {
        match read_once() {
            Ok(bytes) => return Ok(bytes),
            Err(PcblistReadFailure::RetryableEnomem) if attempt + 1 < PCBLIST_READ_ATTEMPTS => {}
            Err(PcblistReadFailure::RetryableEnomem) => {
                return Err(format!(
                    "net.inet.tcp.pcblist_n changed size during all {PCBLIST_READ_ATTEMPTS} read attempts"
                ));
            }
            Err(PcblistReadFailure::Fatal(message)) => return Err(message),
        }
    }
    Err("net.inet.tcp.pcblist_n read retry exhausted".to_string())
}

fn read_pcblist_once() -> Result<Vec<u8>, PcblistReadFailure> {
    let name = c"net.inet.tcp.pcblist_n";
    let mut required = 0_usize;
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut required,
            std::ptr::null_mut(),
            0,
        )
    } != 0
    {
        return Err(PcblistReadFailure::Fatal(format!(
            "failed to size net.inet.tcp.pcblist_n: {}",
            io::Error::last_os_error()
        )));
    }
    if required < 2 * XINPGEN_LEN {
        return Err(PcblistReadFailure::Fatal(format!(
            "net.inet.tcp.pcblist_n reported invalid size {required}"
        )));
    }
    let mut bytes = vec![0_u8; required];
    let mut actual = bytes.len();
    if unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            bytes.as_mut_ptr().cast(),
            &mut actual,
            std::ptr::null_mut(),
            0,
        )
    } == 0
    {
        if actual > bytes.len() {
            return Err(PcblistReadFailure::Fatal(
                "net.inet.tcp.pcblist_n returned an oversized result".to_string(),
            ));
        }
        bytes.truncate(actual);
        return Ok(bytes);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ENOMEM) {
        Err(PcblistReadFailure::RetryableEnomem)
    } else {
        Err(PcblistReadFailure::Fatal(format!(
            "failed to read net.inet.tcp.pcblist_n: {error}"
        )))
    }
}

fn parse_pcblist(bytes: &[u8]) -> Result<Vec<ListenerRecord>, ParseFailure> {
    if bytes.len() < 2 * XINPGEN_LEN {
        return Err(ParseFailure::Invalid(
            "truncated xinpgen envelope".to_string(),
        ));
    }
    let header = parse_generation(&bytes[..XINPGEN_LEN])?;
    let trailer_offset = bytes.len() - XINPGEN_LEN;
    let trailer = parse_generation(&bytes[trailer_offset..])?;
    if header.1 != trailer.1 || header.2 != trailer.2 {
        return Err(ParseFailure::GenerationChanged);
    }

    let mut records = Vec::new();
    let mut offset = XINPGEN_LEN;
    while offset < trailer_offset {
        let inpcb = parse_record(bytes, offset, trailer_offset, XSO_INPCB, XINPCB_MIN_LEN)?;
        offset = inpcb.next;
        let socket = parse_record(bytes, offset, trailer_offset, XSO_SOCKET, XSOCKET_MIN_LEN)?;
        offset = socket.next;
        let receive = parse_record(bytes, offset, trailer_offset, XSO_RCVBUF, 8)?;
        offset = receive.next;
        let send = parse_record(bytes, offset, trailer_offset, XSO_SNDBUF, 8)?;
        offset = send.next;
        let stats = parse_record(bytes, offset, trailer_offset, XSO_STATS, 8)?;
        offset = stats.next;
        let tcp = parse_record(bytes, offset, trailer_offset, XSO_TCPCB, XTCPCB_MIN_LEN)?;
        offset = tcp.next;

        let tcp_state = read_u32_ne(tcp.bytes, 36)?;
        if tcp_state != TCPS_LISTEN {
            continue;
        }
        records.push(parse_listener(inpcb.bytes, socket.bytes)?);
    }
    if offset != trailer_offset {
        return Err(ParseFailure::Invalid(
            "macOS PCB records did not end at the xinpgen trailer".to_string(),
        ));
    }
    records.sort_by(|left, right| left.identity.cmp(&right.identity));
    Ok(records)
}

fn parse_generation(bytes: &[u8]) -> Result<(u32, u64, u64), ParseFailure> {
    if bytes.len() != XINPGEN_LEN || read_u32_ne(bytes, 0)? as usize != XINPGEN_LEN {
        return Err(ParseFailure::Invalid(
            "unsupported macOS xinpgen layout".to_string(),
        ));
    }
    Ok((
        read_u32_ne(bytes, 4)?,
        read_u64_ne(bytes, 8)?,
        read_u64_ne(bytes, 16)?,
    ))
}

struct ParsedRecord<'a> {
    bytes: &'a [u8],
    next: usize,
}

fn parse_record<'a>(
    bytes: &'a [u8],
    offset: usize,
    limit: usize,
    expected_kind: u32,
    minimum_len: usize,
) -> Result<ParsedRecord<'a>, ParseFailure> {
    if offset > limit || limit - offset < 8 {
        return Err(ParseFailure::Invalid(format!(
            "truncated macOS PCB record kind {expected_kind}"
        )));
    }
    let length = read_u32_ne(bytes, offset)? as usize;
    let kind = read_u32_ne(bytes, offset + 4)?;
    if kind != expected_kind {
        return Err(ParseFailure::Invalid(format!(
            "macOS PCB record kind {kind} appeared where {expected_kind} was required"
        )));
    }
    if length < minimum_len || length > limit - offset {
        return Err(ParseFailure::Invalid(format!(
            "invalid macOS PCB record length {length} for kind {kind}"
        )));
    }
    let aligned = align8(length)
        .ok_or_else(|| ParseFailure::Invalid("macOS PCB record length overflow".to_string()))?;
    if aligned > limit - offset {
        return Err(ParseFailure::Invalid(format!(
            "truncated alignment padding for macOS PCB record kind {kind}"
        )));
    }
    Ok(ParsedRecord {
        bytes: &bytes[offset..offset + length],
        next: offset + aligned,
    })
}

fn parse_listener(inpcb: &[u8], socket: &[u8]) -> Result<ListenerRecord, ParseFailure> {
    let port = read_u16_be(inpcb, 18)?;
    if port == 0 {
        return Err(ParseFailure::Invalid(
            "macOS LISTEN record has local port zero".to_string(),
        ));
    }
    let protocol = read_u32_ne(socket, 36)? as libc::c_int;
    if protocol != libc::IPPROTO_TCP {
        return Err(ParseFailure::Invalid(format!(
            "macOS TCP PCB socket reported protocol {protocol}"
        )));
    }
    let socket_family = read_u32_ne(socket, 40)? as libc::c_int;
    let vflag = *inpcb
        .get(44)
        .ok_or_else(|| ParseFailure::Invalid("truncated macOS inp_vflag".to_string()))?;
    let local = inpcb
        .get(64..80)
        .ok_or_else(|| ParseFailure::Invalid("truncated macOS local address".to_string()))?;
    let flags = read_u32_ne(inpcb, 36)?;
    let (family, address, ipv6_only) = match socket_family {
        libc::AF_INET => {
            if vflag & INP_IPV4 == 0 || vflag & INP_IPV6 != 0 {
                return Err(ParseFailure::Invalid(format!(
                    "macOS IPv4 listener has inconsistent inp_vflag {vflag:#x}"
                )));
            }
            (
                EndpointFamily::Ipv4,
                IpAddr::V4(Ipv4Addr::new(local[12], local[13], local[14], local[15])),
                None,
            )
        }
        libc::AF_INET6 => {
            if vflag & INP_IPV6 == 0 {
                return Err(ParseFailure::Invalid(format!(
                    "macOS IPv6 listener has inconsistent inp_vflag {vflag:#x}"
                )));
            }
            let address = Ipv6Addr::from(<[u8; 16]>::try_from(local).map_err(|_| {
                ParseFailure::Invalid("invalid macOS IPv6 local address".to_string())
            })?);
            if vflag & INP_V4MAPPEDV6 != 0 && address.to_ipv4_mapped().is_none() {
                return Err(ParseFailure::Invalid(
                    "macOS listener marks a non-mapped IPv6 address as V4MAPPEDV6".to_string(),
                ));
            }
            (
                EndpointFamily::Ipv6,
                IpAddr::V6(address),
                Some(flags & (IN6P_IPV6_V6ONLY | IN6P_BINDV6ONLY) != 0),
            )
        }
        other => {
            return Err(ParseFailure::Invalid(format!(
                "unsupported macOS TCP listener family {other}"
            )));
        }
    };
    let pcb = read_u64_ne(inpcb, 8)?;
    let pcb_generation = read_u64_ne(inpcb, 28)?;
    let socket_handle = read_u64_ne(socket, 8)?;
    let socket_generation = read_u64_ne(socket, 76)?;
    if pcb == 0 || socket_handle == 0 {
        return Err(ParseFailure::Invalid(
            "macOS listener omitted its kernel PCB/socket identity".to_string(),
        ));
    }
    let mut pid_hints = [read_u32_ne(socket, 68)?, read_u32_ne(socket, 72)?]
        .into_iter()
        .filter(|pid| *pid > 0)
        .collect::<Vec<_>>();
    pid_hints.sort_unstable();
    pid_hints.dedup();
    Ok(ListenerRecord {
        identity: ListenerIdentity {
            family,
            address,
            port,
            kernel: KernelSocketIdentity::Macos {
                pcb,
                pcb_generation,
                socket: socket_handle,
                socket_generation,
            },
            uid: read_u32_ne(socket, 64)?,
            ipv6_only,
        },
        holders: Vec::new(),
        pid_hints,
    })
}

pub(super) fn correlate(records: &mut [ListenerRecord]) -> Result<(), String> {
    let mut indexes = BTreeMap::<u64, Vec<usize>>::new();
    let mut hints = BTreeSet::new();
    for (index, record) in records.iter().enumerate() {
        let KernelSocketIdentity::Macos { socket, .. } = record.identity.kernel;
        indexes.entry(socket).or_default().push(index);
        hints.extend(record.pid_hints.iter().copied());
    }
    if indexes.is_empty() {
        return Ok(());
    }
    let all = macos_process_ids().map_err(|error| {
        format!("failed to enumerate macOS processes for endpoint correlation: {error}")
    })?;
    let mut ordered = hints.iter().copied().collect::<Vec<_>>();
    ordered.extend(all.into_iter().filter(|pid| !hints.contains(pid)));
    for pid in ordered {
        for descriptor in list_socket_fds(pid) {
            let Some(handle) = socket_handle(pid, descriptor)? else {
                continue;
            };
            let Some(record_indexes) = indexes.get(&handle) else {
                continue;
            };
            let Some(pgid) = process_group(pid).map_err(|error| error.message)? else {
                continue;
            };
            let holder = ListenerHolder {
                pid,
                pgid,
                platform_start: platform_start_identity(pid),
            };
            for index in record_indexes {
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

fn list_socket_fds(pid: u32) -> Vec<i32> {
    let required = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            PROC_PIDLISTFDS,
            0,
            std::ptr::null_mut(),
            0,
        )
    };
    if required <= 0 {
        return Vec::new();
    }
    let mut capacity = required as usize + 8 * std::mem::size_of::<libc::proc_fdinfo>();
    loop {
        let entry_size = std::mem::size_of::<libc::proc_fdinfo>();
        capacity = capacity.div_ceil(entry_size) * entry_size;
        let mut bytes = vec![0_u8; capacity];
        let actual = unsafe {
            libc::proc_pidinfo(
                pid as libc::c_int,
                PROC_PIDLISTFDS,
                0,
                bytes.as_mut_ptr().cast(),
                bytes.len() as libc::c_int,
            )
        };
        if actual <= 0 {
            return Vec::new();
        }
        if actual as usize >= bytes.len() {
            capacity = capacity.saturating_mul(2);
            continue;
        }
        let actual = actual as usize;
        if actual % entry_size != 0 {
            return Vec::new();
        }
        return bytes[..actual]
            .chunks_exact(entry_size)
            .filter_map(|entry| {
                let kind = read_u32_ne_raw(entry, 4).ok()?;
                let descriptor = read_i32_ne_raw(entry, 0).ok()?;
                (kind == PROX_FDTYPE_SOCKET).then_some(descriptor)
            })
            .collect();
    }
}

fn socket_handle(pid: u32, descriptor: i32) -> Result<Option<u64>, String> {
    let mut capacity = 256_usize;
    loop {
        let mut bytes = vec![0_u8; capacity];
        let actual = unsafe {
            libc::proc_pidfdinfo(
                pid as libc::c_int,
                descriptor,
                PROC_PIDFDSOCKETINFO,
                bytes.as_mut_ptr().cast(),
                bytes.len() as libc::c_int,
            )
        };
        if actual <= 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ENOMEM) {
                capacity = capacity.saturating_mul(2);
                continue;
            }
            return Ok(None);
        }
        if actual as usize > bytes.len() {
            capacity = actual as usize;
            continue;
        }
        let bytes = &bytes[..actual as usize];
        if bytes.len() < SOCKET_FDINFO_MIN_LEN {
            return Err(format!(
                "macOS socket fd info for pid {pid} fd {descriptor} used an unsupported layout"
            ));
        }
        if read_u32_ne_raw(bytes, SOCKET_FDINFO_KIND_OFFSET)? != SOCKINFO_TCP
            || read_u32_ne_raw(bytes, SOCKET_FDINFO_PROTOCOL_OFFSET)? != libc::IPPROTO_TCP as u32
            || !matches!(
                read_u32_ne_raw(bytes, SOCKET_FDINFO_FAMILY_OFFSET)? as libc::c_int,
                libc::AF_INET | libc::AF_INET6
            )
        {
            return Ok(None);
        }
        let handle = read_u64_ne_raw(bytes, SOCKET_FDINFO_SO_OFFSET)?;
        return Ok((handle != 0).then_some(handle));
    }
}

fn read_u32_ne(bytes: &[u8], offset: usize) -> Result<u32, ParseFailure> {
    read_u32_ne_raw(bytes, offset).map_err(ParseFailure::Invalid)
}

fn read_u16_be(bytes: &[u8], offset: usize) -> Result<u16, ParseFailure> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| ParseFailure::Invalid("truncated macOS PCB u16 field".to_string()))?;
    Ok(u16::from_be_bytes(value.try_into().map_err(|_| {
        ParseFailure::Invalid("invalid macOS PCB u16 field".to_string())
    })?))
}

fn read_u64_ne(bytes: &[u8], offset: usize) -> Result<u64, ParseFailure> {
    read_u64_ne_raw(bytes, offset).map_err(ParseFailure::Invalid)
}

fn read_u32_ne_raw(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let field = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| "truncated macOS u32 field".to_string())?;
    Ok(u32::from_ne_bytes(
        field
            .try_into()
            .map_err(|_| "invalid macOS u32 field".to_string())?,
    ))
}

fn read_u64_ne_raw(bytes: &[u8], offset: usize) -> Result<u64, String> {
    let field = bytes
        .get(offset..offset + 8)
        .ok_or_else(|| "truncated macOS u64 field".to_string())?;
    Ok(u64::from_ne_bytes(
        field
            .try_into()
            .map_err(|_| "invalid macOS u64 field".to_string())?,
    ))
}

fn read_i32_ne_raw(bytes: &[u8], offset: usize) -> Result<i32, String> {
    let field = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| "truncated macOS i32 field".to_string())?;
    Ok(i32::from_ne_bytes(
        field
            .try_into()
            .map_err(|_| "invalid macOS i32 field".to_string())?,
    ))
}

fn align8(value: usize) -> Option<usize> {
    value.checked_add(7).map(|value| value & !7)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generation(generation: u64, socket_generation: u64) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(XINPGEN_LEN);
        bytes.extend_from_slice(&(XINPGEN_LEN as u32).to_ne_bytes());
        bytes.extend_from_slice(&1_u32.to_ne_bytes());
        bytes.extend_from_slice(&generation.to_ne_bytes());
        bytes.extend_from_slice(&socket_generation.to_ne_bytes());
        bytes
    }

    fn record(kind: u32, length: usize) -> Vec<u8> {
        let mut bytes = vec![0_u8; align8(length).unwrap()];
        bytes[..4].copy_from_slice(&(length as u32).to_ne_bytes());
        bytes[4..8].copy_from_slice(&kind.to_ne_bytes());
        bytes
    }

    fn pcb_dump() -> Vec<u8> {
        let mut dump = generation(7, 11);
        let mut inpcb = record(XSO_INPCB, XINPCB_MIN_LEN);
        inpcb[8..16].copy_from_slice(&31_u64.to_ne_bytes());
        inpcb[18..20].copy_from_slice(&23080_u16.to_be_bytes());
        inpcb[28..36].copy_from_slice(&37_u64.to_ne_bytes());
        inpcb[44] = INP_IPV4;
        inpcb[76..80].copy_from_slice(&Ipv4Addr::LOCALHOST.octets());
        dump.extend(inpcb);
        let mut socket = record(XSO_SOCKET, XSOCKET_MIN_LEN);
        socket[8..16].copy_from_slice(&41_u64.to_ne_bytes());
        socket[36..40].copy_from_slice(&(libc::IPPROTO_TCP as u32).to_ne_bytes());
        socket[40..44].copy_from_slice(&(libc::AF_INET as u32).to_ne_bytes());
        socket[64..68].copy_from_slice(&501_u32.to_ne_bytes());
        socket[76..84].copy_from_slice(&43_u64.to_ne_bytes());
        dump.extend(socket);
        dump.extend(record(XSO_RCVBUF, 32));
        dump.extend(record(XSO_SNDBUF, 32));
        dump.extend(record(XSO_STATS, 136));
        let mut tcp = record(XSO_TCPCB, 196);
        tcp[36..40].copy_from_slice(&TCPS_LISTEN.to_ne_bytes());
        dump.extend(tcp);
        dump.extend(generation(7, 11));
        dump
    }

    #[test]
    fn parser_rejects_generation_churn() {
        let mut dump = pcb_dump();
        let trailer = dump.len() - XINPGEN_LEN;
        dump[trailer + 8..trailer + 16].copy_from_slice(&8_u64.to_ne_bytes());
        assert!(matches!(
            parse_pcblist(&dump),
            Err(ParseFailure::GenerationChanged)
        ));
    }

    #[test]
    fn pcblist_enomem_retries_are_bounded() {
        let mut attempts = 0;
        let error = read_pcblist_bounded(|| {
            attempts += 1;
            Err(PcblistReadFailure::RetryableEnomem)
        })
        .expect_err("repeated PCB growth must exhaust instead of looping forever");

        assert_eq!(attempts, PCBLIST_READ_ATTEMPTS);
        assert!(error.contains("all 3 read attempts"));
    }

    #[test]
    fn parser_rejects_record_kind_and_alignment_corruption() {
        let mut wrong_kind = pcb_dump();
        wrong_kind[XINPGEN_LEN + 4..XINPGEN_LEN + 8].copy_from_slice(&XSO_SOCKET.to_ne_bytes());
        assert!(matches!(
            parse_pcblist(&wrong_kind),
            Err(ParseFailure::Invalid(_))
        ));

        let mut wrong_length = pcb_dump();
        wrong_length[XINPGEN_LEN..XINPGEN_LEN + 4].copy_from_slice(&103_u32.to_ne_bytes());
        assert!(matches!(
            parse_pcblist(&wrong_length),
            Err(ParseFailure::Invalid(_))
        ));
    }

    #[test]
    fn parser_retains_socket_identity_and_address() {
        let records = parse_pcblist(&pcb_dump()).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].identity.address, IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(records[0].identity.port, 23080);
        assert_eq!(records[0].identity.uid, 501);
        assert_eq!(
            records[0].identity.kernel,
            KernelSocketIdentity::Macos {
                pcb: 31,
                pcb_generation: 37,
                socket: 41,
                socket_generation: 43,
            }
        );
    }

    #[test]
    fn socket_info_handle_requires_tcp_layout() {
        let mut bytes = vec![0_u8; SOCKET_FDINFO_MIN_LEN];
        bytes[SOCKET_FDINFO_SO_OFFSET..SOCKET_FDINFO_SO_OFFSET + 8]
            .copy_from_slice(&47_u64.to_ne_bytes());
        bytes[SOCKET_FDINFO_PROTOCOL_OFFSET..SOCKET_FDINFO_PROTOCOL_OFFSET + 4]
            .copy_from_slice(&(libc::IPPROTO_TCP as u32).to_ne_bytes());
        bytes[SOCKET_FDINFO_FAMILY_OFFSET..SOCKET_FDINFO_FAMILY_OFFSET + 4]
            .copy_from_slice(&(libc::AF_INET as u32).to_ne_bytes());
        bytes[SOCKET_FDINFO_KIND_OFFSET..SOCKET_FDINFO_KIND_OFFSET + 4]
            .copy_from_slice(&SOCKINFO_TCP.to_ne_bytes());
        assert_eq!(
            read_u64_ne_raw(&bytes, SOCKET_FDINFO_SO_OFFSET).unwrap(),
            47
        );
        bytes[SOCKET_FDINFO_KIND_OFFSET..SOCKET_FDINFO_KIND_OFFSET + 4]
            .copy_from_slice(&0_u32.to_ne_bytes());
        assert_ne!(
            read_u32_ne_raw(&bytes, SOCKET_FDINFO_KIND_OFFSET).unwrap(),
            SOCKINFO_TCP
        );
    }
}
