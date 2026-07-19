//! Private authority for endpoint startup coordination and kernel listener proof.
//!
//! The model exposes endpoints, but none of the machinery in this module is a
//! public runtime surface. Lock files are inert rendezvous inodes; kernel locks
//! serialize startup and kernel listener records are the steady-state truth.

#[cfg(test)]
use std::cell::Cell;
use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::io;
use std::net::IpAddr;
#[cfg(test)]
use std::net::{Ipv4Addr, Ipv6Addr};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
#[cfg(target_os = "linux")]
use std::os::unix::fs::MetadataExt;

use nixfied_model::ContainmentRequirement;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::service::process::{
    SelectedEndpoint, process_is_in_containment, process_is_live_with_identity,
};

use super::TrackedProcessIdentity;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

const LOCK_SUFFIX: &str = ".lock";
const STABLE_SNAPSHOT_ATTEMPTS: usize = 3;

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Option<[u8; N]> {
    let end = offset.checked_add(N)?;
    bytes.get(offset..end)?.try_into().ok()
}

#[cfg(test)]
thread_local! {
    static TEST_LOCK_ROOT_FD: Cell<Option<RawFd>> = const { Cell::new(None) };
}

fn address_family(address: IpAddr) -> &'static str {
    match address {
        IpAddr::V4(_) => "ipv4",
        IpAddr::V6(_) => "ipv6",
    }
}

