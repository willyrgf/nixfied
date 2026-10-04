#![cfg(target_os = "macos")]

// Compile the production decoder with the same generated SDK bindings, then
// inject raw syscall outcomes. Live FD inspection has separate endpoint tests.
#[allow(dead_code)]
#[path = "../src/service/endpoint/macos_socket.rs"]
mod decoder;

use decoder::{Socket, read_socket, sdk};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

fn listener(ipv6: bool) -> sdk::socket_fdinfo {
    // SAFETY: all fields are C scalars, arrays or unions with valid zero values.
    let mut record: sdk::socket_fdinfo = unsafe { std::mem::zeroed() };
    record.psi.soi_kind = sdk::SOCKINFO_TCP as i32;
    record.psi.soi_type = libc::SOCK_STREAM;
    record.psi.soi_protocol = libc::IPPROTO_TCP;
    record.psi.soi_family = if ipv6 { libc::AF_INET6 } else { libc::AF_INET };
    record.psi.soi_so = 0x1234_5678_9abc_def0;
    // SAFETY: this fixture selects and initializes the TCP union member.
    let tcp = unsafe { &mut record.psi.soi_proto.pri_tcp };
    tcp.tcpsi_state = sdk::TSI_S_LISTEN as i32;
    tcp.tcpsi_ini.insi_vflag = if ipv6 { sdk::INI_IPV6 } else { sdk::INI_IPV4 } as u8;
    tcp.tcpsi_ini.insi_lport = u16::to_be(23080).into();
    tcp.tcpsi_ini.insi_gencnt = 0xfedc_ba98_7654_3210;
    if ipv6 {
        tcp.tcpsi_ini.insi_laddr.ina_6.__u6_addr.__u6_addr8 = Ipv6Addr::LOCALHOST.octets();
    } else {
        tcp.tcpsi_ini.insi_laddr.ina_46.i46a_addr4.s_addr =
            u32::from_ne_bytes(Ipv4Addr::LOCALHOST.octets());
    }
    record
}

fn decode(record: sdk::socket_fdinfo, bytes: i32, errno: i32) -> std::io::Result<Option<Socket>> {
    read_socket(|output| {
        *output = record;
        (bytes, errno)
    })
}

#[test]
fn sdk_decoder_preserves_exact_addresses_and_kernel_identities() {
    for ipv6 in [false, true] {
        let record = listener(ipv6);
        let bytes = std::mem::size_of_val(&record) as i32;
        let expected = Socket {
            address: if ipv6 {
                IpAddr::V6(Ipv6Addr::LOCALHOST)
            } else {
                IpAddr::V4(Ipv4Addr::LOCALHOST)
            },
            port: 23080,
            handle: 0x1234_5678_9abc_def0,
            generation: 0xfedc_ba98_7654_3210,
        };
        assert_eq!(decode(record, bytes, 0).unwrap(), Some(expected));
    }
}

#[test]
fn sdk_decoder_rejects_failed_and_incomplete_reads() {
    let record = listener(false);
    let size = std::mem::size_of_val(&record) as i32;
    // Read failures precede address decoding; their behavior is family-independent.
    // Positive incomplete lengths must ignore even a stale syscall errno.
    for (bytes, errno, expected) in [
        (0, 0, libc::EPROTO),
        (-1, 0, libc::EPROTO),
        (0, libc::EPERM, libc::EPERM),
        (-1, libc::EBADF, libc::EBADF),
        (-1, libc::ESRCH, libc::ESRCH),
        (size - 1, libc::EPERM, libc::EPROTO),
        (size + 1, libc::ESRCH, libc::EPROTO),
    ] {
        assert_eq!(
            decode(record, bytes, errno).unwrap_err().raw_os_error(),
            Some(expected)
        );
    }
}

#[test]
fn sdk_decoder_rejects_incoherent_socket_records() {
    for ipv6 in [false, true] {
        let record = listener(ipv6);
        let size = std::mem::size_of_val(&record) as i32;
        for mutate in [
            |record: &mut sdk::socket_fdinfo| record.psi.soi_so = 0,
            |record: &mut sdk::socket_fdinfo| record.psi.soi_family = libc::AF_UNIX,
            |record: &mut sdk::socket_fdinfo| record.psi.soi_type = libc::SOCK_DGRAM,
            |record: &mut sdk::socket_fdinfo| record.psi.soi_protocol = libc::IPPROTO_UDP,
            |record: &mut sdk::socket_fdinfo| record.psi.soi_proto.pri_tcp.tcpsi_ini.insi_vflag = 0,
            |record: &mut sdk::socket_fdinfo| record.psi.soi_proto.pri_tcp.tcpsi_ini.insi_lport = 0,
        ] {
            let mut invalid = record;
            mutate(&mut invalid);
            assert_eq!(
                decode(invalid, size, 0).unwrap_err().raw_os_error(),
                Some(libc::EPROTO)
            );
        }
        let mut non_tcp = record;
        non_tcp.psi.soi_kind = 0;
        assert_eq!(decode(non_tcp, size, 0).unwrap(), None);
        let mut non_listener = record;
        non_listener.psi.soi_proto.pri_tcp.tcpsi_state = 0;
        assert_eq!(decode(non_listener, size, 0).unwrap(), None);
    }
}
