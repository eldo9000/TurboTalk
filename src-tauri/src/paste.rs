// Active-application text injection.
// macOS: write to clipboard (arboard), send Cmd+V via osascript, restore prior clipboard.
use arboard::Clipboard;

pub fn paste(text: &str) -> anyhow::Result<()> {
    let mut cb = Clipboard::new()?;

    // Save prior clipboard contents (best-effort — ignore if empty/non-text).
    let prior = cb.get_text().ok();

    cb.set_text(text)?;

    // Small delay so the clipboard write is visible to the target app.
    std::thread::sleep(std::time::Duration::from_millis(50));

    let status = std::process::Command::new("osascript")
        .args(["-e", r#"tell application "System Events" to keystroke "v" using command down"#])
        .status()?;

    if !status.success() {
        anyhow::bail!("osascript keystroke failed: {}", status);
    }

    // Restore prior clipboard after a short delay so Cmd+V has time to land.
    std::thread::sleep(std::time::Duration::from_millis(150));
    if let Some(prev) = prior {
        let _ = cb.set_text(prev);
    }

    Ok(())
}