fn address_family_tag(address: IpAddr) -> u8 {
    match address {
        IpAddr::V4(_) => 4,
        IpAddr::V6(_) => 6,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum NetworkScope {
    #[cfg(target_os = "linux")]
    Linux { device: u64, inode: u64 },
    #[cfg(target_os = "macos")]
    Host,
}

impl NetworkScope {
    fn production() -> Result<Self, EndpointFailure> {
        #[cfg(target_os = "linux")]
        {
            let metadata = std::fs::metadata("/proc/self/ns/net").map_err(|error| {
                EndpointFailure::unverifiable(
                    None,
                    format!("failed to stat /proc/self/ns/net: {error}"),
                )
            })?;
            Ok(Self::Linux {
                device: metadata.dev(),
                inode: metadata.ino(),
            })
        }
        #[cfg(target_os = "macos")]
        {
            Ok(Self::Host)
        }
    }

    fn append_bytes(&self, out: &mut Vec<u8>) {
        match self {
            #[cfg(target_os = "linux")]
            Self::Linux { device, inode } => {
                out.extend_from_slice(b"linux\0");
                out.extend_from_slice(&device.to_be_bytes());
                out.extend_from_slice(&inode.to_be_bytes());
            }
            #[cfg(target_os = "macos")]
            Self::Host => out.extend_from_slice(b"host\0"),
        }
    }
}

#[derive(Debug)]
struct EndpointKey {
    filename: String,
    endpoint: SelectedEndpoint,
}

impl EndpointKey {
    fn derive(endpoint: &SelectedEndpoint, scope: &NetworkScope) -> Self {
        let mut encoded = Vec::with_capacity(64);
        encoded.extend_from_slice(b"tcp\0");
        scope.append_bytes(&mut encoded);
        let address = endpoint.host.ip();
        encoded.push(address_family_tag(address));
        match address {
            IpAddr::V4(address) => encoded.extend_from_slice(&address.octets()),
            IpAddr::V6(address) => encoded.extend_from_slice(&address.octets()),
        }
        encoded.extend_from_slice(&endpoint.port.to_be_bytes());
        let filename = format!("{}{}", hex::encode(Sha256::digest(&encoded)), LOCK_SUFFIX);
        Self {
            filename,
            endpoint: endpoint.clone(),
        }
    }
}

#[derive(Debug)]
pub(crate) enum EndpointFailure {
    LockContended {
        endpoint: SelectedEndpoint,
    },
    ListenerOccupied {
        endpoint: SelectedEndpoint,
        listeners: Vec<ListenerRecord>,
    },
    Unverifiable {
        endpoint: Option<SelectedEndpoint>,
        message: String,
    },
}

impl EndpointFailure {
    fn unverifiable(endpoint: Option<SelectedEndpoint>, message: impl Into<String>) -> Self {
        Self::Unverifiable {
            endpoint,
            message: message.into(),
        }
    }
}

/// The complete startup lock set for one service. Dropping it releases every
/// kernel lock; the rendezvous files deliberately remain.
pub(crate) struct EndpointLockGuards {
    _guards: Vec<OwnedFd>,
}

impl EndpointLockGuards {
    pub(crate) fn release(self) {
        drop(self);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self._guards.len()
    }
}

pub(crate) fn acquire_startup_locks<'a>(
    endpoints: impl IntoIterator<Item = &'a SelectedEndpoint>,
) -> Result<EndpointLockGuards, EndpointFailure> {
    let mut endpoints = endpoints.into_iter().peekable();
    if endpoints.peek().is_none() {
        return Ok(EndpointLockGuards {
            _guards: Vec::new(),
        });
    }
    let scope = NetworkScope::production()?;
    let mut keys = endpoints
        .map(|endpoint| EndpointKey::derive(endpoint, &scope))
        .collect::<Vec<_>>();
    keys.sort_by(|left, right| left.filename.cmp(&right.filename));
    let lock_dir = open_lock_directory()?;
    let mut guards = Vec::with_capacity(keys.len());
    for key in keys {
        let fd = open_lock_file(lock_dir.as_raw_fd(), &key)?;
        let result = unsafe { libc::flock(fd.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result != 0 {
            let error = io::Error::last_os_error();
            if error
                .raw_os_error()
                .is_some_and(|code| code == libc::EWOULDBLOCK || code == libc::EAGAIN)
            {
                return Err(EndpointFailure::LockContended {
                    endpoint: key.endpoint,
                });
            }
            return Err(EndpointFailure::unverifiable(
                Some(key.endpoint),
                format!("failed to acquire endpoint startup lock: {error}"),
            ));
        }
        guards.push(fd);
    }
    Ok(EndpointLockGuards { _guards: guards })
}

fn open_lock_directory() -> Result<OwnedFd, EndpointFailure> {
    #[cfg(test)]
    if let Some(root) = duplicate_test_lock_root()? {
        let euid = unsafe { libc::geteuid() };
        validate_directory(
            root.as_raw_fd(),
            euid,
            Some(0o700),
            "injected endpoint lock root",
        )?;
        return create_owned_directory(root.as_raw_fd(), c"endpoint-locks", euid);
    }

    #[cfg(target_os = "linux")]
    const SYSTEM_COMPONENTS: &[&CStr] = &[c"tmp"];
    #[cfg(target_os = "macos")]
    const SYSTEM_COMPONENTS: &[&CStr] = &[c"private", c"tmp"];
    let root = open_path(c"/", libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW)?;
    validate_directory(root.as_raw_fd(), 0, None, "filesystem root")?;
    let mut current = root;
    for (index, component) in SYSTEM_COMPONENTS.iter().enumerate() {
        let next = open_at(
            current.as_raw_fd(),
            component,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
            0,
        )?;
        let is_tmp = index + 1 == SYSTEM_COMPONENTS.len();
        validate_directory(
            next.as_raw_fd(),
            0,
            is_tmp.then_some(libc::S_ISVTX as libc::mode_t),
            if is_tmp {
                "system temporary directory"
            } else {
                "fixed system directory"
            },
        )?;
        current = next;
    }

    let euid = unsafe { libc::geteuid() };
    let user_component = CString::new(format!("nixfied-{euid}")).map_err(|error| {
        EndpointFailure::unverifiable(None, format!("invalid endpoint lock directory: {error}"))
    })?;
    current = create_owned_directory(current.as_raw_fd(), &user_component, euid)?;
    create_owned_directory(current.as_raw_fd(), c"endpoint-locks", euid)
}

#[cfg(test)]
fn duplicate_test_lock_root() -> Result<Option<OwnedFd>, EndpointFailure> {
    TEST_LOCK_ROOT_FD.with(|slot| {
        let Some(fd) = slot.get() else {
            return Ok(None);
        };
        let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
        if duplicate < 0 {
            return Err(EndpointFailure::unverifiable(
                None,
                format!(
                    "failed to duplicate injected endpoint lock root: {}",
                    io::Error::last_os_error()
                ),
            ));
        }
        // SAFETY: F_DUPFD_CLOEXEC returned a new descriptor owned by this scope.
        Ok(Some(unsafe { OwnedFd::from_raw_fd(duplicate) }))
    })
}

#[cfg(test)]
fn with_test_lock_root<T>(root: RawFd, operation: impl FnOnce() -> T) -> T {
    struct Reset(Option<RawFd>);
    impl Drop for Reset {
        fn drop(&mut self) {
            TEST_LOCK_ROOT_FD.with(|slot| slot.set(self.0));
        }
    }
    let previous = TEST_LOCK_ROOT_FD.with(|slot| slot.replace(Some(root)));
    let _reset = Reset(previous);
    operation()
}

fn create_owned_directory(
    parent: RawFd,
    name: &CStr,
    euid: libc::uid_t,
) -> Result<OwnedFd, EndpointFailure> {
    let result = unsafe { libc::mkdirat(parent, name.as_ptr(), 0o700) };
    if result != 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EEXIST) {
            return Err(EndpointFailure::unverifiable(
                None,
                format!("failed to create endpoint lock directory: {error}"),
            ));
        }
    }
    let fd = open_at(
        parent,
        name,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        0,
    )?;
    validate_directory(fd.as_raw_fd(), euid, Some(0o700), "endpoint lock directory")?;
    Ok(fd)
}

fn open_lock_file(parent: RawFd, key: &EndpointKey) -> Result<OwnedFd, EndpointFailure> {
    let name = CString::new(key.filename.as_str()).map_err(|error| {
        EndpointFailure::unverifiable(
            Some(key.endpoint.clone()),
            format!("invalid endpoint lock filename: {error}"),
        )
    })?;
    let create_flags =
        libc::O_CREAT | libc::O_EXCL | libc::O_RDWR | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    let fd = match open_at_raw(parent, &name, create_flags, 0o600) {
        Ok(fd) => fd,
        Err(error) if error.raw_os_error() == Some(libc::EEXIST) => open_at_raw(
            parent,
            &name,
            libc::O_RDWR | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0,
        )
        .map_err(|error| {
            EndpointFailure::unverifiable(
                Some(key.endpoint.clone()),
                format!("failed to open existing endpoint lock target: {error}"),
            )
        })?,
        Err(error) => {
            return Err(EndpointFailure::unverifiable(
                Some(key.endpoint.clone()),
                format!("failed to create endpoint lock target: {error}"),
            ));
        }
    };
    let stat = stat_fd(fd.as_raw_fd())
        .map_err(|message| EndpointFailure::unverifiable(Some(key.endpoint.clone()), message))?;
    let kind = stat.st_mode & libc::S_IFMT;
    let mode = stat.st_mode & 0o7777;
    let euid = unsafe { libc::geteuid() };
    if kind != libc::S_IFREG || stat.st_uid != euid || mode != 0o600 {
        return Err(EndpointFailure::unverifiable(
            Some(key.endpoint.clone()),
            "endpoint lock target must be a regular effective-user-owned 0600 file",
        ));
    }
    Ok(fd)
}

fn open_path(path: &CStr, flags: libc::c_int) -> Result<OwnedFd, EndpointFailure> {
    let fd = unsafe { libc::open(path.as_ptr(), flags | libc::O_CLOEXEC) };
    owned_fd(fd, "open fixed endpoint coordination path")
}

fn open_at(
    parent: RawFd,
    name: &CStr,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> Result<OwnedFd, EndpointFailure> {
    open_at_raw(parent, name, flags | libc::O_CLOEXEC, mode).map_err(|error| {
        EndpointFailure::unverifiable(
            None,
            format!("failed to open endpoint coordination component: {error}"),
        )
    })
}

fn open_at_raw(
    parent: RawFd,
    name: &CStr,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> io::Result<OwnedFd> {
    // `mode_t` is narrower than C's default variadic integer type on macOS.
    let fd = unsafe { libc::openat(parent, name.as_ptr(), flags, mode as libc::c_uint) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a successful openat returned a new descriptor now owned here.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn owned_fd(fd: libc::c_int, action: &str) -> Result<OwnedFd, EndpointFailure> {
    if fd < 0 {
        return Err(EndpointFailure::unverifiable(
            None,
            format!("failed to {action}: {}", io::Error::last_os_error()),
        ));
    }
    // SAFETY: a successful open/openat returned a new descriptor now owned here.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn stat_fd(fd: RawFd) -> Result<libc::stat, String> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::zeroed();
    if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
        return Err(format!(
            "failed to inspect endpoint coordination descriptor: {}",
            io::Error::last_os_error()
        ));
    }
    // SAFETY: fstat succeeded and initialized the complete struct.
    Ok(unsafe { stat.assume_init() })
}

fn validate_directory(
    fd: RawFd,
    expected_uid: libc::uid_t,
    expected_mode_or_flag: Option<libc::mode_t>,
    label: &str,
) -> Result<(), EndpointFailure> {
    let stat = stat_fd(fd).map_err(|message| EndpointFailure::unverifiable(None, message))?;
    if stat.st_mode & libc::S_IFMT != libc::S_IFDIR || stat.st_uid != expected_uid {
        return Err(EndpointFailure::unverifiable(
            None,
            format!("{label} must be a directory owned by uid {expected_uid}"),
        ));
    }
    if let Some(expected) = expected_mode_or_flag {
        let mode = stat.st_mode & 0o7777;
        let valid = if expected == libc::S_ISVTX as libc::mode_t {
            mode & expected != 0
        } else {
            mode == expected
        };
        if !valid {
            return Err(EndpointFailure::unverifiable(
                None,
                format!("{label} has unsafe permissions {mode:o}"),
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "platform", rename_all = "kebab-case")]
pub(crate) enum KernelSocketIdentity {
    #[cfg(target_os = "linux")]
    Linux { inode: u32, cookie: [u32; 2] },
    #[cfg(target_os = "macos")]
    Macos {
        pcb: u64,
        pcb_generation: u64,
        socket: u64,
        socket_generation: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ListenerIdentity {
    pub(crate) address: IpAddr,
    pub(crate) port: u16,
    pub(crate) kernel: KernelSocketIdentity,
    pub(crate) uid: u32,
    pub(crate) ipv6_only: Option<bool>,
}

impl Serialize for ListenerIdentity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Borrowed<'a> {
            family: &'static str,
            address: IpAddr,
            port: u16,
            kernel: &'a KernelSocketIdentity,
            uid: u32,
            ipv6_only: Option<bool>,
        }

        Borrowed {
            family: address_family(self.address),
            address: self.address,
            port: self.port,
            kernel: &self.kernel,
            uid: self.uid,
            ipv6_only: self.ipv6_only,
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListenerHolder {
    pub(crate) pid: u32,
    pub(crate) pgid: i32,
    pub(crate) platform_start: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListenerRecord {
    pub(crate) identity: ListenerIdentity,
    pub(crate) holders: Vec<ListenerHolder>,
    #[serde(skip)]
    pub(crate) pid_hints: Vec<u32>,
}

pub(crate) fn conflicts(endpoint: &SelectedEndpoint, observed: &ListenerRecord) -> bool {
    if endpoint.port != observed.identity.port {
        return false;
    }
    match (endpoint.host.ip(), observed.identity.address) {
        (IpAddr::V4(planned), IpAddr::V4(observed)) => {
            observed.is_unspecified() || observed == planned
        }
        (IpAddr::V4(planned), IpAddr::V6(observed_address)) => {
            if observed.identity.ipv6_only != Some(false) {
                return false;
            }
            observed_address.is_unspecified()
                || observed_address
                    .to_ipv4_mapped()
                    .is_some_and(|mapped| mapped.is_unspecified() || mapped == planned)
        }
        (IpAddr::V6(planned), IpAddr::V6(observed)) => {
            observed.is_unspecified() || observed == planned
        }
        (IpAddr::V6(_), IpAddr::V4(_)) => false,
    }
}

pub(crate) fn satisfies_declared_endpoint(
    endpoint: &SelectedEndpoint,
    observed: &ListenerRecord,
) -> bool {
    endpoint.port == observed.identity.port && endpoint.host.ip() == observed.identity.address
}

pub(crate) struct EndpointOwnership<'a> {
    pub(crate) endpoint: &'a SelectedEndpoint,
    pub(crate) listeners: Vec<ListenerRecord>,
}

impl Serialize for EndpointOwnership<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Borrowed<'a> {
            endpoint_id: &'a str,
            address: &'a nixfied_model::LoopbackHost,
            port: u16,
            listeners: &'a [ListenerRecord],
        }

        Borrowed {
            endpoint_id: &self.endpoint.endpoint_id,
            address: &self.endpoint.host,
            port: self.endpoint.port,
            listeners: &self.listeners,
        }
        .serialize(serializer)
    }
}

pub(crate) enum OwnershipObservation<'a> {
    Complete(Vec<EndpointOwnership<'a>>),
    Missing(&'a SelectedEndpoint),
    Outside {
        endpoint: &'a SelectedEndpoint,
        listeners: Vec<ListenerRecord>,
    },
    Unverifiable {
        endpoint: Option<&'a SelectedEndpoint>,
        message: String,
    },
    ContainmentUnconfirmed {
        message: String,
    },
}

pub(crate) struct ExpectedOwner<'a> {
    pub(crate) pid: u32,
    pub(crate) pgid: i32,
    pub(crate) platform_start: Option<&'a str>,
    pub(crate) containment: ContainmentRequirement,
    pub(crate) tracked_processes: &'a [TrackedProcessIdentity],
}

pub(crate) fn observe_ownership<'a>(
    endpoints: &'a BTreeMap<String, SelectedEndpoint>,
    expected: &ExpectedOwner<'_>,
) -> OwnershipObservation<'a> {
    let endpoints = endpoints.values().collect::<Vec<_>>();
    observe_ownership_inner(&endpoints, expected, true)
}

/// Re-observe after the foreground child is known to have exited. The recorded
/// PID/PGID/start identity still defines its containment, so a listener held by
/// a visible process outside that containment can truthfully override the
/// weaker early-exit error with PORT_CONFLICT. A listener left inside the old
/// containment does not become an outside conflict; the caller preserves
/// PROC_ESCAPE and terminates the complete tracked tree.
pub(crate) fn observe_ownership_after_primary_exit<'a>(
    endpoints: &'a BTreeMap<String, SelectedEndpoint>,
    expected: &ExpectedOwner<'_>,
) -> OwnershipObservation<'a> {
    let endpoints = endpoints.values().collect::<Vec<_>>();
    observe_ownership_inner(&endpoints, expected, false)
}

pub(crate) fn observe_single_ownership<'a>(
    endpoint: &'a SelectedEndpoint,
    expected: &ExpectedOwner<'_>,
) -> OwnershipObservation<'a> {
    observe_ownership_inner(&[endpoint], expected, true)
}

