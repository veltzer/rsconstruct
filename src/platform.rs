//! Unix-specific OS operations, isolated behind named wrappers.
//!
//! `RSConstruct` is unix-only (Linux and macOS — the platforms the release
//! matrix builds). This module exists to keep `std::os::unix` imports and
//! `libc` calls out of the rest of the codebase, not to abstract over a
//! second platform: there are no `#[cfg(not(unix))]` branches here, because
//! there is no non-unix build to serve.
//!
//! The wrappers stay even though they no longer switch on anything — they
//! give the OS-level operations names the call sites can read, and they
//! remain the one place to look if a port is ever attempted.

/// Reset SIGPIPE to default behavior so piping to head/less doesn't cause errors.
pub fn reset_sigpipe() {
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

/// Get the Unix permission mode bits for a file.
pub fn get_mode(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode()
}

/// Whether package-manager invocations should be prefixed with `sudo`.
///
/// Returns false when:
/// - Already running as root (uid 0). sudo is a no-op and may not exist
///   (e.g. inside a bare ubuntu container).
/// - sudo is not on PATH. We can't use it; let the package manager fail
///   on its own with its native "are you root?" message.
///
/// Returns true otherwise (the normal local-dev case: non-root user with
/// passwordless or interactive sudo configured).
pub fn needs_sudo() -> bool {
    !is_root() && which::which("sudo").is_ok()
}

/// Whether the current process runs as root (effective uid 0).
pub fn is_root() -> bool {
    (unsafe { libc::geteuid() }) == 0
}

/// Whether the current user may write to `path` (access(2) with `W_OK`).
/// A path that doesn't exist is "not writable" — callers that care about
/// creatability must check an existing ancestor themselves.
pub fn path_is_writable(path: &std::path::Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(cpath) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    unsafe { libc::access(cpath.as_ptr(), libc::W_OK) == 0 }
}

/// Set an environment variable for this process and everything it spawns.
///
/// `std::env::set_var` is unsafe in edition 2024 because mutating the
/// environment while another thread reads it is a data race. The one caller
/// is the PATH augmentation at the top of `main()`, before any thread —
/// tokio's included — exists. Do not call this after startup.
pub fn set_env(key: &str, value: &std::ffi::OsStr) {
    unsafe { std::env::set_var(key, value) }
}

/// Create a symbolic link to a file.
pub fn symlink_file(original: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(original, link)
}

/// Set file permissions from a Unix mode.
pub fn set_permissions_mode(path: &std::path::Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

/// The parts of a file's status that move when its content may have changed.
///
/// mtime alone is not enough: `cp -p`, `tar x` and `rsync -a` write new
/// content and then set mtime back to the source's. Size catches most such
/// rewrites, the inode catches a file replaced by rename, and ctime — which
/// no user-space call can set, and which every write and every mtime change
/// bumps — catches the rest. This is the set git's index compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileStamp {
    pub mtime_secs: i64,
    pub mtime_nanos: i64,
    pub ctime_secs: i64,
    pub ctime_nanos: i64,
    pub size: u64,
    pub inode: u64,
}

impl FileStamp {
    /// Whether the file's content or status changed within `window_secs`
    /// of `now_secs` (Unix time), judged by the later of mtime and ctime.
    pub const fn changed_within(&self, now_secs: i64, window_secs: i64) -> bool {
        let latest = if self.ctime_secs > self.mtime_secs {
            self.ctime_secs
        } else {
            self.mtime_secs
        };
        now_secs - latest < window_secs
    }
}

/// Read a [`FileStamp`] from already-fetched metadata.
pub fn file_stamp(metadata: &std::fs::Metadata) -> FileStamp {
    use std::os::unix::fs::MetadataExt;
    FileStamp {
        mtime_secs: metadata.mtime(),
        mtime_nanos: metadata.mtime_nsec(),
        ctime_secs: metadata.ctime(),
        ctime_nanos: metadata.ctime_nsec(),
        size: metadata.size(),
        inode: metadata.ino(),
    }
}

/// Register a SIGINT stream. Must be called inside a tokio runtime.
///
/// Registration happens at call time — unlike `tokio::signal::ctrl_c()`,
/// which registers only when its future is first polled — so the caller can
/// signal "handler installed" deterministically and close the startup window
/// where a Ctrl+C would hit the default disposition and kill the process
/// with no cleanup.
pub fn interrupt_signal() -> std::io::Result<tokio::signal::unix::Signal> {
    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
}
