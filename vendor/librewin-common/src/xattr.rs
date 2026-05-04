//! File tagging via xattr (`user.tags`).
//!
//! Enabled with the `tags` Cargo feature:
//!   `librewin-common = { workspace = true, features = ["tags"] }`
//!
//! Tags are stored as a comma-separated string in the `user.tags` extended
//! attribute. This module provides the core read/write logic; the Tauri
//! commands wrapping these functions live in each app's `lib.rs`.

/// Read tags from a file's `user.tags` xattr.
/// Returns an empty vec if the attribute is not set or cannot be read.
pub fn read_tags(path: &str) -> Vec<String> {
    xattr::get(path, "user.tags")
        .ok()
        .flatten()
        .and_then(|v| String::from_utf8(v).ok())
        .map(|s| {
            s.split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Write tags to a file's `user.tags` xattr.
/// Passing an empty slice removes the attribute entirely.
pub fn write_tags(path: &str, tags: &[String]) -> Result<(), String> {
    let value: String = tags
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(",");

    if value.is_empty() {
        match xattr::remove(path, "user.tags") {
            Ok(_) => {}
            Err(e) if e.raw_os_error() == Some(61) => {} // ENODATA — already absent
            Err(e) => return Err(e.to_string()),
        }
    } else {
        xattr::set(path, "user.tags", value.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(())
}