fn observe_ownership_inner<'a>(
    endpoints: &[&'a SelectedEndpoint],
    expected: &ExpectedOwner<'_>,
    require_primary_live: bool,
) -> OwnershipObservation<'a> {
    if endpoints.is_empty() {
        return OwnershipObservation::Complete(Vec::new());
    }
    let Some(expected_start) = expected.platform_start else {
        return OwnershipObservation::ContainmentUnconfirmed {
            message: format!(
                "tracked process {} has no live start identity",
                expected.pid
            ),
        };
    };
    if require_primary_live {
        match process_is_live_with_identity(expected.pid, expected.pgid, Some(expected_start)) {
            Ok(true) => {}
            Ok(false) => {
                return OwnershipObservation::ContainmentUnconfirmed {
                    message: format!(
                        "tracked process {} no longer matches its recorded containment identity",
                        expected.pid
                    ),
                };
            }
            Err(error) => {
                return OwnershipObservation::ContainmentUnconfirmed {
                    message: error.message,
                };
            }
        }
    }
    let listeners = match stable_matching_snapshot(endpoints, true) {
        Ok(listeners) => listeners,
        Err(message) => {
            return OwnershipObservation::Unverifiable {
                endpoint: None,
                message,
            };
        }
    };
    let euid = unsafe { libc::geteuid() } as u32;
    let mut ownership = Vec::with_capacity(endpoints.len());
    for endpoint in endpoints.iter().copied() {
        let matches = listeners
            .iter()
            .filter(|listener| conflicts(endpoint, listener))
            .cloned()
            .collect::<Vec<_>>();
        for listener in &matches {
            if listener.identity.uid != euid {
                return OwnershipObservation::Outside {
                    endpoint,
                    listeners: matches,
                };
            }
            if listener.holders.is_empty() {
                return OwnershipObservation::Unverifiable {
                    endpoint: Some(endpoint),
                    message: format!(
                        "listener record for {}:{} could not be correlated to a live fd holder",
                        listener.identity.address, listener.identity.port
                    ),
                };
            }
            for holder in &listener.holders {
                if holder.platform_start.is_none() {
                    return OwnershipObservation::Unverifiable {
                        endpoint: Some(endpoint),
                        message: format!(
                            "listener holder pid {} has no live start identity",
                            holder.pid
                        ),
                    };
                }
                if !require_primary_live {
                    if holder.pid == expected.pid {
                        if holder.platform_start.as_deref() == Some(expected_start) {
                            continue;
                        }
                        return OwnershipObservation::Outside {
                            endpoint,
                            listeners: matches,
                        };
                    }
                    if let Some(monitored) = expected
                        .tracked_processes
                        .iter()
                        .find(|process| process.pid == holder.pid)
                    {
                        let Some(monitored_start) = monitored.platform_start.as_deref() else {
                            return OwnershipObservation::ContainmentUnconfirmed {
                                message: format!(
                                    "tracked descendant {} has no recorded start identity",
                                    monitored.pid
                                ),
                            };
                        };
                        if holder.platform_start.as_deref() == Some(monitored_start) {
                            continue;
                        }
                        return OwnershipObservation::Outside {
                            endpoint,
                            listeners: matches,
                        };
                    }
                }
                match process_is_in_containment(
                    expected.pid,
                    expected.pgid,
                    &expected.containment,
                    holder.pid,
                    holder.pgid,
                ) {
                    Ok(true) => {}
                    Ok(false) => {
                        return OwnershipObservation::Outside {
                            endpoint,
                            listeners: matches,
                        };
                    }
                    Err(error) => {
                        return OwnershipObservation::ContainmentUnconfirmed {
                            message: error.message,
                        };
                    }
                }
            }
        }
        if !matches
            .iter()
            .any(|listener| satisfies_declared_endpoint(endpoint, listener))
        {
            return OwnershipObservation::Missing(endpoint);
        }
        ownership.push(EndpointOwnership {
            endpoint,
            listeners: matches,
        });
    }
    OwnershipObservation::Complete(ownership)
}

