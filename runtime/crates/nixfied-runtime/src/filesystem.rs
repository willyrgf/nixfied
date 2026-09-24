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
