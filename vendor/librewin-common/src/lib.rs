pub mod config;
pub mod media;
pub mod os;
pub mod window;
#[cfg(feature = "tags")]
pub mod xattr;

/// Read the LibreWin theme preference from the shared config file.
/// Returns "dark", "light", or "system" (default when file absent).
pub fn get_theme() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    std::fs::read_to_string(format!("{}/.config/librewin/theme", home))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "system".to_string())
}

/// Read the LibreWin accent colour from the shared config file.
/// Returns the hex colour string, defaulting to "#0066cc".
pub fn get_accent() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    std::fs::read_to_string(format!("{}/.config/librewin/accent", home))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "#0066cc".to_string())
}