pub(crate) fn preflight<'a>(
    endpoints: impl IntoIterator<Item = &'a SelectedEndpoint>,
) -> Result<(), EndpointFailure> {
    let endpoints = endpoints.into_iter().collect::<Vec<_>>();
    if endpoints.is_empty() {
        return Ok(());
    }
    // Observer support is part of admission to service mutation even when the
    // point-in-time bind succeeds.
    platform_snapshot().map_err(|message| EndpointFailure::unverifiable(None, message))?;
    for endpoint in endpoints {
        match bind_exact(endpoint) {
            Ok(BindResult::Available) => {}
            Ok(BindResult::AddressInUse) => {
                let listeners =
                    stable_matching_snapshot(&[endpoint], false).map_err(|message| {
                        EndpointFailure::unverifiable(Some(endpoint.clone()), message)
                    })?;
                if listeners.is_empty() {
                    return Err(EndpointFailure::unverifiable(
                        Some(endpoint.clone()),
                        format!(
                            "bind reported address in use for {}:{} without an observable listener",
                            endpoint.host, endpoint.port
                        ),
                    ));
                }
                return Err(EndpointFailure::ListenerOccupied {
                    endpoint: endpoint.clone(),
                    listeners,
                });
            }
            Err(message) => {
                return Err(EndpointFailure::unverifiable(
                    Some(endpoint.clone()),
                    message,
                ));
            }
        }
    }
    Ok(())
}

