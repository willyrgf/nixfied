//! Decode complete libproc records using the selected SDK's generated layout.
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[allow(
    non_camel_case_types,
    non_upper_case_globals,
    non_snake_case,
    dead_code,
    clippy::all
)]
pub(crate) mod sdk {
    include!(concat!(env!("OUT_DIR"), "/macos_socket_bindings.rs"));
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Socket {
    pub address: IpAddr,
    pub port: u16,
    pub handle: u64,
    pub generation: u64,
}

pub(crate) fn inspect(pid: i32, fd: i32) -> io::Result<Option<Socket>> {
    read_socket(|record| {
        // SAFETY: libproc receives a writable, fully sized SDK record. Clear
        // errno first so zero/short results cannot inherit an unrelated error.
        unsafe {
            *libc::__error() = 0;
            let bytes = libc::proc_pidfdinfo(
                pid,
                fd,
                sdk::PROC_PIDFDSOCKETINFO as i32,
                std::ptr::from_mut(record).cast(),
                std::mem::size_of_val(record) as i32,
            );
            (bytes, *libc::__error())
        }
    })
}

pub(crate) fn read_socket(
    read: impl FnOnce(&mut sdk::socket_fdinfo) -> (i32, i32),
) -> io::Result<Option<Socket>> {
    // SAFETY: the SDK record consists solely of C integer/array/union fields;
    // zero is valid for every field. No references or Rust enums are present.
    let mut record: sdk::socket_fdinfo = unsafe { std::mem::zeroed() };
    let (bytes, errno) = read(&mut record);
    if bytes != std::mem::size_of_val(&record) as i32 {
        return Err(io::Error::from_raw_os_error(if bytes <= 0 && errno != 0 {
            errno
        } else {
            libc::EPROTO
        }));
    }
    let socket = record.psi;
    if socket.soi_kind != sdk::SOCKINFO_TCP as i32 {
        return Ok(None);
    }
    if socket.soi_type != libc::SOCK_STREAM || socket.soi_protocol != libc::IPPROTO_TCP {
        return Err(io::Error::from_raw_os_error(libc::EPROTO));
    }
    // SAFETY: the complete record's kind identifies the TCP union member.
    let tcp = unsafe { socket.soi_proto.pri_tcp };
    if tcp.tcpsi_state != sdk::TSI_S_LISTEN as i32 {
        return Ok(None);
    }
    let inet = tcp.tcpsi_ini;
    let address = match socket.soi_family {
        libc::AF_INET if u32::from(inet.insi_vflag) & sdk::INI_IPV4 != 0 => {
            // SAFETY: family and vflag select the IPv4 union member. s_addr
            // stores network bytes, so preserve its memory byte order.
            let address = unsafe { inet.insi_laddr.ina_46.i46a_addr4.s_addr };
            IpAddr::V4(Ipv4Addr::from(address.to_ne_bytes()))
        }
        libc::AF_INET6 if u32::from(inet.insi_vflag) & sdk::INI_IPV6 != 0 => {
            // SAFETY: family and vflag select IPv6; the SDK byte-array member
            // exposes its network bytes without guessed offsets or layout.
            let address = unsafe { inet.insi_laddr.ina_6.__u6_addr.__u6_addr8 };
            IpAddr::V6(Ipv6Addr::from(address))
        }
        _ => return Err(io::Error::from_raw_os_error(libc::EPROTO)),
    };
    let port = u16::from_be(inet.insi_lport as u16);
    if socket.soi_so == 0 || port == 0 {
        return Err(io::Error::from_raw_os_error(libc::EPROTO));
    }
    Ok(Some(Socket {
        address,
        port,
        handle: socket.soi_so,
        generation: inet.insi_gencnt,
    }))
}
