/// Cross-platform wrappers for OS-specific operations.
///
/// On Unix, file permissions use mode bits (e.g. 0o644).
/// On Windows, only read-only vs read-write is supported.
///
/// Reset SIGPIPE to default behavior so piping to head/less doesn't cause errors.
/// No-op on Windows (SIGPIPE doesn't exist there).
pub fn reset_sigpipe() {
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

/// Get the Unix permission mode bits for a file.
/// Returns `None` on non-Unix platforms.
#[allow(clippy::unnecessary_wraps)] // Non-unix branch returns None; clippy only sees the unix path.
pub fn get_mode(metadata: &std::fs::Metadata) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Some(metadata.permissions().mode())
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        None
    }
}

/// Set file permissions from a Unix mode. On Unix this sets the exact mode bits.
/// On Windows this approximates by setting read-only when the mode has no owner
/// write bit (i.e. `mode & 0o200 == 0`).
pub fn set_permissions_mode(path: &std::path::Path, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let readonly = mode & 0o200 == 0;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_readonly(readonly);
        std::fs::set_permissions(path, perms)
    }
}