fn stable_matching_snapshot(
    endpoints: &[&SelectedEndpoint],
    correlation_required: bool,
) -> Result<Vec<ListenerRecord>, String> {
    let mut last_churn = None;
    for _ in 0..STABLE_SNAPSHOT_ATTEMPTS {
        let mut before = matching_snapshot(endpoints)?;
        let correlation = platform_correlate(&mut before);
        if correlation_required {
            correlation?;
        } else if correlation.is_err() {
            for listener in &mut before {
                listener.holders.clear();
            }
        }
        let after = matching_snapshot(endpoints)?;
        let before_ids = before
            .iter()
            .map(|record| record.identity.clone())
            .collect::<Vec<_>>();
        let after_ids = after
            .iter()
            .map(|record| record.identity.clone())
            .collect::<Vec<_>>();
        if before_ids == after_ids {
            return Ok(before);
        }
        last_churn = Some(format!(
            "listener snapshot changed during holder correlation ({before_ids:?} -> {after_ids:?})"
        ));
    }
    Err(last_churn.unwrap_or_else(|| "listener snapshot was unstable".to_string()))
}

fn matching_snapshot(endpoints: &[&SelectedEndpoint]) -> Result<Vec<ListenerRecord>, String> {
    Ok(platform_snapshot()?
        .into_iter()
        .filter(|listener| {
            endpoints
                .iter()
                .any(|endpoint| conflicts(endpoint, listener))
        })
        .collect())
}

fn platform_snapshot() -> Result<Vec<ListenerRecord>, String> {
    #[cfg(target_os = "linux")]
    {
        linux::snapshot()
    }
    #[cfg(target_os = "macos")]
    {
        macos::snapshot()
    }
}

fn platform_correlate(records: &mut [ListenerRecord]) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        linux::correlate(records)
    }
    #[cfg(target_os = "macos")]
    {
        macos::correlate(records)
    }
}

enum BindResult {
    Available,
    AddressInUse,
}

