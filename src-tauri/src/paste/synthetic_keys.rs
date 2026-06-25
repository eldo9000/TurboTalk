use crate::diagnostic_log;

// CGEventSourceCreate and CGEventCreateKeyboardEvent both crash with
// EXC_BREAKPOINT on macOS 26 from ad-hoc signed binaries — the
// CoreGraphics event creation path requires entitlements unavailable
// to this build.  We use osascript instead, which works without extra
// entitlements (same path as focus_capture::activate_app).

pub fn post_cmd_v() {
    diagnostic_log::emergency_trace("[synth] osascript cmd+v");
    let script = "tell application \"System Events\" to keystroke \"v\" using command down";
    match std::process::Command::new("osascript")
        .args(["-e", script])
        .output()
    {
        Ok(out) if out.status.success() => {
            diagnostic_log::emergency_trace("[synth] osascript ok");
        }
        Ok(out) => {
            tracing::warn!(
                "[synthetic_keys] osascript failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Err(e) => {
            tracing::warn!("[synthetic_keys] osascript spawn failed: {e}");
        }
    }
    diagnostic_log::emergency_trace("[synth] done");
}
