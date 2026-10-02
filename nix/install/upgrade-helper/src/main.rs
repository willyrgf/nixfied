//! Private source-edit and file-application boundary for the Nix upgrade app.
//! Directory flock serializes cooperating upgraders. Exchanges are per-file;
//! arbitrary editors, uncatchable termination and crash durability are not CAS.
use rnix::ast::{self, HasEntry};
use rowan::ast::AstNode;
use sha2::{Digest, Sha256};
use std::{
    ffi::{CString, OsString},
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        io::AsRawFd,
    },
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, Instant},
};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
extern "C" fn interrupt(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

#[derive(Debug)]
enum Failure {
    Conflict,
    Operation(io::Error),
    Interrupted,
    Rollback(Vec<PathBuf>),
}
impl From<io::Error> for Failure {
    fn from(e: io::Error) -> Self {
        Self::Operation(e)
    }
}
type Result<T> = std::result::Result<T, Failure>;

fn literal(value: &ast::Str) -> Option<String> {
    value
        .normalized_parts()
        .into_iter()
        .map(|p| match p {
            ast::InterpolPart::Literal(s) => Some(s),
            _ => None,
        })
        .collect()
}
fn attr_name(attr: ast::Attr) -> Option<String> {
    match attr {
        ast::Attr::Ident(i) => Some(i.to_string()),
        ast::Attr::Str(s) => literal(&s),
        _ => None,
    }
}
fn find_url(
    set: ast::AttrSet,
    prefix: &[String],
    found: &mut Vec<ast::Str>,
) -> std::result::Result<(), &'static str> {
    const TARGET: [&str; 3] = ["inputs", "nixfied", "url"];
    for entry in set.attrpath_values() {
        let mut path = prefix.to_vec();
        let attrs = entry.attrpath().ok_or("missing attribute path")?;
        let names = attrs.attrs().map(attr_name).collect::<Option<Vec<_>>>();
        let Some(names) = names else {
            return Err("dynamic attribute names are unsupported in the input scope");
        };
        path.extend(names);
        if path.len() > TARGET.len() || !path.iter().zip(TARGET).all(|(a, b)| a == b) {
            continue;
        }
        match entry.value().ok_or("missing attribute value")? {
            ast::Expr::Str(s) if path.len() == TARGET.len() => {
                literal(&s).ok_or("the nixfied URL must be a literal string")?;
                found.push(s);
            }
            ast::Expr::AttrSet(s) if path.len() < TARGET.len() => find_url(s, &path, found)?,
            _ => return Err("the nixfied input must use literal attribute sets and a literal URL"),
        }
    }
    Ok(())
}
fn rewrite(source: &str, url: &str) -> std::result::Result<String, &'static str> {
    let parsed = rnix::Root::parse(source);
    if !parsed.errors().is_empty() {
        return Err("flake.nix is not valid Nix syntax");
    }
    let Some(ast::Expr::AttrSet(root)) = parsed.tree().expr() else {
        return Err("flake.nix must be a literal attribute set");
    };
    let mut found = Vec::new();
    find_url(root, &[], &mut found)?;
    if found.len() != 1 {
        return Err("expected exactly one literal nixfied input URL");
    }
    let escaped = url
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace("${", "\\${")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    let range = found[0].syntax().text_range();
    let mut result = source.to_owned();
    result.replace_range(
        usize::from(range.start())..usize::from(range.end()),
        &format!("\"{escaped}\""),
    );
    Ok(result)
}

