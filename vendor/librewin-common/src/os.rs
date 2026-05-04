//! OS-level utilities shared across LibreWin apps.

use std::path::PathBuf;

/// Resolve the path to a named binary, checking common install locations
/// before falling back to PATH.
///
/// Checks in order:
///   1. Homebrew Intel  (`/usr/local/bin/<name>`)
///   2. Homebrew ARM    (`/opt/homebrew/bin/<name>`)
///   3. System bin      (`/usr/bin/<name>`)
///   4. System local    (`/usr/local/bin/<name>`)  — already checked above
///   5. PATH via `which`
///
/// Returns `None` if the binary cannot be found anywhere.
///
/// # Example
/// ```
/// use librewin_common::os::resolve_binary;
/// if let Some(path) = resolve_binary("ffmpeg") {
///     println!("ffmpeg at: {}", path.display());
/// }
/// ```
pub fn resolve_binary(name: &str) -> Option<PathBuf> {
    // Explicit locations first (avoids shell overhead, works in restricted envs)
    let candidates = [
        format!("/usr/local/bin/{name}"),
        format!("/opt/homebrew/bin/{name}"),
        format!("/usr/bin/{name}"),
        format!("/home/linuxbrew/.linuxbrew/bin/{name}"),
    ];

    for path in &candidates {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }

    // Fall back to `which` for anything in a non-standard PATH
    let out = std::process::Command::new("which")
        .arg(name)
        .output()
        .ok()?;

    if out.status.success() {
        let s = String::from_utf8(out.stdout).ok()?;
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }

    None
}

/// Returns the current user's home directory.
///
/// Prefers `$HOME`; falls back to `/home/<uid>` on Unix if unset.
pub fn home_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home);
    }
    #[cfg(unix)]
    {
        extern "C" {
            fn getuid() -> u32;
        }
        let uid = unsafe { getuid() };
        PathBuf::from(format!("/home/{uid}"))
    }
    #[cfg(not(unix))]
    PathBuf::from("/home/user")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_dir_is_non_empty() {
        let h = home_dir();
        assert!(!h.as_os_str().is_empty());
    }
}
