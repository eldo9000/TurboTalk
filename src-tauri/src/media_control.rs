// Pause/resume media playback when dictation starts/stops.
// Uses MPRemoteCommandCenter from MediaPlayer.framework on macOS via objc2.

#[cfg(target_os = "macos")]
fn send_toggle_play_pause() {
    use block2::Block;
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    #[link(name = "MediaPlayer", kind = "framework")]
    extern "C" {}

    unsafe {
        let cls = objc2::class!(MPRemoteCommandCenter);
        let center: *mut AnyObject = msg_send![cls, sharedCommandCenter];
        let cmd: *mut AnyObject = msg_send![center, togglePlayPauseCommand];
        // sendWithCompletion takes a nullable block (NSError * _Nullable) -> void
        type MpcBlock = Block<dyn Fn(*mut AnyObject)>;
        let nil_block: Option<&MpcBlock> = None;
        let _: bool = msg_send![cmd, sendWithCompletion: nil_block];
    }
}

/// Pause media playback. Call before dictation starts.
/// On macOS toggles play/pause once (pause if playing). No-op on other platforms.
pub fn pause() {
    #[cfg(target_os = "macos")]
    send_toggle_play_pause();
}

/// Resume media playback. Call after dictation finishes.
/// On macOS toggles play/pause once (play if paused). No-op on other platforms.
pub fn resume() {
    #[cfg(target_os = "macos")]
    send_toggle_play_pause();
}