#[derive(Clone, Debug)]
struct Snapshot {
    bytes: Vec<u8>,
    device: u64,
    inode: u64,
    mode: u32,
}
impl Snapshot {
    fn hash(&self) -> String {
        format!("{:x}", Sha256::digest(&self.bytes))
    }
    fn same_file(&self, other: &Self) -> bool {
        self.device == other.device && self.inode == other.inode && self.bytes == other.bytes
    }
}
fn read(path: &Path) -> io::Result<Option<Snapshot>> {
    read_regular(path, libc::O_NOFOLLOW)
}
fn read_regular(path: &Path, flags: libc::c_int) -> io::Result<Option<Snapshot>> {
    let mut file = match OpenOptions::new()
        .read(true)
        .custom_flags(flags | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let before = file.metadata()?;
    if !before.is_file() {
        return Err(io::Error::other("expected a regular file"));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    if before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err(io::Error::other("file changed while reading"));
    }
    Ok(Some(Snapshot {
        bytes,
        device: after.dev(),
        inode: after.ino(),
        mode: after.mode(),
    }))
}
fn expected(path: &Path, hash: &str) -> Result<Option<Snapshot>> {
    let snapshot = read(path)?;
    let actual = snapshot
        .as_ref()
        .map(Snapshot::hash)
        .unwrap_or_else(|| "absent".into());
    if actual != hash {
        return Err(Failure::Conflict);
    }
    Ok(snapshot)
}
// The declaration is observed, never replaced. Preserve the shell app's
// support for a project-owned symlink while rejecting nonregular input reads.
fn expected_project(path: &Path, hash: &str) -> Result<()> {
    let actual = match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => read_regular(path, 0)?
            .map(|snapshot| snapshot.hash())
            .unwrap_or_else(|| "absent".into()),
        Ok(_) => "absent".into(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => "absent".into(),
        Err(error) => return Err(error.into()),
    };
    if actual != hash {
        return Err(Failure::Conflict);
    }
    Ok(())
}
fn unchanged(path: &Path, snapshot: &Snapshot) -> bool {
    matches!(read(path), Ok(Some(current)) if snapshot.same_file(&current))
}
struct DirectoryLock(File);
impl DirectoryLock {
    fn acquire(root: &Path) -> Result<Self> {
        let lock = Self(File::open(root)?);
        loop {
            if INTERRUPTED.load(Ordering::SeqCst) {
                return Err(Failure::Interrupted);
            }
            if unsafe { libc::flock(lock.0.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(lock);
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::WouldBlock {
                return Err(error.into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
fn exchange(first: &Path, second: &Path) -> io::Result<()> {
    let a = CString::new(first.as_os_str().as_bytes()).map_err(io::Error::other)?;
    let b = CString::new(second.as_os_str().as_bytes()).map_err(io::Error::other)?;
    #[cfg(target_os = "linux")]
    let status = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            a.as_ptr(),
            libc::AT_FDCWD,
            b.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    };
    #[cfg(target_os = "macos")]
    let status = unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_SWAP) };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
struct Staged {
    path: PathBuf,
    keep: bool,
}
impl Drop for Staged {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_file(&self.path);
        }
    }
}
impl Staged {
    fn new(root: &Path, bytes: &[u8], mode: u32) -> io::Result<Self> {
        let path = root.join(format!(
            ".nixfied-upgrade.{}.{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        let staged = Self { path, keep: false };
        file.write_all(bytes)?;
        file.set_permissions(fs::Permissions::from_mode(mode))?;
        Ok(staged)
    }
}
struct Replacement {
    destination: PathBuf,
    original: Snapshot,
    candidate: Snapshot,
    staged: Staged,
    applied: bool,
}
impl Replacement {
    fn prepare(
        root: &Path,
        name: &str,
        original: Snapshot,
        candidate: &Path,
    ) -> Result<Option<Self>> {
        let contents = read(candidate)?.ok_or_else(|| io::Error::other("candidate is missing"))?;
        if original.bytes == contents.bytes {
            return Ok(None);
        }
        let staged = Staged::new(root, &contents.bytes, original.mode)?;
        let candidate =
            read(&staged.path)?.ok_or_else(|| io::Error::other("staged candidate is missing"))?;
        Ok(Some(Self {
            destination: root.join(name),
            original,
            candidate,
            staged,
            applied: false,
        }))
    }
    fn apply(&mut self) -> Result<()> {
        if !unchanged(&self.destination, &self.original) {
            return Err(Failure::Conflict);
        }
        exchange(&self.staged.path, &self.destination)?;
        self.applied = true;
        // Never blindly exchange back: rollback first proves the public entry
        // is still ours, and retains displaced evidence if it is not.
        if !unchanged(&self.staged.path, &self.original)
            || !unchanged(&self.destination, &self.candidate)
        {
            return Err(Failure::Conflict);
        }
        Ok(())
    }
    fn rollback(&mut self) -> bool {
        if !self.applied {
            return true;
        }
        if !unchanged(&self.destination, &self.candidate) {
            self.staged.keep = true;
            return false;
        }
        if exchange(&self.staged.path, &self.destination).is_err() {
            self.staged.keep = true;
            return false;
        }
        self.applied = false;
        // A non-cooperating editor racing the exchange must leave its bytes
        // available for recovery rather than having them unlinked as a temp.
        if !unchanged(&self.staged.path, &self.candidate) {
            self.staged.keep = true;
            return false;
        }
        true
    }
}
#[derive(Clone, Copy, PartialEq)]
enum Point {
    BeforeApply,
    AfterExchange,
    AfterLock,
}
fn apply(
    root: &Path,
    hashes: [&str; 3],
    flake: Option<&Path>,
    lock: Option<&Path>,
    mut hook: impl FnMut(Point, &Path) -> Result<()>,
) -> Result<()> {
    let _guard = DirectoryLock::acquire(root)?;
    let originals = [
        expected(&root.join("flake.nix"), hashes[0])?,
        expected(&root.join("flake.lock"), hashes[1])?,
    ];
    expected_project(&root.join("nixfied.nix"), hashes[2])?;
    let mut replacements = Vec::new();
    for (index, name, candidate) in [(1, "flake.lock", lock), (0, "flake.nix", flake)] {
        if let Some(candidate) = candidate {
            let original = originals[index]
                .clone()
                .ok_or_else(|| io::Error::other("replacement requires an existing regular file"))?;
            if let Some(replacement) = Replacement::prepare(root, name, original, candidate)? {
                replacements.push(replacement);
            }
        }
    }
    let outcome = (|| {
        hook(Point::BeforeApply, root)?;
        for replacement in &mut replacements {
            if INTERRUPTED.load(Ordering::SeqCst) {
                return Err(Failure::Interrupted);
            }
            replacement.apply()?;
            hook(Point::AfterExchange, &replacement.destination)?;
            if replacement.destination.file_name() == Some(std::ffi::OsStr::new("flake.lock")) {
                hook(Point::AfterLock, root)?;
            }
        }
        if INTERRUPTED.load(Ordering::SeqCst) {
            return Err(Failure::Interrupted);
        }
        for replacement in &replacements {
            if !unchanged(&replacement.destination, &replacement.candidate) {
                return Err(Failure::Conflict);
            }
        }
        for (index, name) in ["flake.nix", "flake.lock"].iter().enumerate() {
            if !replacements
                .iter()
                .any(|r| r.destination == root.join(name))
            {
                expected(&root.join(name), hashes[index])?;
            }
        }
        expected_project(&root.join("nixfied.nix"), hashes[2])?;
        Ok(())
    })();
    if let Err(failure) = outcome {
        let mut retained = Vec::new();
        for replacement in replacements.iter_mut().rev() {
            if !replacement.rollback() {
                retained.push(replacement.staged.path.clone());
            }
        }
        if !retained.is_empty() {
            return Err(Failure::Rollback(retained));
        }
        return Err(failure);
    }
    Ok(())
}
fn install_signals() -> io::Result<()> {
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = interrupt as *const () as usize;
        unsafe {
            libc::sigemptyset(&mut action.sa_mask);
        }
        if unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}
fn report(failure: Failure) -> i32 {
    match failure {
        Failure::Conflict => {
            eprintln!(
                "upgrade aborted: project files changed concurrently; candidate was rolled back"
            );
            6
        }
        Failure::Operation(error) => {
            eprintln!("failed to apply candidate; candidate was rolled back: {error}");
            7
        }
        Failure::Interrupted => {
            eprintln!("upgrade interrupted; candidate rolled back");
            130
        }
        Failure::Rollback(paths) => {
            eprintln!(
                "upgrade rollback failed; concurrent changes were preserved; inspect the project files"
            );
            for path in paths {
                eprintln!("preserved recovery file: {}", path.display());
            }
            8
        }
    }
}
fn run(args: &[OsString]) -> i32 {
    match args.get(1).and_then(|x| x.to_str()) {
        Some("rewrite") if args.len() == 5 => {
            let result = fs::read_to_string(&args[2])
                .map_err(|_| "cannot read flake.nix")
                .and_then(|source| rewrite(&source, args[4].to_str().ok_or("URL is not UTF-8")?))
                .and_then(|rewritten| {
                    fs::write(&args[3], rewritten).map_err(|_| "cannot write staged flake.nix")
                });
            match result {
                Ok(()) => 0,
                Err(message) => {
                    eprintln!(
                        "{message}\nNo files were changed.\nRefusing to guess which input pin to rewrite."
                    );
                    3
                }
            }
        }
        Some("apply") if args.len() == 8 => {
            let hashes = [args[3].to_str(), args[4].to_str(), args[5].to_str()];
            let [Some(a), Some(b), Some(c)] = hashes else {
                return 2;
            };
            let flake = (!args[6].is_empty()).then(|| Path::new(&args[6]));
            let lock = (!args[7].is_empty()).then(|| Path::new(&args[7]));
            let result = install_signals().map_err(Failure::from).and_then(|()| {
                apply(Path::new(&args[2]), [a, b, c], flake, lock, |point, _| {
                    if point == Point::AfterLock
                        && flake.is_some()
                        && let Ok(seconds) = std::env::var("NIXFIED_UPGRADE_TEST_PAUSE_AFTER_LOCK")
                    {
                        let duration = seconds
                            .parse::<u64>()
                            .map_err(|_| io::Error::other("invalid test pause"))?;
                        eprintln!("test pause after lock apply");
                        let start = Instant::now();
                        while start.elapsed() < Duration::from_secs(duration) {
                            if INTERRUPTED.load(Ordering::SeqCst) {
                                return Err(Failure::Interrupted);
                            }
                            std::thread::sleep(Duration::from_millis(20));
                        }
                    }
                    Ok(())
                })
            });
            match result {
                Ok(()) => 0,
                Err(failure) => report(failure),
            }
        }
        _ => {
            eprintln!("invalid private upgrade-files invocation");
            2
        }
    }
}
fn main() {
    std::process::exit(run(&std::env::args_os().collect::<Vec<_>>()));
}

#[cfg(test)]
mod tests;
