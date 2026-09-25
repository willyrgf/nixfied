//! Descriptor-relative access to runtime-managed coordination objects.
//!
//! These checks establish the opened object and its current directory entry, not
//! lasting protection against a same-user writer replacing managed ancestry.
//! Lock lifetimes and deletion authorization belong to the calling owners.

use std::ffi::{CStr, CString};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

pub(crate) enum DirectoryMode {
    Any,
    Sticky,
    Private,
}

pub(crate) struct Directory(OwnedFd);
pub(crate) struct PrivateFile(OwnedFd);

impl Directory {
    /// Resolve aliases only at the caller-selected anchor. Managed children are
    /// subsequently opened descriptor-relative without following symlinks.
    pub(crate) fn private_anchor(path: &Path) -> io::Result<Self> {
        if path.as_os_str().is_empty() {
            return Err(invalid("empty coordination anchor"));
        }
        match path.canonicalize() {
            Ok(canonical) => {
                let directory = Self::open_external(&canonical)?;
                Self::checked(
                    directory.0,
                    unsafe { libc::geteuid() },
                    DirectoryMode::Private,
                )
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let absolute = std::path::absolute(path)?;
                let mut ancestor = absolute.as_path();
                let mut missing: Vec<CString> = Vec::new();
                loop {
                    match ancestor.canonicalize() {
                        Ok(canonical) => {
                            let mut directory = Self::open_external(&canonical)?;
                            for name in missing.into_iter().rev() {
                                directory = directory.create_private_child(&name)?;
                            }
                            return Ok(directory);
                        }
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {
                            let name = ancestor
                                .file_name()
                                .ok_or_else(|| invalid("invalid coordination anchor"))?;
                            missing.push(
                                CString::new(name.as_bytes())
                                    .map_err(|_| invalid("NUL in coordination anchor"))?,
                            );
                            ancestor = ancestor
                                .parent()
                                .ok_or_else(|| invalid("coordination anchor has no parent"))?;
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
            Err(error) => Err(error),
        }
    }

    fn open_external(path: &Path) -> io::Result<Self> {
        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| invalid("NUL in coordination anchor"))?;
        let fd = owned(unsafe {
            libc::open(
                path.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        })?;
        let stat = stat_fd(fd.as_raw_fd())?;
        let trusted_owner = stat.st_uid == 0 || stat.st_uid == unsafe { libc::geteuid() };
        let writable = stat.st_mode & 0o022 != 0;
        let sticky_root = stat.st_uid == 0 && stat.st_mode & libc::S_ISVTX as libc::mode_t != 0;
        if !trusted_owner || (writable && !sticky_root) {
            return Err(invalid("unsafe coordination anchor ancestry"));
        }
        Self::checked(fd, stat.st_uid, DirectoryMode::Any)
    }

    pub(crate) fn root() -> io::Result<Self> {
        let raw = unsafe {
            libc::open(
                c"/".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        Self::checked(owned(raw)?, 0, DirectoryMode::Any)
    }

    pub(crate) fn checked(fd: OwnedFd, uid: libc::uid_t, mode: DirectoryMode) -> io::Result<Self> {
        check_directory(fd.as_raw_fd(), uid, mode)?;
        Ok(Self(fd))
    }

    pub(crate) fn open_child(
        &self,
        name: &CStr,
        uid: libc::uid_t,
        mode: DirectoryMode,
    ) -> io::Result<Self> {
        let fd = open_at(
            self.0.as_raw_fd(),
            name,
            libc::O_RDONLY | libc::O_DIRECTORY,
            0,
        )?;
        let child = Self::checked(fd, uid, mode)?;
        self.verify_entry(name, child.0.as_raw_fd())?;
        Ok(child)
    }

    pub(crate) fn create_private_child(&self, name: &CStr) -> io::Result<Self> {
        component(name)?;
        if unsafe { libc::mkdirat(self.0.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::EEXIST) {
                return Err(error);
            }
        }
        self.open_child(name, unsafe { libc::geteuid() }, DirectoryMode::Private)
    }

    /// Open without truncation or repair. Nonblocking open lets validation reject
    /// a substituted FIFO instead of waiting for an unrelated writer.
    pub(crate) fn open_private_file(&self, name: &CStr) -> io::Result<PrivateFile> {
        let flags = libc::O_RDWR | libc::O_NONBLOCK;
        let fd = match open_at(
            self.0.as_raw_fd(),
            name,
            flags | libc::O_CREAT | libc::O_EXCL,
            0o600,
        ) {
            Ok(fd) => fd,
            Err(error) if error.raw_os_error() == Some(libc::EEXIST) => {
                open_at(self.0.as_raw_fd(), name, flags, 0)?
            }
            Err(error) => return Err(error),
        };
        let file = PrivateFile(fd);
        self.verify_private_file(name, &file)?;
        Ok(file)
    }

    /// Repeat after acquiring a lock: the locked descriptor must still name the
    /// expected private object. This does not authorize replacement or repair.
    pub(crate) fn verify_private_file(&self, name: &CStr, file: &PrivateFile) -> io::Result<()> {
        let stat = stat_fd(file.0.as_raw_fd())?;
        if stat.st_mode & libc::S_IFMT != libc::S_IFREG
            || stat.st_uid != unsafe { libc::geteuid() }
            || stat.st_mode & 0o7777 != 0o600
            || stat.st_nlink != 1
        {
            return Err(invalid(
                "file must be regular, effective-user-owned, mode 0600, and singly linked",
            ));
        }
        self.verify_entry(name, file.0.as_raw_fd())
    }

    pub(crate) fn verify_child(&self, name: &CStr, child: &Self) -> io::Result<()> {
        check_directory(
            child.0.as_raw_fd(),
            unsafe { libc::geteuid() },
            DirectoryMode::Private,
        )?;
        self.verify_entry(name, child.0.as_raw_fd())
    }

    pub(crate) fn verify_anchor(&self, path: &Path) -> io::Result<()> {
        let current = Self::open_external(&path.canonicalize()?)?;
        check_directory(
            current.0.as_raw_fd(),
            unsafe { libc::geteuid() },
            DirectoryMode::Private,
        )?;
        let held = stat_fd(self.0.as_raw_fd())?;
        let observed = stat_fd(current.0.as_raw_fd())?;
        if held.st_dev != observed.st_dev || held.st_ino != observed.st_ino {
            return Err(invalid("coordination anchor was replaced"));
        }
        Ok(())
    }

    fn verify_entry(&self, name: &CStr, fd: RawFd) -> io::Result<()> {
        component(name)?;
        let opened = stat_fd(fd)?;
        let mut entry = std::mem::MaybeUninit::<libc::stat>::zeroed();
        if unsafe {
            libc::fstatat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                entry.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fstatat initialized the complete stat on success.
        let entry = unsafe { entry.assume_init() };
        if opened.st_dev != entry.st_dev
            || opened.st_ino != entry.st_ino
            || opened.st_mode & libc::S_IFMT != entry.st_mode & libc::S_IFMT
        {
            return Err(invalid(
                "opened object no longer matches its directory entry",
            ));
        }
        Ok(())
    }
}

impl Directory {
    /// Open a child directory owned by the effective user without following a
    /// symlink, and verify the opened object is still the named entry.
    pub(crate) fn open_owned_child(&self, name: &CStr) -> io::Result<Self> {
        self.open_child(name, unsafe { libc::geteuid() }, DirectoryMode::Any)
    }

    /// Another close-on-exec descriptor for the same opened directory.
    pub(crate) fn try_clone(&self) -> io::Result<Self> {
        self.0.try_clone().map(Self)
    }

    pub(crate) fn identity(&self) -> io::Result<FileIdentity> {
        stat_fd(self.0.as_raw_fd()).map(|stat| FileIdentity::from(&stat))
    }

    /// Observe one entry without following it. Absence is `None`.
    pub(crate) fn entry(&self, name: &CStr) -> io::Result<Option<EntryKind>> {
        component(name)?;
        let mut entry = std::mem::MaybeUninit::<libc::stat>::zeroed();
        if unsafe {
            libc::fstatat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                entry.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(libc::ENOENT) {
                Ok(None)
            } else {
                Err(error)
            };
        }
        // SAFETY: fstatat initialized the complete stat on success.
        let entry = unsafe { entry.assume_init() };
        let identity = FileIdentity::from(&entry);
        Ok(Some(match entry.st_mode & libc::S_IFMT {
            libc::S_IFDIR => EntryKind::Directory(identity),
            libc::S_IFREG => EntryKind::File(identity),
            _ => EntryKind::Other(identity),
        }))
    }

    /// Whether the named entry is the root of a mount. Linux reports this
    /// through `statx`, which also sees a bind mount of the same filesystem.
    /// A kernel that cannot report it fails closed.
    #[cfg(target_os = "linux")]
    pub(crate) fn is_mount_root(&self, name: &CStr) -> io::Result<bool> {
        component(name)?;
        let mut observed = std::mem::MaybeUninit::<libc::statx>::zeroed();
        if unsafe {
            libc::statx(
                self.0.as_raw_fd(),
                name.as_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
                libc::STATX_BASIC_STATS,
                observed.as_mut_ptr(),
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: statx initialized the complete buffer on success.
        let observed = unsafe { observed.assume_init() };
        let mount_root = libc::STATX_ATTR_MOUNT_ROOT as u64;
        if observed.stx_attributes_mask & mount_root == 0 {
            return Err(invalid("the kernel cannot report mount roots"));
        }
        Ok(observed.stx_attributes & mount_root != 0)
    }

    /// Other platforms have no same-device mounts; a device change is the
    /// available evidence and the caller checks it.
    #[cfg(not(target_os = "linux"))]
    pub(crate) fn is_mount_root(&self, name: &CStr) -> io::Result<bool> {
        component(name)?;
        Ok(false)
    }

    /// The name of a known network filesystem that holds this directory. Its
    /// lock and deletion semantics are outside the supported placement.
    pub(crate) fn network_filesystem(&self) -> io::Result<Option<&'static str>> {
        let mut observed = std::mem::MaybeUninit::<libc::statfs>::zeroed();
        if unsafe { libc::fstatfs(self.0.as_raw_fd(), observed.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fstatfs initialized the complete buffer on success.
        Ok(network_filesystem(&unsafe { observed.assume_init() }))
    }

    /// All entry names except `.` and `..`, read through a duplicate descriptor.
    pub(crate) fn entry_names(&self) -> io::Result<Vec<CString>> {
        let duplicate =
            owned(unsafe { libc::fcntl(self.0.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) })?;
        // SAFETY: fdopendir takes ownership of the duplicate on success.
        let stream = unsafe { libc::fdopendir(duplicate.as_raw_fd()) };
        if stream.is_null() {
            return Err(io::Error::last_os_error());
        }
        let _ = duplicate.into_raw_fd();
        let mut names = Vec::new();
        let result = loop {
            // readdir reports errors only through errno with a null result.
            unsafe { *errno() = 0 };
            let entry = unsafe { libc::readdir(stream) };
            if entry.is_null() {
                let code = unsafe { *errno() };
                break if code == 0 {
                    Ok(())
                } else {
                    Err(io::Error::from_raw_os_error(code))
                };
            }
            // SAFETY: readdir returned a valid entry with a NUL-terminated name.
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
            if name.to_bytes() != b"." && name.to_bytes() != b".." {
                names.push(name.to_owned());
            }
        };
        unsafe { libc::closedir(stream) };
        result.map(|()| names)
    }

    /// Read a small regular file without following a symlink.
    pub(crate) fn read_regular_file(&self, name: &CStr, limit: usize) -> io::Result<Vec<u8>> {
        let fd = open_at(
            self.0.as_raw_fd(),
            name,
            libc::O_RDONLY | libc::O_NONBLOCK,
            0,
        )?;
        let stat = stat_fd(fd.as_raw_fd())?;
        if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
            return Err(invalid("expected a regular file"));
        }
        self.verify_entry(name, fd.as_raw_fd())?;
        let mut bytes = Vec::new();
        let mut reader = std::io::Read::take(std::fs::File::from(fd), limit as u64 + 1);
        std::io::Read::read_to_end(&mut reader, &mut bytes)?;
        if bytes.len() > limit {
            return Err(invalid("file exceeds its size limit"));
        }
        Ok(bytes)
    }

    /// Remove one entry relative to this directory. Unlinking never opens,
    /// truncates, or follows the entry.
    pub(crate) fn remove_entry(&self, name: &CStr, directory: bool) -> io::Result<()> {
        component(name)?;
        let flags = if directory { libc::AT_REMOVEDIR } else { 0 };
        if unsafe { libc::unlinkat(self.0.as_raw_fd(), name.as_ptr(), flags) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Write a complete private temporary file, make it durable, atomically
    /// replace `name`, then make the directory entry durable.
    pub(crate) fn publish_file(&self, name: &CStr, bytes: &[u8]) -> io::Result<()> {
        use std::io::Write;
        component(name)?;
        let token = crate::token::random_hex().map_err(|error| io::Error::other(error.message))?;
        let temporary = temporary_name(name, &token)?;
        let fd = open_at(
            self.0.as_raw_fd(),
            &temporary,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )?;
        let mut file = std::fs::File::from(fd);
        let written = file.write_all(bytes).and_then(|()| file.sync_all());
        drop(file);
        let renamed = written.and_then(|()| {
            if unsafe {
                libc::renameat(
                    self.0.as_raw_fd(),
                    temporary.as_ptr(),
                    self.0.as_raw_fd(),
                    name.as_ptr(),
                )
            } != 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
        if let Err(error) = renamed {
            let _ = self.remove_entry(&temporary, false);
            return Err(error);
        }
        self.sync()
    }

    /// Make this directory's entry changes durable to the extent `fsync`
    /// guarantees on the host filesystem.
    pub(crate) fn sync(&self) -> io::Result<()> {
        if unsafe { libc::fsync(self.0.as_raw_fd()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

/// Device and inode of an opened object: corroborating evidence only, never a
/// permanent identity across inode reuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileIdentity {
    pub(crate) device: u64,
    pub(crate) inode: u64,
}

impl From<&libc::stat> for FileIdentity {
    fn from(stat: &libc::stat) -> Self {
        #[allow(clippy::unnecessary_cast)]
        Self {
            device: stat.st_dev as u64,
            inode: stat.st_ino as u64,
        }
    }
}

impl std::fmt::Display for FileIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}:{}", self.device, self.inode)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryKind {
    Directory(FileIdentity),
    File(FileIdentity),
    Other(FileIdentity),
}

#[cfg(target_os = "linux")]
unsafe fn errno() -> *mut libc::c_int {
    unsafe { libc::__errno_location() }
}

#[cfg(target_os = "macos")]
unsafe fn errno() -> *mut libc::c_int {
    unsafe { libc::__error() }
}

impl PrivateFile {
    /// Consume ownership before close; an error must never cause a second close
    /// against a potentially reused descriptor number.
    pub(crate) fn close(self) -> io::Result<()> {
        let raw = self.0.into_raw_fd();
        if unsafe { libc::close(raw) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

impl AsRawFd for PrivateFile {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

#[cfg(target_os = "linux")]
fn network_filesystem(observed: &libc::statfs) -> Option<&'static str> {
    // The magic number's C type differs across Linux targets.
    #[allow(clippy::unnecessary_cast)]
    network_magic(observed.f_type as u32)
}

#[cfg(target_os = "linux")]
fn network_magic(magic: u32) -> Option<&'static str> {
    match magic {
        0x6969 => Some("nfs"),
        0x517b => Some("smb"),
        0xff53_4d42 => Some("cifs"),
        0xfe53_4d42 => Some("smb2"),
        0x5346_414f => Some("afs"),
        0x00c3_6400 => Some("ceph"),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn network_filesystem(observed: &libc::statfs) -> Option<&'static str> {
    // SAFETY: the kernel NUL-terminates the fixed-size type name.
    let name = unsafe { CStr::from_ptr(observed.f_fstypename.as_ptr()) };
    match name.to_bytes() {
        b"nfs" => Some("nfs"),
        b"smbfs" => Some("smbfs"),
        b"afpfs" => Some("afpfs"),
        b"webdav" => Some("webdav"),
        _ => None,
    }
}

/// The temporary entry [`Directory::publish_file`] writes before its rename.
/// Only a process killed between creation and rename leaves one behind.
pub(crate) fn temporary_name(name: &CStr, token: &str) -> io::Result<CString> {
    let mut bytes = b".".to_vec();
    bytes.extend_from_slice(name.to_bytes());
    bytes.extend_from_slice(format!(".{token}.tmp").as_bytes());
    CString::new(bytes).map_err(|_| invalid("NUL in file name"))
}

/// Whether `entry` is a temporary that publishing `name` left behind.
pub(crate) fn is_temporary_of(entry: &CStr, name: &CStr) -> bool {
    let entry = entry.to_bytes();
    let Some(rest) = entry
        .strip_prefix(b".")
        .and_then(|rest| rest.strip_prefix(name.to_bytes()))
        .and_then(|rest| rest.strip_prefix(b"."))
        .and_then(|rest| rest.strip_suffix(b".tmp"))
    else {
        return false;
    };
    !rest.is_empty() && rest.iter().all(u8::is_ascii_hexdigit)
}

fn component(name: &CStr) -> io::Result<()> {
    let bytes = name.to_bytes();
    if bytes.is_empty() || bytes == b"." || bytes == b".." || bytes.contains(&b'/') {
        return Err(invalid("expected one non-traversing filesystem component"));
    }
    Ok(())
}

fn open_at(
    parent: RawFd,
    name: &CStr,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> io::Result<OwnedFd> {
    component(name)?;
    // mode_t is narrower than C's variadic integer type on macOS.
    owned(unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            mode as libc::c_uint,
        )
    })
}

fn owned(fd: libc::c_int) -> io::Result<OwnedFd> {
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful open/openat returned a new descriptor owned here.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn stat_fd(fd: RawFd) -> io::Result<libc::stat> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::zeroed();
    if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fstat initialized the complete stat on success.
    Ok(unsafe { stat.assume_init() })
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn check_directory(fd: RawFd, uid: libc::uid_t, mode: DirectoryMode) -> io::Result<()> {
    let stat = stat_fd(fd)?;
    if stat.st_mode & libc::S_IFMT != libc::S_IFDIR || stat.st_uid != uid {
        return Err(invalid("directory has an unexpected type or owner"));
    }
    let permissions = stat.st_mode & 0o7777;
    let valid = match mode {
        DirectoryMode::Any => true,
        DirectoryMode::Sticky => permissions & libc::S_ISVTX as libc::mode_t != 0,
        DirectoryMode::Private => permissions == 0o700,
    };
    if !valid {
        return Err(invalid("directory has unsafe permissions"));
    }
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn known_network_filesystems_are_named_and_local_ones_are_not() {
        for (magic, name) in [
            (0x6969, "nfs"),
            (0x517b, "smb"),
            (0xff53_4d42, "cifs"),
            (0xfe53_4d42, "smb2"),
            (0x5346_414f, "afs"),
            (0x00c3_6400, "ceph"),
        ] {
            assert_eq!(network_magic(magic), Some(name));
        }
        // ext4, tmpfs, btrfs, xfs, overlayfs
        for magic in [0xef53, 0x0102_1994, 0x9123_683e, 0x5846_5342, 0x794c_7630] {
            assert_eq!(network_magic(magic), None);
        }
        let temporary = std::env::temp_dir();
        let directory = Directory::open_external(&temporary.canonicalize().unwrap()).unwrap();
        assert_eq!(directory.network_filesystem().unwrap(), None);
    }

    #[test]
    fn mount_roots_are_reported_through_statx() {
        let root = Directory::root().unwrap();
        assert!(root.is_mount_root(c"proc").unwrap());
        let base = crate::test_support::TestDir::new("mount-root");
        std::os::unix::fs::DirBuilderExt::mode(&mut std::fs::DirBuilder::new(), 0o700)
            .create(base.join("child"))
            .unwrap();
        let parent = Directory::private_anchor(&base).unwrap();
        assert!(!parent.is_mount_root(c"child").unwrap());
    }
}
