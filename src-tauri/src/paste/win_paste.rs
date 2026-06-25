use super::win_focus;

use super::win_clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

/// Full Windows paste sequence:
/// 1. Save clipboard (full format snapshot)
/// 2. Write transcript text to clipboard
/// 3. Activate the foreground window
/// 4. Send Ctrl+V via enigo
/// 5. Restore clipboard from snapshot
///
/// Returns Ok(true) on success.
pub fn paste(text: &str) -> anyhow::Result<bool> {
    // 1. Save clipboard snapshot (best-effort — continue on failure)
    let snapshot = match win_clipboard::snapshot() {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::warn!("[win_paste] clipboard snapshot failed (continuing): {e}");
            None
        }
    };

    // 2. Write transcript text to clipboard
    if let Err(e) = win_clipboard::write_text(text) {
        tracing::warn!("[win_paste] write_text failed: {e}");
    }

    // 3. Activate the foreground window
    let hwnd = win_focus::foreground_hwnd();
    if hwnd != 0 {
        if let Err(e) = win_focus::activate_hwnd(hwnd) {
            tracing::warn!("[win_paste] activate_hwnd failed (continuing): {e}");
        }
    }

    // 4. Send Ctrl+V via enigo
    //
    // We use Key::Layout('v') (virtual-key code) rather than Key::Unicode('v')
    // (WM_CHAR) because Ctrl+V is a keyboard accelerator that dispatches on
    // VK_V (0x56), not a character event.
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| anyhow::anyhow!("enigo init failed: {e}"))?;

    enigo
        .key(Key::Control, Direction::Press)
        .map_err(|e| anyhow::anyhow!("enigo Ctrl press failed: {e}"))?;

    // If the 'v' key fails, release Ctrl before propagating the error so the
    // modifier is never left stuck.
    let v_result = enigo.key(Key::Layout('v'), Direction::Click);
    if let Err(e) = v_result {
        let _ = enigo.key(Key::Control, Direction::Release);
        return Err(anyhow::anyhow!("enigo 'v' failed: {e}"));
    }

    enigo
        .key(Key::Control, Direction::Release)
        .map_err(|e| anyhow::anyhow!("enigo Ctrl release failed: {e}"))?;

    // Small delay for paste to land
    std::thread::sleep(std::time::Duration::from_millis(100));

    // 5. Restore clipboard from snapshot (with sequence-number guard)
    if let Some(snapshot) = snapshot {
        match win_clipboard::restore(&snapshot) {
            Ok(true) => tracing::info!("[win_paste] clipboard restored"),
            Ok(false) => tracing::warn!(
                "[win_paste] clipboard changed during paste, restore skipped"
            ),
            Err(e) => tracing::warn!("[win_paste] clipboard restore failed: {e}"),
        }
    }

    Ok(true)
}
