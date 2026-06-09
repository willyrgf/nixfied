use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

#[cfg(target_os = "linux")]
use std::collections::BTreeSet;
#[cfg(target_os = "linux")]
use std::path::Path;

use nixfied_model::EndpointSpec;
use serde::Serialize;

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::service::process::{platform_start_identity, process_group};

pub struct ExpectedEndpointOwner<'a> {
    pub pid: u32,
    pub pgid: i32,
    pub process_key: &'a str,
    pub platform_start_identity: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedEndpointOwnership {
    pub endpoint_id: String,
    pub address: String,
    pub port: u16,
    pub owner_pid: u32,
    pub owner_pgid: i32,
    pub owner_platform_start: Option<String>,
    pub expected_process_key: String,
    pub matched_by: String,
}

pub fn verify_endpoint_ownership(
    endpoint: &EndpointSpec,
    selected_port: u16,
    expected: &ExpectedEndpointOwner<'_>,
) -> RuntimeResult<VerifiedEndpointOwnership> {
    let expected_addr = loopback_addr(&endpoint.host)?;
    let owner = resolve_listener_owner(expected_addr, selected_port)?;
    if owner.pgid != expected.pgid {
        return Err(port_unverifiable(format!(
            "listener for {}:{selected_port} belongs to pid {} pgid {}, not service pgid {}",
            endpoint.host, owner.pid, owner.pgid, expected.pgid
        )));
    }
    let matched_by = if owner.pid == expected.pid {
        if expected.platform_start_identity.is_some()
            && owner.platform_start.as_deref() != expected.platform_start_identity
        {
            return Err(port_unverifiable(format!(
                "listener pid {} start identity did not match tracked service process",
                owner.pid
            )));
        }
        "tracked-process"
    } else {
        "tracked-process-group"
    };
    Ok(VerifiedEndpointOwnership {
        endpoint_id: endpoint.endpoint_id.clone(),
        address: endpoint.host.clone(),
        port: selected_port,
        owner_pid: owner.pid,
        owner_pgid: owner.pgid,
        owner_platform_start: owner.platform_start,
        expected_process_key: expected.process_key.to_string(),
        matched_by: matched_by.to_string(),
    })
}

#[derive(Debug, Clone)]
struct ObservedPortOwner {
    pid: u32,
    pgid: i32,
    platform_start: Option<String>,
}

fn loopback_addr(host: &str) -> RuntimeResult<IpAddr> {
    let addr = IpAddr::from_str(host).map_err(|error| {
        RuntimeError::new(
            ErrorCode::ModelAdmission,
            format!("invalid endpoint host {host}: {error}"),
        )
    })?;
    if !addr.is_loopback() {
        return Err(RuntimeError::new(
            ErrorCode::ModelAdmission,
            format!("endpoint ownership supports loopback only, got {host}"),
        ));
    }
    Ok(addr)
}

fn port_unverifiable(message: impl Into<String>) -> RuntimeError {
    RuntimeError::new(ErrorCode::PortUnverifiable, message)
}

#[cfg(target_os = "linux")]
fn resolve_listener_owner(addr: IpAddr, port: u16) -> RuntimeResult<ObservedPortOwner> {
    let inodes = listener_inodes_linux(addr, port)?;
    if inodes.is_empty() {
        return Err(port_unverifiable(format!(
            "no listening socket was found for {addr}:{port}"
        )));
    }
    for entry in std::fs::read_dir("/proc").map_err(|error| {
        port_unverifiable(format!(
            "failed to inspect /proc for port ownership: {error}"
        ))
    })? {
        let entry = entry.map_err(|error| {
            port_unverifiable(format!(
                "failed to inspect /proc for port ownership: {error}"
            ))
        })?;
        let Some(pid) = entry
            .file_name()
            .to_string_lossy()
            .parse::<u32>()
            .ok()
            .filter(|pid| *pid > 0)
        else {
            continue;
        };
        if process_owns_socket_inode(pid, &inodes) {
            let Some(pgid) = process_group(pid)? else {
                continue;
            };
            return Ok(ObservedPortOwner {
                pid,
                pgid,
                platform_start: platform_start_identity(pid),
            });
        }
    }
    Err(port_unverifiable(format!(
        "listening socket for {addr}:{port} could not be mapped to a process"
    )))
}