fn bind_exact(endpoint: &SelectedEndpoint) -> Result<BindResult, String> {
    let domain = match endpoint.host.ip() {
        IpAddr::V4(_) => libc::AF_INET,
        IpAddr::V6(_) => libc::AF_INET6,
    };
    #[cfg(target_os = "linux")]
    let socket_type = libc::SOCK_STREAM | libc::SOCK_CLOEXEC;
    #[cfg(not(target_os = "linux"))]
    let socket_type = libc::SOCK_STREAM;
    let raw = unsafe { libc::socket(domain, socket_type, libc::IPPROTO_TCP) };
    if raw < 0 {
        return Err(format!(
            "failed to create endpoint preflight socket: {}",
            io::Error::last_os_error()
        ));
    }
    // SAFETY: socket returned a new descriptor owned by this scope.
    let socket = unsafe { OwnedFd::from_raw_fd(raw) };
    #[cfg(not(target_os = "linux"))]
    set_cloexec(socket.as_raw_fd())?;
    let result = match endpoint.host.ip() {
        IpAddr::V4(address) => {
            // SAFETY: zero is a valid initial representation; every field bind
            // consumes is initialized below.
            let mut sockaddr = unsafe { std::mem::zeroed::<libc::sockaddr_in>() };
            #[cfg(target_os = "macos")]
            {
                sockaddr.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
            }
            sockaddr.sin_family = libc::AF_INET as libc::sa_family_t;
            sockaddr.sin_port = endpoint.port.to_be();
            sockaddr.sin_addr = libc::in_addr {
                s_addr: u32::from_ne_bytes(address.octets()),
            };
            unsafe {
                libc::bind(
                    socket.as_raw_fd(),
                    (&sockaddr as *const libc::sockaddr_in).cast(),
                    std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
                )
            }
        }
        IpAddr::V6(address) => {
            // SAFETY: zero is a valid initial representation; every field bind
            // consumes is initialized below.
            let mut sockaddr = unsafe { std::mem::zeroed::<libc::sockaddr_in6>() };
            #[cfg(target_os = "macos")]
            {
                sockaddr.sin6_len = std::mem::size_of::<libc::sockaddr_in6>() as u8;
            }
            sockaddr.sin6_family = libc::AF_INET6 as libc::sa_family_t;
            sockaddr.sin6_port = endpoint.port.to_be();
            sockaddr.sin6_addr = libc::in6_addr {
                s6_addr: address.octets(),
            };
            unsafe {
                libc::bind(
                    socket.as_raw_fd(),
                    (&sockaddr as *const libc::sockaddr_in6).cast(),
                    std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
                )
            }
        }
    };
    if result == 0 {
        Ok(BindResult::Available)
    } else {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EADDRINUSE) {
            Ok(BindResult::AddressInUse)
        } else {
            Err(format!(
                "failed to bind endpoint preflight socket at {}:{}: {error}",
                endpoint.host, endpoint.port
            ))
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn set_cloexec(fd: RawFd) -> Result<(), String> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(format!(
            "failed to mark endpoint socket close-on-exec: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{File, OpenOptions};
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_ROOT: AtomicU64 = AtomicU64::new(1);

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "nixfied-endpoint-test-{}-{}",
                std::process::id(),
                NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            Self(path)
        }

        fn open(&self) -> File {
            File::open(&self.0).unwrap()
        }

        fn locks(&self) -> PathBuf {
            self.0.join("endpoint-locks")
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn with_root<T>(root: &TestRoot, operation: impl FnOnce() -> T) -> T {
        let fd = root.open();
        with_test_lock_root(fd.as_raw_fd(), operation)
    }

    fn assert_unverifiable(result: Result<EndpointLockGuards, EndpointFailure>) {
        assert!(matches!(result, Err(EndpointFailure::Unverifiable { .. })));
    }

    fn endpoint(address: &str, port: u16) -> SelectedEndpoint {
        SelectedEndpoint {
            endpoint_id: "test".to_string(),
            host: nixfied_model::LoopbackHost::parse(address).unwrap(),
            port,
        }
    }

    fn listener(address: IpAddr, port: u16, ipv6_only: Option<bool>) -> ListenerRecord {
        ListenerRecord {
            identity: ListenerIdentity {
                address,
                port,
                kernel: test_kernel_identity(),
                uid: unsafe { libc::geteuid() } as u32,
                ipv6_only,
            },
            holders: Vec::new(),
            pid_hints: Vec::new(),
        }
    }

    fn test_kernel_identity() -> KernelSocketIdentity {
        #[cfg(target_os = "linux")]
        {
            KernelSocketIdentity::Linux {
                inode: 1,
                cookie: [2, 3],
            }
        }
        #[cfg(target_os = "macos")]
        {
            KernelSocketIdentity::Macos {
                pcb: 1,
                pcb_generation: 2,
                socket: 3,
                socket_generation: 4,
            }
        }
    }

    #[test]
    fn wildcard_collision_is_not_exact_satisfaction() {
        let planned = endpoint("127.0.0.1", 23080);
        let wildcard = listener(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 23080, None);
        assert!(conflicts(&planned, &wildcard));
        assert!(!satisfies_declared_endpoint(&planned, &wildcard));
    }

    #[test]
    fn exact_and_wildcard_relations_cover_ipv4_ipv6_and_mapped_ipv4() {
        let v4 = endpoint("127.0.0.1", 23080);
        let exact_v4 = listener(IpAddr::V4(Ipv4Addr::LOCALHOST), 23080, None);
        assert!(conflicts(&v4, &exact_v4));
        assert!(satisfies_declared_endpoint(&v4, &exact_v4));

        let v6 = endpoint("::1", 23080);
        let exact_v6 = listener(IpAddr::V6(Ipv6Addr::LOCALHOST), 23080, Some(true));
        let wildcard_v6 = listener(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 23080, Some(true));
        assert!(conflicts(&v6, &exact_v6));
        assert!(satisfies_declared_endpoint(&v6, &exact_v6));
        assert!(conflicts(&v6, &wildcard_v6));
        assert!(!satisfies_declared_endpoint(&v6, &wildcard_v6));

        let mapped = listener(
            IpAddr::V6(Ipv4Addr::LOCALHOST.to_ipv6_mapped()),
            23080,
            Some(false),
        );
        assert!(conflicts(&v4, &mapped));
        assert!(!satisfies_declared_endpoint(&v4, &mapped));
        let mapped_wildcard = listener(
            IpAddr::V6(Ipv4Addr::UNSPECIFIED.to_ipv6_mapped()),
            23080,
            Some(false),
        );
        assert!(conflicts(&v4, &mapped_wildcard));
        assert!(!satisfies_declared_endpoint(&v4, &mapped_wildcard));
        let mapped_v6_only = listener(
            IpAddr::V6(Ipv4Addr::LOCALHOST.to_ipv6_mapped()),
            23080,
            Some(true),
        );
        assert!(!conflicts(&v4, &mapped_v6_only));
        assert!(!conflicts(
            &v4,
            &listener(IpAddr::V4(Ipv4Addr::LOCALHOST), 23081, None)
        ));
    }

    #[test]
    fn co_bound_exact_and_wildcard_records_are_distinct_proof_units() {
        let planned = endpoint("127.0.0.1", 23080);
        let records = [
            listener(IpAddr::V4(Ipv4Addr::LOCALHOST), 23080, None),
            listener(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 23080, None),
        ];
        assert_eq!(
            records
                .iter()
                .filter(|record| conflicts(&planned, record))
                .count(),
            2
        );
        assert_eq!(
            records
                .iter()
                .filter(|record| satisfies_declared_endpoint(&planned, record))
                .count(),
            1
        );
    }

    #[test]
    fn ipv6_dual_stack_overlap_is_explicit() {
        let planned = endpoint("127.0.0.1", 23080);
        let dual = listener(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 23080, Some(false));
        let v6_only = listener(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 23080, Some(true));
        assert!(conflicts(&planned, &dual));
        assert!(!conflicts(&planned, &v6_only));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_endpoint_key_uses_the_host_scope() {
        assert_eq!(NetworkScope::production().unwrap(), NetworkScope::Host);
        let key = EndpointKey::derive(&endpoint("127.0.0.1", 23080), &NetworkScope::Host);
        assert_eq!(
            key.filename,
            "bec0f5812199b9f16f3c4bcac20d074711ee127f5d9f28f692e470e972e8fc6e.lock"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn endpoint_key_digest_is_stable() {
        let scope = NetworkScope::Linux {
            device: 0x0102_0304_0506_0708,
            inode: 0x1112_1314_1516_1718,
        };
        let key = EndpointKey::derive(&endpoint("127.0.0.1", 23080), &scope);
        assert_eq!(
            key.filename,
            "e57a1586478a9da5cbd9deb535ba9cb28fda348d8855c6e47979f68f9c3f2ef0.lock"
        );
        let other = EndpointKey::derive(
            &endpoint("127.0.0.1", 23080),
            &NetworkScope::Linux {
                device: 0x0102_0304_0506_0708,
                inode: 0x1112_1314_1516_1719,
            },
        );
        assert_ne!(key.filename, other.filename);
    }

    #[test]
    fn ownership_evidence_projects_the_selected_endpoint_as_an_address() {
        let endpoint = endpoint("127.0.0.1", 23080);
        let ownership = EndpointOwnership {
            endpoint: &endpoint,
            listeners: vec![listener(IpAddr::V4(Ipv4Addr::LOCALHOST), 23080, None)],
        };
        let value = serde_json::to_value(ownership).unwrap();
        assert_eq!(value["endpointId"], "test");
        assert_eq!(value["address"], "127.0.0.1");
        assert_eq!(value["port"], 23080);
        assert_eq!(value["listeners"][0]["identity"]["family"], "ipv4");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn production_network_scope_comes_from_proc_namespace_stat() {
        let metadata = std::fs::metadata("/proc/self/ns/net").unwrap();
        assert_eq!(
            NetworkScope::production().unwrap(),
            NetworkScope::Linux {
                device: metadata.dev(),
                inode: metadata.ino(),
            }
        );
    }

    #[test]
    fn endpoint_lock_is_nonblocking_and_reuses_the_same_inode() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let endpoint = endpoint("127.0.0.1", port);
        let first = acquire_startup_locks(std::slice::from_ref(&endpoint)).unwrap();
        assert_eq!(first.len(), 1);
        assert!(matches!(
            acquire_startup_locks(std::slice::from_ref(&endpoint)),
            Err(EndpointFailure::LockContended { .. })
        ));
        drop(first);
        assert_eq!(
            acquire_startup_locks(std::slice::from_ref(&endpoint))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn raw_bind_distinguishes_available_from_listening_address_in_use() {
        let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let planned = endpoint("127.0.0.1", held.local_addr().unwrap().port());
        assert!(matches!(
            bind_exact(&planned).unwrap(),
            BindResult::AddressInUse
        ));
        drop(held);
        assert!(matches!(
            bind_exact(&planned).unwrap(),
            BindResult::Available
        ));
    }

    #[test]
    fn occupied_bind_with_complete_empty_listener_snapshot_is_unverifiable() {
        let raw = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
        assert!(
            raw >= 0,
            "test socket should open: {}",
            io::Error::last_os_error()
        );
        // SAFETY: `socket` returned a new descriptor owned by this test.
        let held = unsafe { OwnedFd::from_raw_fd(raw) };
        let mut address: libc::sockaddr_in = unsafe { std::mem::zeroed() };
        #[cfg(target_os = "macos")]
        {
            address.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
        }
        address.sin_family = libc::AF_INET as _;
        address.sin_port = 0;
        address.sin_addr.s_addr = u32::from_ne_bytes(Ipv4Addr::LOCALHOST.octets());
        let bound = unsafe {
            libc::bind(
                held.as_raw_fd(),
                (&raw const address).cast(),
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            )
        };
        assert_eq!(
            bound,
            0,
            "test socket should bind: {}",
            io::Error::last_os_error()
        );
        let mut selected: libc::sockaddr_in = unsafe { std::mem::zeroed() };
        let mut selected_len = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
        let named = unsafe {
            libc::getsockname(
                held.as_raw_fd(),
                (&raw mut selected).cast(),
                &mut selected_len,
            )
        };
        assert_eq!(named, 0, "test socket name should read");
        let planned = endpoint("127.0.0.1", u16::from_be(selected.sin_port));

        assert!(matches!(
            preflight(std::slice::from_ref(&planned)),
            Err(EndpointFailure::Unverifiable {
                endpoint: Some(endpoint),
                ..
            }) if endpoint == planned
        ));
    }

    #[test]
    fn live_listener_correlates_to_the_expected_current_process() {
        let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let planned = endpoint("127.0.0.1", held.local_addr().unwrap().port());
        let pid = std::process::id();
        let pgid = crate::service::process::process_group(pid)
            .unwrap()
            .expect("test process is live");
        let start = crate::service::process::platform_start_identity(pid)
            .expect("test process has a start identity");

        let endpoints = BTreeMap::from([(planned.endpoint_id.clone(), planned.clone())]);
        let observation = observe_ownership(
            &endpoints,
            &ExpectedOwner {
                pid,
                pgid,
                platform_start: Some(&start),
                containment: ContainmentRequirement::ProcessGroup,
                tracked_processes: &[],
            },
        );

        let OwnershipObservation::Complete(ownership) = observation else {
            panic!("current-process listener should be complete ownership evidence")
        };
        assert_eq!(ownership.len(), 1);
        assert!(ownership[0].listeners.iter().all(|record| {
            record
                .holders
                .iter()
                .any(|holder| holder.pid == std::process::id())
        }));
    }

    #[test]
    fn endpoint_id_and_declared_values_do_not_enter_the_lock_filename() {
        let scope = NetworkScope::production().unwrap();
        let mut public = endpoint("127.0.0.1", 23100);
        public.endpoint_id = "public".to_string();
        let mut secret_named = endpoint("127.0.0.1", 23100);
        secret_named.endpoint_id = "SECRET_TOKEN_and_ENV_VALUE_and_arbitrary-command".to_string();
        let public_key = EndpointKey::derive(&public, &scope);
        let secret_key = EndpointKey::derive(&secret_named, &scope);
        assert_eq!(public_key.filename, secret_key.filename);
        assert!(!secret_key.filename.contains("SECRET"));
        assert!(!secret_key.filename.contains("ENV_VALUE"));
        assert!(!secret_key.filename.contains("command"));
    }

    #[test]
    fn injected_lock_root_rejects_symlink_mode_and_nonregular_targets() {
        let planned = endpoint("127.0.0.1", 23101);

        let symlink_root = TestRoot::new();
        let target = symlink_root.0.join("target");
        std::fs::create_dir(&target).unwrap();
        symlink(&target, symlink_root.locks()).unwrap();
        with_root(&symlink_root, || {
            assert_unverifiable(acquire_startup_locks(std::slice::from_ref(&planned)));
        });

        let mode_root = TestRoot::new();
        std::fs::create_dir(mode_root.locks()).unwrap();
        std::fs::set_permissions(mode_root.locks(), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        with_root(&mode_root, || {
            assert_unverifiable(acquire_startup_locks(std::slice::from_ref(&planned)));
        });

        let target_root = TestRoot::new();
        std::fs::create_dir(target_root.locks()).unwrap();
        std::fs::set_permissions(target_root.locks(), std::fs::Permissions::from_mode(0o700))
            .unwrap();
        let key = EndpointKey::derive(&planned, &NetworkScope::production().unwrap());
        std::fs::create_dir(target_root.locks().join(key.filename)).unwrap();
        with_root(&target_root, || {
            assert_unverifiable(acquire_startup_locks(std::slice::from_ref(&planned)));
        });
    }

    #[test]
    fn multi_lock_failure_releases_the_partial_set_and_fds_are_cloexec() {
        let root = TestRoot::new();
        with_root(&root, || {
            let scope = NetworkScope::production().unwrap();
            let mut endpoints = [endpoint("127.0.0.1", 23111), endpoint("127.0.0.1", 23112)];
            endpoints.sort_by_key(|endpoint| EndpointKey::derive(endpoint, &scope).filename);
            let contended = acquire_startup_locks(std::slice::from_ref(&endpoints[1])).unwrap();
            assert!(matches!(
                acquire_startup_locks(&endpoints),
                Err(EndpointFailure::LockContended { .. })
            ));
            let partial_was_released =
                acquire_startup_locks(std::slice::from_ref(&endpoints[0])).unwrap();
            for guard in &partial_was_released._guards {
                let flags = unsafe { libc::fcntl(guard.as_raw_fd(), libc::F_GETFD) };
                assert!(flags >= 0 && flags & libc::FD_CLOEXEC != 0);
            }
            drop(partial_was_released);
            drop(contended);
        });
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rendezvous_file_is_empty_stable_and_never_unlinked() {
        let root = TestRoot::new();
        let planned = endpoint("127.0.0.1", 23121);
        let key = EndpointKey::derive(&planned, &NetworkScope::production().unwrap());
        with_root(&root, || {
            let guard = acquire_startup_locks(std::slice::from_ref(&planned)).unwrap();
            let path = root.locks().join(&key.filename);
            let first = std::fs::metadata(&path).unwrap();
            assert_eq!(first.len(), 0);
            assert_eq!(first.permissions().mode() & 0o777, 0o600);
            assert_eq!(key.filename.len(), 64 + LOCK_SUFFIX.len());
            assert!(
                key.filename[..64]
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
            );
            drop(guard);
            assert!(path.exists());
            let second = std::fs::metadata(&path).unwrap();
            assert_eq!(first.ino(), second.ino());
            let again = acquire_startup_locks(std::slice::from_ref(&planned)).unwrap();
            drop(again);
            assert_eq!(std::fs::metadata(path).unwrap().len(), 0);
        });
    }

    #[test]
    fn injected_root_itself_must_be_exact_mode() {
        let root = TestRoot::new();
        std::fs::set_permissions(&root.0, std::fs::Permissions::from_mode(0o755)).unwrap();
        with_root(&root, || {
            assert_unverifiable(acquire_startup_locks(&[endpoint("127.0.0.1", 23131)]));
        });
    }

    #[test]
    fn existing_lock_target_must_remain_exact_mode() {
        let root = TestRoot::new();
        let planned = endpoint("127.0.0.1", 23141);
        let key = EndpointKey::derive(&planned, &NetworkScope::production().unwrap());
        std::fs::create_dir(root.locks()).unwrap();
        std::fs::set_permissions(root.locks(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = root.locks().join(key.filename);
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o644)
            .open(&path)
            .unwrap();
        with_root(&root, || {
            assert_unverifiable(acquire_startup_locks(std::slice::from_ref(&planned)));
        });
        assert!(Path::new(&path).exists());
    }

    #[test]
    fn unsafe_lock_root_fails_the_real_start_path_before_prepare() {
        use crate::admission::secrets::ResolvedSecrets;
        use crate::cancellation::CancellationToken;
        use crate::registry::{Registry, RegistryIdentity};
        use crate::service::process::{ServiceSelection, start_service_for_slot};
        use crate::service::record_run_created;
        use crate::slot::select_slot;
        use crate::state::{derive_host_placement_for_slot, materialize_run_roots};
        use crate::{Admission, AdmittedSource, ErrorCode};
        use nixfied_model::fixtures::{SyntheticModelOptions, synthetic_model};
        use nixfied_model::{DirtyPolicy, Model, ServiceLifetime, SourceMode, Validate};
        use serde_json::json;

        let unsafe_root = TestRoot::new();
        let symlink_target = unsafe_root.0.join("attacker-controlled");
        std::fs::create_dir(&symlink_target).unwrap();
        symlink(&symlink_target, unsafe_root.locks()).unwrap();

        let workspace = TestRoot::new();
        let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = held.local_addr().unwrap().port();
        drop(held);
        let mut value = synthetic_model(&SyntheticModelOptions {
            executable: "/bin/sleep".to_string(),
            start_args: vec!["30".to_string()],
            port_start: port,
            port_end: port,
            ..SyntheticModelOptions::default()
        });
        value["services"]["synthetic"]["lifecycle"]["prepare"] = json!({ "task": "smoke" });
        value["tasks"]["smoke"]["requires"] = json!([]);
        value["tasks"]["smoke"]["servicesRequired"] = json!([]);
        value["tasks"]["smoke"]["invocation"]["run"] = json!(["sleep", "0"]);
        let model: Model = serde_json::from_value(value).unwrap();
        model.validate().unwrap();
        let selected = select_slot(&model, None).unwrap();
        let placement =
            derive_host_placement_for_slot(&model, &selected, "run-unsafe-lock-root", &workspace.0)
                .unwrap();
        materialize_run_roots(&placement).unwrap();
        let mut registry = Registry::open_or_create(
            placement.registry_path(),
            &RegistryIdentity::for_slot(
                &model.project.project_id,
                selected.environment,
                selected.slot,
                &model.runtime_abi,
                &model.toolchain_id,
            ),
        )
        .unwrap();
        let admission = Admission {
            model_path: workspace.0.join("model.json"),
            computed_model_hash: "unsafe-root-test-hash".to_string(),
            raw_len: 1,
            project_id: model.project.project_id.clone(),
            runtime_abi: model.runtime_abi.clone(),
            toolchain_id: model.toolchain_id.clone(),
            target_system: model.target.system.clone(),
            source: Some(AdmittedSource {
                codebase_id: "main".to_string(),
                logical_root: ".".to_string(),
                observed_root: workspace.0.canonicalize().unwrap(),
                source_mode: SourceMode::LiveWorkspace,
                source_identity: "live".to_string(),
                dirty_policy: DirtyPolicy::Warn,
                admission_fingerprint_policy: "live-fingerprint".to_string(),
            }),
            generator_json: serde_json::to_string(&model.generator).unwrap(),
            target_json: serde_json::to_string(&model.target).unwrap(),
            execution_model: crate::execution::lower(&model).unwrap(),
            secrets: ResolvedSecrets::empty(),
        };
        record_run_created(
            &mut registry,
            "run-unsafe-lock-root",
            &admission,
            &placement,
        )
        .unwrap();
        let endpoint_ports =
            std::collections::BTreeMap::from([("synthetic-tcp".to_string(), port)]);
        let mut prepare_ran = false;
        let injected = unsafe_root.open();
        let error = with_test_lock_root(injected.as_raw_fd(), || {
            match start_service_for_slot(
                &admission,
                &placement,
                &mut registry,
                "run-unsafe-lock-root",
                &selected,
                ServiceSelection {
                    service_name: "synthetic",
                    service_lifetime: ServiceLifetime::RunScoped,
                    endpoint_ports: &endpoint_ports,
                    slot_endpoints: &std::collections::BTreeMap::new(),
                    run_timeout_ms: 5000,
                    cancellation: &CancellationToken::new(),
                    prepare_runner: Some(Box::new(|_| {
                        prepare_ran = true;
                        Ok(())
                    })),
                },
            ) {
                Ok(service) => {
                    let _ = service.stop(&mut registry, 1000);
                    panic!("unsafe lock target must fail before prepare");
                }
                Err(error) => error,
            }
        });

        assert_eq!(error.code, ErrorCode::PortUnverifiable);
        assert!(!prepare_ran);
        let mutations: i64 = registry
            .connection()
            .query_row(
                "SELECT (SELECT count(*) FROM services) + (SELECT count(*) FROM processes)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(mutations, 0);
    }
}
