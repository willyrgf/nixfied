//! Private authority for endpoint startup coordination and kernel listener proof.
//!
//! The manifest exposes endpoints, but none of the machinery in this module is a
//! public runtime surface. Lock files are inert rendezvous inodes; kernel locks
//! serialize startup and kernel listener records are the steady-state truth.

use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::io;
use std::net::IpAddr;
#[cfg(test)]
use std::net::Ipv4Addr;
use std::os::fd::AsRawFd;
#[cfg(test)]
use std::os::fd::{FromRawFd, OwnedFd};
#[cfg(target_os = "linux")]
use std::os::unix::fs::MetadataExt;

use nixfied_manifest::ContainmentRequirement;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::filesystem::{Directory, DirectoryMode, PrivateFile};
use crate::service::process::{
    SelectedEndpoint, managed_listener_candidates, verify_listener_candidate,
};

use super::socket::TcpSocket;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
use linux::inspect as platform_inspect;
#[cfg(target_os = "macos")]
use macos::inspect as platform_inspect;

const LOCK_SUFFIX: &str = ".lock";
const MAX_DESCRIPTORS: usize = 65536;

#[cfg(target_os = "linux")]
fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> Option<[u8; N]> {
    let end = offset.checked_add(N)?;
    bytes.get(offset..end)?.try_into().ok()
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
    BindUnavailable {
        endpoint: SelectedEndpoint,
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
    _guards: Vec<PrivateFile>,
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

pub(super) struct ValidatedLockRoot(Directory);

impl ValidatedLockRoot {
    #[cfg(test)]
    fn new(fd: OwnedFd) -> Result<Self, EndpointFailure> {
        Directory::checked(fd, unsafe { libc::geteuid() }, DirectoryMode::Private)
            .map(Self)
            .map_err(coordination_error)
    }

    fn directory(&self) -> Result<ValidatedLockDirectory, EndpointFailure> {
        self.0
            .create_private_child(c"endpoint-locks")
            .map(ValidatedLockDirectory)
            .map_err(coordination_error)
    }
}

struct ValidatedLockDirectory(Directory);

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
    acquire_keys(&open_lock_root()?.directory()?, keys)
}

fn acquire_keys(
    lock_dir: &ValidatedLockDirectory,
    keys: Vec<EndpointKey>,
) -> Result<EndpointLockGuards, EndpointFailure> {
    let mut guards = Vec::with_capacity(keys.len());
    for key in keys {
        let name = CString::new(key.filename.as_str()).map_err(|_| {
            EndpointFailure::unverifiable(
                Some(key.endpoint.clone()),
                "invalid endpoint lock filename",
            )
        })?;
        let failure = |error| {
            EndpointFailure::unverifiable(
                Some(key.endpoint.clone()),
                format!("invalid endpoint coordination object: {error}"),
            )
        };
        let fd = lock_dir
            .0
            .try_lock_private_file(&name)
            .map_err(failure)?
            .ok_or_else(|| EndpointFailure::LockContended {
                endpoint: key.endpoint.clone(),
            })?;
        guards.push(fd);
    }
    Ok(EndpointLockGuards { _guards: guards })
}

#[cfg(test)]
thread_local! {
    /// Fault tests substitute a concrete lock root for their own thread; the
    /// production root is never configurable.
    static TEST_LOCK_ROOT: std::cell::RefCell<Option<OwnedFd>> =
        const { std::cell::RefCell::new(None) };
}

fn open_lock_root() -> Result<ValidatedLockRoot, EndpointFailure> {
    #[cfg(test)]
    if let Some(fd) = TEST_LOCK_ROOT.with(|root| root.borrow().as_ref().map(OwnedFd::try_clone)) {
        return ValidatedLockRoot::new(fd.map_err(coordination_error)?);
    }
    #[cfg(target_os = "linux")]
    const SYSTEM_COMPONENTS: &[&CStr] = &[c"tmp"];
    #[cfg(target_os = "macos")]
    const SYSTEM_COMPONENTS: &[&CStr] = &[c"private", c"tmp"];
    let mut current = Directory::root().map_err(coordination_error)?;
    for (index, component) in SYSTEM_COMPONENTS.iter().enumerate() {
        current = current
            .open_child(
                component,
                0,
                if index + 1 == SYSTEM_COMPONENTS.len() {
                    DirectoryMode::Sticky
                } else {
                    DirectoryMode::Any
                },
            )
            .map_err(coordination_error)?;
    }
    let user_component = CString::new(format!("nixfied-{}", unsafe { libc::geteuid() }))
        .expect("numeric user ID cannot contain NUL");
    current
        .create_private_child(&user_component)
        .map(ValidatedLockRoot)
        .map_err(coordination_error)
}

fn coordination_error(error: io::Error) -> EndpointFailure {
    EndpointFailure::unverifiable(
        None,
        format!("invalid endpoint coordination directory: {error}"),
    )
}

/// Stable OS identity of one listening socket, not an ownership lease.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "platform", rename_all = "kebab-case")]
pub(crate) enum KernelSocketIdentity {
    #[cfg(target_os = "linux")]
    Linux { inode: u32, cookie: [u32; 2] },
    #[cfg(target_os = "macos")]
    Macos {
        #[serde(rename = "socketHandle", serialize_with = "hex64")]
        socket_handle: u64,
        #[serde(rename = "inpcbGeneration", serialize_with = "hex64")]
        inpcb_generation: u64,
    },
}

#[cfg(target_os = "macos")]
fn hex64<S: serde::Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format!("0x{value:016x}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SocketRecord {
    address: IpAddr,
    port: u16,
    identity: KernelSocketIdentity,
}

/// Partial inspection retains positive sightings. Unreadable unrelated FDs do
/// not defeat them, but must never be interpreted as an empty socket set.
#[derive(Default)]
struct SocketScan {
    records: Vec<SocketRecord>,
    uncertainty: Option<String>,
}
impl SocketScan {
    fn uncertain(&mut self, reason: impl Into<String>) {
        self.uncertainty.get_or_insert(reason.into());
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListenerWitness {
    holder_pid: u32,
    holder_start_identity: String,
    socket_identity: KernelSocketIdentity,
    #[serde(skip)]
    holder_pgid: i32,
    #[serde(skip)]
    endpoint: SelectedEndpoint,
}
impl ListenerWitness {
    pub(crate) fn endpoint(&self) -> &SelectedEndpoint {
        &self.endpoint
    }
}

/// Only an OS observation can construct this exact selected endpoint set.
#[derive(Debug)]
pub(crate) struct CompleteWitnessSet(BTreeMap<String, ListenerWitness>);
impl CompleteWitnessSet {
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&String, &ListenerWitness)> {
        self.0.iter()
    }
    pub(crate) fn into_entries(self) -> BTreeMap<String, ListenerWitness> {
        self.0
    }
    pub(crate) fn matches(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

pub(crate) enum ListenerObservation {
    Complete(CompleteWitnessSet),
    Pending(String),
    Unverifiable(String),
}

pub(crate) struct ExpectedOwner<'a> {
    pub(crate) pid: u32,
    pub(crate) pgid: i32,
    pub(crate) platform_start: &'a str,
    pub(crate) containment: ContainmentRequirement,
}

/// Process identity/containment failures retain PROC_ESCAPE. `initial` requests
/// those exact witnesses again; another coexisting listener cannot replace one.
pub(crate) fn observe_listeners<'a>(
    endpoints: impl IntoIterator<Item = &'a SelectedEndpoint>,
    expected: &ExpectedOwner<'_>,
    initial: Option<&CompleteWitnessSet>,
) -> crate::error::RuntimeResult<ListenerObservation> {
    observe_with(endpoints, expected, initial, platform_inspect)
}

fn observe_with<'a>(
    endpoints: impl IntoIterator<Item = &'a SelectedEndpoint>,
    expected: &ExpectedOwner<'_>,
    initial: Option<&CompleteWitnessSet>,
    mut inspect: impl FnMut(u32, &BTreeMap<String, &SelectedEndpoint>) -> SocketScan,
) -> crate::error::RuntimeResult<ListenerObservation> {
    let endpoints: BTreeMap<_, _> = endpoints
        .into_iter()
        .map(|e| (e.endpoint_id.clone(), e))
        .collect();
    let candidates = managed_listener_candidates(expected)?;
    let mut found = BTreeMap::new();
    let mut uncertainty = None;
    for candidate in candidates {
        if !verify_listener_candidate(expected, &candidate)? {
            continue;
        }
        let scan = inspect(candidate.pid, &endpoints);
        if !verify_listener_candidate(expected, &candidate)? {
            continue;
        }
        if uncertainty.is_none() {
            uncertainty = scan.uncertainty;
        }
        for record in scan.records {
            for (id, endpoint) in &endpoints {
                if found.contains_key(id)
                    || endpoint.port != record.port
                    || endpoint.host.ip() != record.address
                {
                    continue;
                }
                let witness = ListenerWitness {
                    endpoint: (*endpoint).clone(),
                    holder_pid: candidate.pid,
                    holder_pgid: candidate.pgid,
                    holder_start_identity: candidate.start.clone(),
                    socket_identity: record.identity.clone(),
                };
                if initial.is_none_or(|set| set.0.get(id) == Some(&witness)) {
                    found.insert(id.clone(), witness);
                }
            }
        }
        if found.len() == endpoints.len() {
            break;
        }
    }
    if found.len() == endpoints.len() {
        Ok(ListenerObservation::Complete(CompleteWitnessSet(found)))
    } else if let Some(reason) = uncertainty {
        Ok(ListenerObservation::Unverifiable(reason))
    } else {
        let missing = endpoints
            .keys()
            .find(|id| !found.contains_key(*id))
            .expect("incomplete witness set");
        Ok(ListenerObservation::Pending(missing.clone()))
    }
}

pub(crate) fn preflight<'a>(
    endpoints: impl IntoIterator<Item = &'a SelectedEndpoint>,
) -> Result<(), EndpointFailure> {
    for endpoint in endpoints {
        match bind_exact(endpoint)
            .map_err(|message| EndpointFailure::unverifiable(Some(endpoint.clone()), message))?
        {
            BindResult::Available => {}
            BindResult::AddressInUse => {
                return Err(EndpointFailure::BindUnavailable {
                    endpoint: endpoint.clone(),
                });
            }
        }
    }
    Ok(())
}

