// Pause/resume media playback when dictation starts/stops.
// Uses MPRemoteCommandCenter from MediaPlayer.framework.

#[cfg(target_os = "macos")]
fn send_toggle_play_pause() {
    use block2::Block;
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    #[link(name = "MediaPlayer", kind = "framework")]
    extern "C" {}

    let result = std::panic::catch_unwind(|| unsafe {
        let cls = objc2::class!(MPRemoteCommandCenter);
        let center: *mut AnyObject = msg_send![cls, sharedCommandCenter];
        let cmd: *mut AnyObject = msg_send![center, togglePlayPauseCommand];
        let nil_block: Option<&Block<dyn Fn(*mut AnyObject)>> = None;
        let _: bool = msg_send![cmd, sendWithCompletion: nil_block];
    });

    if let Err(e) = result {
        tracing::debug!("[media_control] toggle failed: {:?}", e);
    }
}

/// Pause media playback. Call before dictation starts.
pub fn pause() {
    #[cfg(target_os = "macos")]
    send_toggle_play_pause();
}

/// Resume media playback. Call after dictation finishes.
pub fn resume() {
    #[cfg(target_os = "macos")]
    send_toggle_play_pause();
}