#[cfg(target_os = "linux")]
fn listener_inodes_linux(addr: IpAddr, port: u16) -> RuntimeResult<BTreeSet<String>> {
    let mut inodes = BTreeSet::new();
    collect_listener_inodes_linux(Path::new("/proc/net/tcp"), addr, port, &mut inodes)?;
    collect_listener_inodes_linux(Path::new("/proc/net/tcp6"), addr, port, &mut inodes)?;
    Ok(inodes)
}

#[cfg(target_os = "linux")]
fn collect_listener_inodes_linux(
    path: &Path,
    addr: IpAddr,
    port: u16,
    inodes: &mut BTreeSet<String>,
) -> RuntimeResult<()> {
    let content = std::fs::read_to_string(path).map_err(|error| {
        port_unverifiable(format!(
            "failed to inspect {} for port ownership: {error}",
            path.display()
        ))
    })?;
    for line in content.lines().skip(1) {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() <= 9 || fields[3] != "0A" {
            continue;
        }
        if linux_local_endpoint_matches(fields[1], addr, port) {
            inodes.insert(fields[9].to_string());
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_local_endpoint_matches(local: &str, expected_addr: IpAddr, expected_port: u16) -> bool {
    let Some((addr_hex, port_hex)) = local.split_once(':') else {
        return false;
    };
    let Ok(port) = u16::from_str_radix(port_hex, 16) else {
        return false;
    };
    if port != expected_port {
        return false;
    }
    match expected_addr {
        IpAddr::V4(expected) => {
            let Ok(raw) = u32::from_str_radix(addr_hex, 16) else {
                return false;
            };
            let observed = Ipv4Addr::from(raw.to_le_bytes());
            observed == expected
        }
        IpAddr::V6(expected) => {
            if addr_hex.len() != 32 {
                return false;
            }
            let mut bytes = [0u8; 16];
            for index in 0..4 {
                let chunk = &addr_hex[index * 8..(index + 1) * 8];
                let Ok(raw) = u32::from_str_radix(chunk, 16) else {
                    return false;
                };
                bytes[index * 4..(index + 1) * 4].copy_from_slice(&raw.to_le_bytes());
            }
            let observed = Ipv6Addr::from(bytes);
            observed == expected
        }
    }
}

#[cfg(target_os = "linux")]
fn process_owns_socket_inode(pid: u32, inodes: &BTreeSet<String>) -> bool {
    let fd_dir = format!("/proc/{pid}/fd");
    let Ok(entries) = std::fs::read_dir(fd_dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let Ok(target) = std::fs::read_link(entry.path()) else {
            continue;
        };
        let target = target.to_string_lossy();
        let Some(inode) = target
            .strip_prefix("socket:[")
            .and_then(|value| value.strip_suffix(']'))
        else {
            continue;
        };
        if inodes.contains(inode) {
            return true;
        }
    }
    false
}

#[cfg(target_os = "macos")]
fn resolve_listener_owner(addr: IpAddr, port: u16) -> RuntimeResult<ObservedPortOwner> {
    let capacity = 8192usize;
    let mut buffer = vec![0 as libc::pid_t; capacity];
    let count = unsafe {
        libc::proc_listallpids(
            buffer.as_mut_ptr().cast(),
            (capacity * std::mem::size_of::<libc::pid_t>()) as libc::c_int,
        )
    };
    if count < 0 {
        return Err(port_unverifiable(format!(
            "failed to list pids for port ownership: {}",
            std::io::Error::last_os_error()
        )));
    }
    for pid in buffer.into_iter().take(count as usize) {
        let Ok(pid) = u32::try_from(pid) else {
            continue;
        };
        if pid == 0 || !process_has_tcp_listener_macos(pid, addr, port) {
            continue;
        }
        let Some(pgid) = process_group(pid)? else {
            continue;
        };
        return Ok(ObservedPortOwner {
            pid,
            pgid,
            platform_start: platform_start_identity(pid),
        });
    }
    Err(port_unverifiable(format!(
        "listening socket for {addr}:{port} could not be mapped to a process"
    )))
}

#[cfg(target_os = "macos")]
fn process_has_tcp_listener_macos(pid: u32, addr: IpAddr, port: u16) -> bool {
    let capacity = 4096usize;
    let mut fds = vec![
        libc::proc_fdinfo {
            proc_fd: 0,
            proc_fdtype: 0,
        };
        capacity
    ];
    let bytes = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDLISTFDS,
            0,
            fds.as_mut_ptr().cast(),
            (capacity * std::mem::size_of::<libc::proc_fdinfo>()) as libc::c_int,
        )
    };
    if bytes <= 0 {
        return false;
    }
    let count = bytes as usize / std::mem::size_of::<libc::proc_fdinfo>();
    for fd in fds.into_iter().take(count) {
        if fd.proc_fdtype as libc::c_int != libc::PROX_FDTYPE_SOCKET {
            continue;
        }
        let mut info = std::mem::MaybeUninit::<SocketFdInfo>::zeroed();
        let size = std::mem::size_of::<SocketFdInfo>() as libc::c_int;
        let result = unsafe {
            libc::proc_pidfdinfo(
                pid as libc::c_int,
                fd.proc_fd,
                PROC_PIDFDSOCKETINFO,
                info.as_mut_ptr().cast(),
                size,
            )
        };
        if result != size {
            continue;
        }
        let info = unsafe { info.assume_init() };
        if unsafe { socket_info_matches(&info, addr, port) } {
            return true;
        }
    }
    false
}

#[cfg(target_os = "macos")]
const PROC_PIDFDSOCKETINFO: libc::c_int = 3;
#[cfg(target_os = "macos")]
const SOCKINFO_TCP: libc::c_int = 2;
#[cfg(target_os = "macos")]
const TSI_S_LISTEN: libc::c_int = 1;
#[cfg(target_os = "macos")]
const INI_IPV4: u8 = 0x1;
#[cfg(target_os = "macos")]
const INI_IPV6: u8 = 0x2;

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct SocketFdInfo {
    pfi: [u8; 24],
    psi: SocketInfo,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct SocketInfo {
    prefix: [u8; 232],
    soi_kind: libc::c_int,
    rfu_1: u32,
    soi_proto: SocketProtocolInfo,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
union SocketProtocolInfo {
    pri_tcp: TcpSockInfo,
    padding: [u8; 528],
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct TcpSockInfo {
    tcpsi_ini: InSockInfo,
    tcpsi_state: libc::c_int,
    tcpsi_timer: [libc::c_int; 4],
    tcpsi_mss: libc::c_int,
    tcpsi_flags: u32,
    rfu_1: u32,
    tcpsi_tp: u64,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct InSockInfo {
    insi_fport: libc::c_int,
    insi_lport: libc::c_int,
    insi_gencnt: u64,
    insi_flags: u32,
    insi_flow: u32,
    insi_vflag: u8,
    insi_ip_ttl: u8,
    rfu_1: u32,
    insi_faddr: InAddrUnion,
    insi_laddr: InAddrUnion,
    insi_v4: [u8; 1],
    insi_v6: InSockInfoV6,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct InSockInfoV6 {
    in6_hlim: u8,
    in6_cksum: libc::c_int,
    in6_ifindex: u16,
    in6_hops: i16,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
union InAddrUnion {
    bytes: [u8; 16],
}

#[cfg(target_os = "macos")]
unsafe fn socket_info_matches(info: &SocketFdInfo, addr: IpAddr, port: u16) -> bool {
    if info.psi.soi_kind != SOCKINFO_TCP {
        return false;
    }
    let tcp = unsafe { info.psi.soi_proto.pri_tcp };
    if tcp.tcpsi_state != TSI_S_LISTEN {
        return false;
    }
    if !port_number_matches(tcp.tcpsi_ini.insi_lport, port) {
        return false;
    }
    let local = unsafe { tcp.tcpsi_ini.insi_laddr.bytes };
    match addr {
        IpAddr::V4(expected) => {
            tcp.tcpsi_ini.insi_vflag & INI_IPV4 != 0
                && Ipv4Addr::new(local[12], local[13], local[14], local[15]) == expected
        }
        IpAddr::V6(expected) => {
            tcp.tcpsi_ini.insi_vflag & INI_IPV6 != 0 && Ipv6Addr::from(local) == expected
        }
    }
}

#[cfg(target_os = "macos")]
fn port_number_matches(observed: libc::c_int, expected: u16) -> bool {
    let observed = observed as u16;
    observed == expected || u16::from_be(observed) == expected
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn resolve_listener_owner(addr: IpAddr, port: u16) -> RuntimeResult<ObservedPortOwner> {
    Err(port_unverifiable(format!(
        "endpoint ownership is unsupported on this platform for {addr}:{port}"
    )))
}