enum BindResult {
    Available,
    AddressInUse,
}

fn bind_exact(endpoint: &SelectedEndpoint) -> Result<BindResult, String> {
    let socket = TcpSocket::new(endpoint.host.ip())
        .map_err(|error| format!("failed to create endpoint preflight socket: {error}"))?;
    let reuse: libc::c_int = 1;
    // SAFETY: `socket` is live and `reuse` has the type and size required by
    // SO_REUSEADDR.
    let reused = unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_REUSEADDR,
            (&raw const reuse).cast(),
            std::mem::size_of_val(&reuse) as libc::socklen_t,
        )
    };
    if reused < 0 {
        return Err(format!(
            "failed to enable address reuse for endpoint preflight socket: {}",
            io::Error::last_os_error()
        ));
    }
    match socket.bind(endpoint.host.ip(), endpoint.port) {
        Ok(()) => Ok(BindResult::Available),
        Err(error) if error.raw_os_error() == Some(libc::EADDRINUSE) => {
            Ok(BindResult::AddressInUse)
        }
        Err(error) => Err(format!(
            "failed to bind endpoint preflight socket at {}:{}: {error}",
            endpoint.host, endpoint.port
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{File, OpenOptions};
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink};
    use std::path::{Path, PathBuf};

    struct TestRoot(crate::test_support::TestDir);

    impl TestRoot {
        fn new() -> Self {
            Self(crate::test_support::TestDir::new("endpoint-test"))
        }

        fn open(&self) -> File {
            File::open(&self.0).unwrap()
        }

        fn locks(&self) -> PathBuf {
            self.0.join("endpoint-locks")
        }
    }

    /// Run `operation` with `root` substituted as this thread's lock root.
    fn with_root<T>(root: &TestRoot, operation: impl FnOnce(&ValidatedLockRoot) -> T) -> T {
        let fd: OwnedFd = root.open().into();
        TEST_LOCK_ROOT.with(|slot| *slot.borrow_mut() = Some(fd.try_clone().unwrap()));
        let result = operation(&ValidatedLockRoot::new(fd).unwrap());
        TEST_LOCK_ROOT.with(|slot| *slot.borrow_mut() = None);
        result
    }

    fn assert_unverifiable(result: Result<EndpointLockGuards, EndpointFailure>) {
        assert!(matches!(result, Err(EndpointFailure::Unverifiable { .. })));
    }

    fn endpoint(address: &str, port: u16) -> SelectedEndpoint {
        SelectedEndpoint {
            endpoint_id: "test".to_string(),
            host: nixfied_manifest::LoopbackHost::parse(address).unwrap(),
            port,
        }
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
    fn bound_non_listener_is_bind_unavailable() {
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
            Err(EndpointFailure::BindUnavailable { endpoint }) if endpoint == planned
        ));
    }

    fn observe_current(
        endpoint: &SelectedEndpoint,
        initial: Option<&CompleteWitnessSet>,
    ) -> ListenerObservation {
        let pid = std::process::id();
        let pgid = crate::service::process::process_group(pid)
            .unwrap()
            .unwrap();
        let start = crate::service::process::platform_start_identity(pid).unwrap();
        observe_listeners(
            [endpoint],
            &ExpectedOwner {
                pid,
                pgid,
                platform_start: &start,
                containment: ContainmentRequirement::ProcessGroup,
            },
            initial,
        )
        .unwrap()
    }

    #[test]
    fn production_observer_requires_exact_managed_listen_and_stable_identity() {
        if crate::test_support::isolate(
            "service::endpoint::tests::production_observer_requires_exact_managed_listen_and_stable_identity",
        ) {
            return;
        }
        for host in ["127.0.0.1", "::1"] {
            let held = std::net::TcpListener::bind((host, 0)).unwrap();
            let planned = endpoint(host, held.local_addr().unwrap().port());
            let ListenerObservation::Complete(first) = observe_current(&planned, None) else {
                panic!("missing positive listener");
            };
            let ListenerObservation::Complete(second) = observe_current(&planned, Some(&first))
            else {
                panic!("unchanged witness missing");
            };
            assert!(first.matches(&second));
            drop(held);
            let _replacement = std::net::TcpListener::bind((host, planned.port)).unwrap();
            assert!(matches!(
                observe_current(&planned, Some(&first)),
                ListenerObservation::Pending(_)
            ));
            assert!(matches!(
                observe_current(&planned, None),
                ListenerObservation::Complete(_)
            ));
        }
        let wildcard = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        let planned = endpoint("127.0.0.1", wildcard.local_addr().unwrap().port());
        assert!(matches!(
            observe_current(&planned, None),
            ListenerObservation::Pending(_)
        ));
    }

    #[test]
    fn exact_ipv6_listeners_with_either_v6only_mode_are_positive_witnesses() {
        if crate::test_support::isolate(
            "service::endpoint::tests::exact_ipv6_listeners_with_either_v6only_mode_are_positive_witnesses",
        ) {
            return;
        }
        for only_v6 in [0_i32, 1] {
            let address: IpAddr = "::1".parse().unwrap();
            let held = super::super::socket::TcpSocket::new(address).unwrap();
            assert_eq!(
                unsafe {
                    libc::setsockopt(
                        held.as_raw_fd(),
                        libc::IPPROTO_IPV6,
                        libc::IPV6_V6ONLY,
                        (&raw const only_v6).cast(),
                        std::mem::size_of_val(&only_v6) as libc::socklen_t,
                    )
                },
                0
            );
            held.bind(address, 0).unwrap();
            let mut name: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
            let mut size = std::mem::size_of_val(&name) as libc::socklen_t;
            assert_eq!(
                unsafe { libc::getsockname(held.as_raw_fd(), (&raw mut name).cast(), &mut size) },
                0
            );
            let planned = endpoint("::1", u16::from_be(name.sin6_port));
            assert!(matches!(
                observe_current(&planned, None),
                ListenerObservation::Pending(_)
            ));
            assert_eq!(unsafe { libc::listen(held.as_raw_fd(), 8) }, 0);
            assert!(matches!(
                observe_current(&planned, None),
                ListenerObservation::Complete(_)
            ));
        }
    }

    #[test]
    fn a_managed_witness_does_not_claim_exclusive_socket_ownership() {
        if crate::test_support::isolate(
            "service::endpoint::tests::a_managed_witness_does_not_claim_exclusive_socket_ownership",
        ) {
            return;
        }
        use std::os::unix::process::CommandExt;
        let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let planned = endpoint("127.0.0.1", held.local_addr().unwrap().port());
        let inherited = OwnedFd::from(held.try_clone().unwrap());
        // A process outside the managed group also holds this very socket.
        let mut foreign = std::process::Command::new(
            std::env::var_os("NIXFIED_TEST_CHILD").expect("run with nix develop"),
        )
        .arg("accept-inherited")
        .stdin(inherited)
        .process_group(0)
        .spawn()
        .unwrap();
        let observed = observe_current(&planned, None);
        use std::io::Read;
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", planned.port)).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        let mut response = String::new();
        let read = stream.read_to_string(&mut response);
        let _ = foreign.kill();
        foreign.wait().unwrap();
        read.unwrap();
        assert_eq!(response, "foreign responder\n");
        assert!(matches!(observed, ListenerObservation::Complete(_)));
        assert!(matches!(
            observe_current(&planned, None),
            ListenerObservation::Complete(_)
        ));
    }

    #[test]
    fn inspection_faults_never_become_absence_or_erase_complete_positive_evidence() {
        if crate::test_support::isolate(
            "service::endpoint::tests::inspection_faults_never_become_absence_or_erase_complete_positive_evidence",
        ) {
            return;
        }
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let planned = endpoint("127.0.0.1", listener.local_addr().unwrap().port());
        let pid = std::process::id();
        let start = crate::service::process::platform_start_identity(pid).unwrap();
        let owner = ExpectedOwner {
            pid,
            pgid: crate::service::process::process_group(pid)
                .unwrap()
                .unwrap(),
            platform_start: &start,
            containment: ContainmentRequirement::ProcessTree,
        };
        // Inject at the platform inspector boundary while retaining real
        // candidate enumeration and identity checks on both sides.
        for fault in [
            "permission denied",
            "unsupported record",
            "bounded list exhausted",
        ] {
            let observation = observe_with([&planned], &owner, None, |_, _| SocketScan {
                records: vec![],
                uncertainty: Some(fault.into()),
            })
            .unwrap();
            assert!(matches!(observation, ListenerObservation::Unverifiable(_)));
            let observation = observe_with([&planned], &owner, None, |pid, endpoints| {
                let mut scan = platform_inspect(pid, endpoints);
                scan.uncertain(fault);
                scan
            })
            .unwrap();
            assert!(matches!(observation, ListenerObservation::Complete(_)));
        }
        let observation =
            observe_with([&planned], &owner, None, |_, _| SocketScan::default()).unwrap();
        assert!(matches!(observation, ListenerObservation::Pending(_)));
        let stale_candidate = crate::service::process::ListenerCandidate {
            pid,
            pgid: owner.pgid,
            start: "previous-holder-of-this-pid".into(),
        };
        assert_eq!(
            verify_listener_candidate(&owner, &stale_candidate)
                .unwrap_err()
                .code,
            crate::error::ErrorCode::ProcEscape
        );
        let stale = ExpectedOwner {
            platform_start: "stale-pid-identity",
            ..owner
        };
        let error = observe_with([&planned], &stale, None, |_, _| {
            panic!("stale leader must reject before FD inspection")
        })
        .err()
        .unwrap();
        assert_eq!(error.code, crate::error::ErrorCode::ProcEscape);
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
        with_root(&symlink_root, |_| {
            // Endpoint-less services must not inspect even an unsafe lock child.
            assert_eq!(acquire_startup_locks(std::iter::empty()).unwrap().len(), 0);
            assert_unverifiable(acquire_startup_locks(std::slice::from_ref(&planned)));
        });

        let mode_root = TestRoot::new();
        std::fs::create_dir(mode_root.locks()).unwrap();
        std::fs::set_permissions(mode_root.locks(), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        with_root(&mode_root, |_| {
            assert_unverifiable(acquire_startup_locks(std::slice::from_ref(&planned)));
        });

        let target_root = TestRoot::new();
        std::fs::create_dir(target_root.locks()).unwrap();
        std::fs::set_permissions(target_root.locks(), std::fs::Permissions::from_mode(0o700))
            .unwrap();
        let key = EndpointKey::derive(&planned, &NetworkScope::production().unwrap());
        std::fs::create_dir(target_root.locks().join(key.filename)).unwrap();
        with_root(&target_root, |_| {
            assert_unverifiable(acquire_startup_locks(std::slice::from_ref(&planned)));
        });
    }

    #[test]
    fn endpoint_lock_rejects_hardlinks_and_special_files_without_repair() {
        let planned = endpoint("127.0.0.1", 23109);
        for special in [false, true] {
            let root = TestRoot::new();
            with_root(&root, |lock_root| {
                let directory = lock_root.directory().unwrap();
                let key = EndpointKey::derive(&planned, &NetworkScope::production().unwrap());
                let name = CString::new(key.filename.as_str()).unwrap();
                let path = root.locks().join(&key.filename);
                if special {
                    use std::os::unix::ffi::OsStrExt;
                    let path = CString::new(path.as_os_str().as_bytes()).unwrap();
                    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
                } else {
                    drop(directory.0.open_private_file(&name).unwrap());
                    std::fs::hard_link(&path, root.locks().join("alias")).unwrap();
                }
                // A substituted FIFO must reject immediately, never block on open.
                assert_unverifiable(acquire_startup_locks(std::slice::from_ref(&planned)));
                let metadata = std::fs::symlink_metadata(path).unwrap();
                if special {
                    use std::os::unix::fs::FileTypeExt;
                    assert!(metadata.file_type().is_fifo());
                } else {
                    assert!(root.locks().join("alias").exists());
                }
            });
        }
    }

    #[test]
    fn coordination_revalidation_rejects_replaced_entry_after_locking() {
        let root = TestRoot::new();
        with_root(&root, |lock_root| {
            let directory = lock_root.directory().unwrap();
            let file = directory.0.open_private_file(c"slot.lock").unwrap();
            assert_eq!(
                unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
                0
            );
            std::fs::rename(
                root.locks().join("slot.lock"),
                root.locks().join("displaced"),
            )
            .unwrap();
            let replacement = directory.0.open_private_file(c"slot.lock").unwrap();
            assert!(
                directory
                    .0
                    .verify_private_file(c"slot.lock", &file)
                    .is_err()
            );
            directory
                .0
                .verify_private_file(c"slot.lock", &replacement)
                .unwrap();
            // The check never unlinks either object to pretend exclusion survived.
            assert!(root.locks().join("displaced").exists());
        });
    }

    #[test]
    fn coordination_components_cannot_traverse_or_follow_symlinks() {
        let root = TestRoot::new();
        with_root(&root, |lock_root| {
            let directory = lock_root.directory().unwrap();
            for name in [c"", c".", c"..", c"../escape", c"/escape", c"nested/file"] {
                assert!(directory.0.open_private_file(name).is_err());
                assert!(directory.0.create_private_child(name).is_err());
            }
            std::fs::write(root.0.join("untouched"), b"keep").unwrap();
            symlink(root.0.join("untouched"), root.locks().join("linked")).unwrap();
            assert!(directory.0.open_private_file(c"linked").is_err());
            assert_eq!(std::fs::read(root.0.join("untouched")).unwrap(), b"keep");
            assert!(!root.0.join("escape").exists());
        });
    }

    #[test]
    fn multi_lock_failure_releases_the_partial_set_and_fds_are_cloexec() {
        let root = TestRoot::new();
        with_root(&root, |_| {
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
        with_root(&root, |_| {
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
        assert!(matches!(
            ValidatedLockRoot::new(root.open().into()),
            Err(EndpointFailure::Unverifiable { .. })
        ));
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
        with_root(&root, |_| {
            assert_unverifiable(acquire_startup_locks(std::slice::from_ref(&planned)));
        });
        assert!(Path::new(&path).exists());
    }

    #[test]
    fn unsafe_lock_root_fails_the_real_start_path_before_prepare() {
        use crate::ErrorCode;
        use crate::admission::{AdmissionContext, InvocationRoot, StoreOriginPolicy};
        use crate::cancellation::CancellationToken;
        use crate::registry::session::record_run_created;
        use crate::registry::{Registry, RegistryIdentity};
        use crate::service::process::{ServiceSelection, start_service_for_slot};
        use crate::slot::select_slot;
        use nixfied_manifest::Manifest;
        use nixfied_manifest::fixtures::{SyntheticManifestOptions, synthetic_manifest};
        use serde_json::json;

        let unsafe_root = TestRoot::new();
        let symlink_target = unsafe_root.0.join("attacker-controlled");
        std::fs::create_dir(&symlink_target).unwrap();
        symlink(&symlink_target, unsafe_root.locks()).unwrap();

        let workspace = TestRoot::new();
        let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = held.local_addr().unwrap().port();
        drop(held);
        let mut value = synthetic_manifest(&SyntheticManifestOptions {
            executable: std::env::var("NIXFIED_TEST_SLEEP").expect("Nix-built sleep fixture"),
            start_args: vec!["30".to_string()],
            port_start: port,
            port_end: port,
            ..SyntheticManifestOptions::default()
        });
        value["services"]["synthetic"]["lifecycle"]["prepare"] = json!({ "task": "smoke" });
        value["tasks"]["smoke"]["requires"] = json!([]);

        value["tasks"]["smoke"]["invocation"]["run"] = json!(["sleep", "0"]);
        let manifest: Manifest = serde_json::from_value(value).unwrap();
        let manifest_path = workspace.0.join("manifest.json");
        std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let mut context = AdmissionContext::current(StoreOriginPolicy::AllowNonStoreForTests);
        context.invocation_root = InvocationRoot::Path(workspace.0.to_path_buf());
        let admission = crate::admit_run(&manifest_path, &context).unwrap();
        let selected = select_slot(&manifest, None).unwrap();
        let placement = crate::state::derive_slot_placement(
            &manifest.project.project_id,
            selected.environment,
            selected.slot,
            "run-unsafe-lock-root",
            &workspace.0,
        )
        .unwrap();
        let mut registry = Registry::open_or_create(
            crate::state::ownership::fixture_guard(
                placement.state_base(),
                &RegistryIdentity::for_slot(
                    &manifest.project.project_id,
                    selected.environment,
                    selected.slot,
                    &manifest.runtime_abi,
                    &manifest.toolchain_id,
                ),
            ),
            &RegistryIdentity::for_slot(
                &manifest.project.project_id,
                selected.environment,
                selected.slot,
                &manifest.runtime_abi,
                &manifest.toolchain_id,
            ),
        )
        .unwrap();
        registry.authority().claim_run_dir(&placement).unwrap();
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
        let started = with_root(&unsafe_root, |_| {
            start_service_for_slot(
                &admission,
                &placement,
                &mut registry,
                "run-unsafe-lock-root",
                &selected,
                ServiceSelection {
                    launcher: Path::new("/unused-before-endpoint-rejection"),
                    session_checkpoint: &|| Ok(()),
                    service_name: "synthetic",
                    endpoint_ports: &endpoint_ports,
                    slot_endpoints: &std::collections::BTreeMap::new(),
                    run_timeout_ms: 5000,
                    cancellation: &CancellationToken::new(),
                    prepare_runner: Some(Box::new(|_| {
                        prepare_ran = true;
                        Ok(())
                    })),
                },
            )
        });
        let error = match started {
            Ok(service) => {
                drop(service);
                panic!("unsafe lock target must fail before prepare");
            }
            Err(error) => error,
        };

        assert_eq!(error.code, ErrorCode::PortUnverifiable);
        assert!(!prepare_ran);
        let mutations: i64 = registry
            .connection()
            .query_row(
                "SELECT (SELECT count(*) FROM sqlite_master WHERE name = 'services') + (SELECT count(*) FROM processes)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(mutations, 0);
    }
}
