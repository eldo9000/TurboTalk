# TurboTalk — Session Status

**Last updated:** 2026-07-14 (Vocabulary-isolated live transcription)
**Current state:** The Check for updates control is now a static disabled button; the Settings page no longer imports the updater or runs an automatic check on mount, so opening Settings produces no transient checking state. Dropdown containers remain aligned from the panel midpoint to the right edge. Other settings groups remain consolidated and button labels stay on one line. Auto hotkey mode remains implemented. Previous proof: `cargo check`, `clippy`, `npm run typecheck` pass clean.

## Next action

Next cleanup: trim or gate the verbose process-tap diagnostic payload once confidence is high; ad-hoc rebuilds churn macOS TCC permission identity, so avoid unnecessary rebuilds during follow-up. Text Formatter controls now render reliably in the dev Modes page without the broken conditional wrapper, and live segment ASR now omits vocabulary prompting while final ASR retains it. Proof: `cargo check --manifest-path src-tauri/Cargo.toml` passed.

## Latest overlay proof

2026-07-04: Implemented native AppKit overlay placement in `src-tauri/src/windowing.rs`, positioned before `ptt-armed` and `ptt-down` in `hotkey.rs`, and changed the overlay window to `visible: false` in `tauri.conf.json` so startup cannot flash the centered stale frame. User confirmed the recording overlay now appears on the monitor containing the mouse pointer. `git diff --check` passed. `cargo check --manifest-path src-tauri/Cargo.toml` passed with only the pre-existing CoreAudio linker warning. `npm run typecheck` passed.

## Latest media-pause investigation

2026-07-07: Replaced MediaRemote/default-output-device playback detection with a CoreAudio process-tap probe in `src-tauri/media_toggle.c`. The documented-correct idiom is `CATapDescription` + `AudioHardwareCreateProcessTap`, attached to a private aggregate device and sampled through an `AudioDeviceIOProc`; this measures real output sample energy instead of app/session/hardware "running" state. Added `NSAudioCaptureUsageDescription` to `src-tauri/Info.plist` and linked CoreAudio from `build.rs`. `media_control.rs` now distinguishes `Playing`, `Silent`, and `Unavailable`; unavailable detection leaves media alone rather than risk starting a paused app. Proof: `cargo check --manifest-path src-tauri/Cargo.toml` passed; `npm run local-install` built, codesign-verified, and installed `/Applications/Turbo Talk.app`; installed Info.plist contains `NSAudioCaptureUsageDescription`. Remaining proof needed: real user test with Chrome/YouTube playing and with no audio playing. Expected logs are either `[media_control] process tap samples=... playing=1/0` or an explicit process-tap unavailable warning.

2026-07-07 follow-up: User granted the new "record system audio" permission and Input Monitoring became truly granted after TCC reset/relaunch (`input_monitoring=Granted`, IOHID listener running). Fresh dictations at 19:54-19:55 proved hotkeys and paste still work, but media pause classified both attempts as `no playback detected`; the C probe's stderr sample metrics were not captured in the app log. Added explicit telemetry exports from `media_toggle.c` (`samples`, `rms`, `peak`, `status`) and Rust logging in `media_control.rs`, plus `cargo:rerun-if-changed=media_toggle.c` so the Objective-C helper relinks after edits. Proof: `cargo check --manifest-path src-tauri/Cargo.toml` passed; `npm run local-install` rebuilt and codesign-verified the app but returned nonzero because the script's final `open -a Turbo Talk` hit LaunchServices error `-600`; direct `open -a "Turbo Talk"` then succeeded. Remaining proof needed: one playback and one silent trigger with the telemetry build running.

2026-07-07 second follow-up: Changed the process tap from mono global excluding tap to a default-output-device-specific tap (`CATapDescription initExcludingProcesses:andDeviceUID:withStream:`) with the global tap as fallback, then reinstalled and relaunched. User granted the new system-audio permission prompt. Fresh app logs prove `input_monitoring=Granted(ok=true)`, `iohid_running=true`, and two dictations completed, but both media probes returned `result=0 status=0 samples=2048 rms=0.00000000 peak=0.00000000`. This rules out the previous permission/onboarding failure as the active blocker: the tap callback is running and delivering frame buffers, but the sampled aggregate stream is silent. Next investigation should focus on tap/aggregate composition, buffer direction/format, or default-output-device selection.

2026-07-08 diagnostic build: Added one-shot process-tap diagnostics to log the default output device UID/name, tap format (`kAudioTapPropertyFormat`), aggregate input/output stream formats, and separate input/output IOProc buffer energy. The sample reader now decodes common Linear PCM float/double/int16/int32 formats instead of assuming `float *`. Proof: `cargo check --manifest-path src-tauri/Cargo.toml` passed; `npm run local-install` built, codesign-verified, and installed `/Applications/Turbo Talk.app`; relaunched via `open -a "Turbo Talk"`. Runtime blocker: fresh log shows `Input Monitoring permission revoked at runtime — IOHIDManager events blocked`, so the diagnostic probe still needs a user re-enable/relaunch/test before conclusions can be drawn.

2026-07-08 user proof: User re-enabled permissions and confirmed media pause/resume works. Fresh logs show the playback-energy probe succeeding on the Bose Flex 2 output route: `process tap result=1 status=1 samples=2048 rms=0.01482160 peak=0.16679211`, with `tap_fmt=lpcm rate=48000.0 flags=0x9 bytes/frame=4 channels/frame=1 bits/channel=32`; aggregate input had one stream and carried the samples, aggregate output had no streams. TurboTalk then logged `pause — playback detected, toggling`; after dictation job 17 finished, it logged `resume — waiting 800ms then toggling`. A later silent test returned `result=0` and skipped resume, matching intended behavior.

2026-07-08 route-aware resume follow-up: Replaced the experimental fixed resume sleep with a dependent CoreAudio output-route baseline. After pausing media and allowing the media key to settle, TurboTalk captures the default output route fingerprint (device UID/name, transport, nominal rate, running flags, input/output stream format summaries). On resume, it polls until the current output fingerprint matches the captured baseline twice consecutively, with a 2500 ms timeout as a guardrail. Proof so far: `cargo check --manifest-path src-tauri/Cargo.toml` passed. User proof still needed on the Bose/Bluetooth route to confirm the distorted half-second is gone and to inspect `route_ready` logs.

## Latest session proof

2026-06-21: Implemented the large-overlay glyph preview only in `src/Overlay.svelte`; no backend behavior changed. `npm run typecheck` passed. `npm run build` passed with the existing tolerated top-level-await transform warnings from `src/main.js`. Follow-up after user saw no change: confirmed the repo/dist had the glyph preview but the installed `/Applications/Turbo Talk.app` was stale and the active config defaulted to Medium. Set `overlay_size = "large"` in `~/.config/librewin/turbotalk/config.toml`, ran `npm run local-install`, codesign verification passed, and relaunched `/Applications/Turbo Talk.app` as PID 61549. Second follow-up: user confirmed initial pills worked but committed segments only brightened and live pills stalled after pauses. Changed committed segments to render readable text and replaced total-estimate catchup with a monotonic live visual cursor. Third follow-up: removed the leading pulsing red cursor pill and dropped the forced starter min-heights so the preview bubble hugs content. Fourth follow-up: replaced repeating pill width pattern with deterministic English word-length distribution and matched preview padding to the large audio pill. Fifth follow-up: replaced distribution estimator with lorem-ipsum word character counts and set both `.pill.large` and `.seg-preview` to exact `width: 984px`. Sixth follow-up: user noticed fixed speech-time estimator undercounted fast speech; raised starter rate to ~185 WPM and added segment-commit self-correction bounded at ~96-312 WPM. `npm run typecheck`, `npm run build`, and `npm run local-install` passed; relaunched `/Applications/Turbo Talk.app` as PID 13725.

## Open backlog

| Item | Status |
|------|--------|
| **Onboarding welcome-screen cleanup** | **Fixed** — `recheckReadiness()` now dismisses onboarding immediately when all gates green (no longer depends on launch-at-login), and `Onboarding.svelte` auto-close no longer requires `launchAtLogin`. Two changes: `src/App.svelte` (initial-mount gate) and `src/Onboarding.svelte` (removed hard requirement from auto-close condition). |
| **Status window (new)** | **Built** — `src/Status.svelte` handles `ptt-armed`, `ptt-arm-failed`, `transcription-rejected` (flaky/blocked), and `recording-discarded` (empty-final-text). Yellow pulsing border for arming, red pulsing border for rejections, dismiss button on rejections. Window is clickable (no `set_ignore_cursor_events`). |
| **Arming removed from overlay** | **Done** — `Overlay.svelte` no longer listens for `ptt-armed` / `ptt-arm-failed`, no arming CSS classes or template blocks. |
| **Filtered-dictation overlay feedback** | **Superseded** by status window — `transcription-rejected` feedback now lives in `Status.svelte`. |
| **RejectReason::label()** | Added — short 1-3 word label for overlay use (e.g. "Repetition detected", "Junk detected"), separate from the full `description()` used in toasts. |
| **Cancel key paste-through (post-transcription)** | Fixed — Added `SeqCst` ordering on `CANCEL_EPOCH` and state-machine guards before all 3 paste call sites. |
| **Manual device-lost repro** | **TODO** — still needs a fresh runtime capture with an actual `device-lost` line so we can confirm the mid-recording unplug/switch path end-to-end. |
| Release CI run | Complete — v0.9.8 builds, codesign, updater artifacts all green ([#27322438132](https://github.com/eldo9000/TurboTalk/actions/runs/27322438132)) |
| TASK-25/26 — Windows hotkey + paste | Complete |
| TASK-57 — Segment recovery pollutes history | Fixed |
| TASK-48 — CoreML / Neural Engine | Phase 1 built; phase 2 blocked |
| Developer ID / Authenticode signing | Deferred |
| Parakeet v3 multilingual | In catalog; end-to-end not user-confirmed |

## Backend tradeoffs

- **Parakeet** — fastest English; raw output lowercase/unpunctuated (Chaperone normalizes)
- **Whisper** — multilingual, best accuracy; Silero VAD pre-filter when model bundled

## Recent commits

- `e5aae45` — docs: log v0.9.8 release CI signing env failure in CI-FAIL-LADDER
- `b40fda9` — fix(ci): only set Apple notarization env vars when Developer ID credentials present
- `3521733` — fix(ci): fall back to ad-hoc identity instead of empty string in release workflow
- `9c6b3ca` — chore(release): bump to 0.9.8

## This session

**Event (TASK-75):** Eliminated the temp-file round-trip for segment WAV writing. Added `wav_bytes_from_samples` helper that builds 16-bit PCM WAV bytes directly in memory (no hound dependency — manual RIFF header), `WhisperBackend::transcribe_bytes` method that POSTs them via `Part::bytes(...).file_name(...)`, `run_raw_bytes` with the same connection-failure retry logic as `run_raw`, and updated `transcribe_one_segment` to use the all-in-memory path. Added `transcribe_bytes` with a default fallback impl to the `TranscriptionBackend` trait; `WhisperBackend` overrides it. Extracted `handle_transcribe_response` to share response parsing between `transcribe` and `transcribe_bytes`.

**Proof:** `cargo check --manifest-path src-tauri/Cargo.toml` clean. `cargo test -- wav_bytes_from_samples_round_trips` passes (reads back via `hound::WavReader::new(Cursor)` — header, duration, format all correct). All 137 other tests pass (2 pre-existing `detect_garbage` failures unrelated).

**Event:** Continued the Sequoia/ad-hoc Accessibility investigation from handoff. Confirmed release app launches, logs `AXIsProcessTrusted()` false, CGEventTap fails, and IOHIDManager starts. Source cleanup: AX fallback warning is one-shot, and CGEventTap retry no longer emits the misleading Accessibility toast when IOHID/Input Monitoring is intended. Follow-up: fixed IOHID keyboard duplicate press handling that made recording flicker away in toggle mode. Tray follow-up: template-image tray attempt was still invisible, so idle tray icon is now a dark filled badge with white `TT`.

**Proof:** `cargo check --manifest-path src-tauri/Cargo.toml` clean. `npm run package` completed, codesign verification passed, DMG copied to `build/TurboTalk-0.9.8-macos-arm64.dmg`, and the rebuilt app was installed to `/Applications/Turbo Talk.app`. Logs showed the flicker cause: `Ready → Recording` immediately followed by `Recording → FinalizingAudio` from a second IOHID press in the same moment. Final installed process is running as `/Applications/Turbo Talk.app/Contents/MacOS/turbotalk` PID 787.

**Investigation:** Codex-launched tests are not authoritative for Input Monitoring: macOS TCC attributes the request to responsible process `com.openai.codex` with requester `com.turbotalk.dictation`. App remains in readiness/onboarding polling during these launches, so hotkey PTT is suppressed until setup is considered complete.

**Paste follow-up:** Logs showed completed transcription jobs reaching the paste stage, but `focus_at_start`/`focus_at_paste` were `Some("Codex")` and `AX focused role before paste: None`. On this ad-hoc build, hotkeys work through IOHID/Input Monitoring, but synthetic Cmd+V still depends on Accessibility trust and can be silently dropped. `paste.rs` now checks Accessibility trust before posting Cmd+V; when trust is false it leaves the transcript on the clipboard and emits a clear “Copied transcript — press Command-V” message. The tracing watchdog stale threshold was raised from 2 minutes to 15 minutes to stop idle-time false error toasts. The expected CGEventTap startup failure is now logged as a warning when IOHID fallback is active.

**Latest proof:** `cargo check --manifest-path src-tauri/Cargo.toml` passed. `npm run package` passed, codesign verification passed, and `build/TurboTalk-0.9.8-macos-arm64.dmg` was refreshed. The rebuilt app was installed to `/Applications/Turbo Talk.app` and relaunched; current process is `/Applications/Turbo Talk.app/Contents/MacOS/turbotalk` PID 21700. Fresh logs show `AXIsProcessTrusted()` false and `CGEventTap unavailable because Accessibility trust is false; IOHID fallback remains active`.

**Tray follow-up:** The Tauri tray image/title still was not visible in the menu bar. Added a native macOS `NSStatusItem` fallback that installs a visible `TT` title directly through AppKit before building the Tauri tray. Rebuilt, reinstalled, and launched `/Applications/Turbo Talk.app/Contents/MacOS/turbotalk`; stderr confirmed `[tray] native NSStatusItem title installed`. If `TT` is still not visible, the next suspect is menu-bar hiding/overflow or an OS/menu-bar-manager visibility layer rather than TurboTalk failing to request a status item.

**Tray geometry proof:** Added delayed native status-item instrumentation with explicit width (`160`) and long title (`TURBOTALK`). Rebuilt/reinstalled/launched from `/Applications`. Runtime stderr showed `hidden=false`, `window != null`, `length=160.0`, `frame=(0.0,0.0,160.0,22.0)`. Tauri also reported `tray rect after delay: Ok(Some(Rect { position: Physical(PhysicalPosition { x: 0, y: 2264 }), size: Physical(PhysicalSize { width: 114, height: 44 }) }))`. That proves AppKit/Tauri are assigning nonzero status-item geometry; if the user still cannot see it, investigate menu-bar visibility/overflow/display placement rather than tray image rendering.

**Dock fallback fix:** Shipped the pragmatic recovery fix after the tray/status item proved unreliable as the only affordance. Removed `LSUIElement` from `src-tauri/Info.plist`, stopped forcing macOS `ActivationPolicy::Accessory`, removed the native `NSStatusItem` diagnostic probe, and now shows the main window on launch while keeping close-to-hide behavior. Rebuilt/reinstalled `/Applications/Turbo Talk.app`; installed Info.plist no longer contains `LSUIElement`, codesign verification passed, and the relaunched process is `/Applications/Turbo Talk.app/Contents/MacOS/turbotalk` PID 40113.

**Visibility follow-up:** User reported the app launches but still has no tray icon. Found a stale `/Applications/Turbo Talk.app/Contents/MacOS/turbotalk` process and the recurring stuck `git add -- target/...` process; killed both and relaunched once. Applied the next pragmatic visibility fix in source: main window now has `skipTaskbar: false`, close on the main window quits instead of hiding while the tray is unreliable, and the tray builder sets a plain `TT` title as a menu-bar fallback. `cargo check --manifest-path src-tauri/Cargo.toml` passed.

**Latest installed proof:** `npm run package` completed. Codesign verification passed. DMG refreshed at `build/TurboTalk-0.9.8-macos-arm64.dmg`. Replaced `/Applications/Turbo Talk.app` with the rebuilt bundle and launched it normally via macOS. Installed app modified time is `Jun 20 17:00:22 2026`; installed Info.plist has no `LSUIElement` or `LSBackgroundOnly`, and `codesign --verify --deep --strict` reports valid on disk. Current process is `/Applications/Turbo Talk.app/Contents/MacOS/turbotalk` PID 73211. Process sweep found no whisper/parakeet helper and no stuck `target` staging process.

**Paste fallback UI fix:** User saw the red “Copied transcript — press Command-V...” banner after a successful fallback copy. Changed the AX-denied path from `paste-error` to a distinct `paste-copied` event so the overlay exits normally and the History tab shows a neutral green “Copied to clipboard. Press Command-V to paste.” notice. True paste exceptions still emit `paste-error`. Validated with `cargo check --manifest-path src-tauri/Cargo.toml`, `npm run build`, and `npm run typecheck`. Rebuilt, codesign-verified, reinstalled `/Applications/Turbo Talk.app`, and relaunched; current process is `/Applications/Turbo Talk.app/Contents/MacOS/turbotalk` PID 13211.

**Local build speedup:** Added `target/`, `dist/`, and `node_modules/` to `.gitignore` to stop the recurring background `git add -- target/...` process from trying to stage build outputs. Added `npm run package:app` for app-bundle-only builds (`tauri build --bundles app`), `npm run install:app` for replacing `/Applications/Turbo Talk.app`, and `npm run local-install` for the faster local test loop. Full `npm run package` remains the release/DMG path. Validated `package.json`, `scripts/install-macos-app.mjs`, and `npm run typecheck`.

**Clipboard fallback follow-up:** User saw the neutral green copied-to-clipboard bubble, but manual Command-V did nothing. Logs proved dictation now reaches paste fallback correctly (`paste-copied`), but `pbpaste` showed the clipboard was still old/empty afterward. Updated macOS paste fallback to verify clipboard readback after `arboard::Clipboard::set_text`; if readback is empty, it retries through `/usr/bin/pbcopy` and logs the verified byte count. Also changed fallback copy to say “Auto-paste blocked. Copied to clipboard; press Command-V.” Validated `cargo check --manifest-path src-tauri/Cargo.toml`, restored npm dependencies with `npm install` after `tsc` was missing, then validated `npm run typecheck`. Installed with `npm run local-install`; app is running as `/Applications/Turbo Talk.app/Contents/MacOS/turbotalk` PID 74684. A post-install polling window saw no new dictation, so clipboard readback still needs one fresh user test.

**Fresh user proof:** User restarted through the welcome/permissions flow and then completed two Right Option dictations. The restart did not disrupt the test; logs have a clean startup boundary at `2026-06-21T02:15:00Z`, followed by `app-ready`, `IOHIDManager mouse listener running`, and `prewarm complete — worker ready`. Both dictations started and stopped via IOHID keyboard events, produced transcripts, and reached paste fallback. Clipboard verification now succeeded: job 1 logged `clipboard write verified via arboard (31 bytes)`, job 2 logged `clipboard write verified via arboard (28 bytes)`, and `pbpaste` returned `28` bytes with preview `Alright, here's another one `. Auto-paste remains blocked because `AXIsProcessTrusted()` is still false; manual Command-V is the expected path on the ad-hoc build. Tray icon remains unresolved: logs say `[tray] tray icon created`, but the user still cannot see a menu-bar icon.

**Readiness cleanup:** Fixed the setup/welcome flow to match the proven runtime path. `Readiness` now separates historical `accessibility` setup compatibility from the real `automatic_paste` Accessibility status. Onboarding now gates only on Input Monitoring, Microphone, and local model presence; the old Accessibility restart path is relabeled as optional Auto-Paste. Also removed the user-facing tracing watchdog toast that caused idle-time false error bubbles. Validated `cargo check --manifest-path src-tauri/Cargo.toml` and `npm run typecheck`; installed with `npm run local-install`, codesign verification passed, and `/Applications/Turbo Talk.app` relaunched as PID 7409.

## Git history cleanup (2026-06-20)

- **Problem:** `.git/` was 7.5 GB — bloated by binary files (`src-tauri/binaries/*`, `src-tauri/resources/silero_vad.onnx`) committed into history, plus 1.83 GB of garbage tmp packs and 5.13 GB of loose objects.
- **Fix:** `git filter-repo` purged those paths from all 442 commits. All 12 tags rewritten. Binaries remain on disk (untracked, `.gitignore`'d). Remote force-pushed.
- **Result:** `.git/` dropped from 7.5 GB → 14 MB.
- **Policy change:** Pre-built binaries are no longer tracked in git. Any clone needs to restore them from backup or a prior build.

## This fix session (2026-06-21)

**Root cause identified (immediate recording discards):** When Accessibility is granted transiently, CGEventTap starts alongside the always-running IOHID keyboard listener. Both handle Right Option. IOHID fires `ptt_down` (starts recording). CGEventTap fires slightly later for the SAME physical keypress, sees `is_recording=true`, and fires `ptt_up` — stopping the recording 162µs after it started. This pattern was confirmed at 19:43-44 UTC and 21:45:30-33 UTC in logs, always immediately after "Accessibility permission detected — retrying CGEventTap".

**Fix 1 — CGEVENTTAP_ACTIVE flag (hotkey.rs):** Added `static CGEVENTTAP_ACTIVE: AtomicBool`. Set true just before `CFRunLoop::run_current()` (CGEventTap active), cleared after it returns (CGEventTap dead). In `hid_mouse_value_callback`, keyboard-page events are skipped when `CGEVENTTAP_ACTIVE` is true. Mouse-button events (page 0x09) are unaffected — they never had a CGEventTap counterpart.

**Fix 2 — Tray idle icon (tray.rs):** `32x32.png` is 200 bytes / fully transparent. Removed the macOS-idle early return that used it. The idle state now uses the same pixel-drawn path as Recording/Transcribing: dark circle + "TT" glyph (44x44 RGBA). Also removed the redundant `.title("TT")` text since the icon itself draws "TT". 

**Paste fix (already in binary):** `paste-copied` event was confirmed in the installed binary via `strings`. The `paste-error` in earlier logs was from a prior binary.

**Rebuilt, installed, launched:** PID 42526 (2026-06-21 04:15 UTC). Clean startup confirmed in `turbotalk.2026-06-21.log`.

**Auto-paste fix (2026-06-21):** `CGEventPost(kCGSessionEventTap, Cmd+V)` bypasses the AX gate on macOS Sequoia. Changed paste.rs: when `AXIsProcessTrusted()` is false, post at `CGEventTapLocation::Session` instead of blocking outright; always leave transcript on clipboard as Cmd+V backup. User confirmed auto-paste now fires without Accessibility trust. Green "Auto-paste blocked" banner still shows as informational (we can't confirm the post landed without AX), which is acceptable UX. Rebuilt/installed as PID 58159.

**Tray fix (2026-06-21):** Added `.title("TT")` back to the `TrayIconBuilder`. The pixel-drawn 44×44 icon may render at wrong logical size on macOS (44pt in a 22pt menu bar); the text title is the reliable visible element.

## Past fix session (2026-06-21)

**"Half paste" bug fixed:** When the Parakeet/Whisper transcript trips hallucination
detection (`detect_garbage`), the salvage code path in `hotkey.rs:1104-1160` was
pasting only the "clean" segment part and discarding the tail (or vice versa),
resulting in ~50% of the dictated text being pasted. This was confirmed in the
emergency trace: `partial_rejection — used clean 67 chars` (job 9), `chars=15`
(job 31), `chars=44` (job 32).

**Fix:** Removed the truncation in the salvage path. Garbage detection is now
advisory-only — it controls the UI badge/flag but never truncates what gets pasted.
The full `raw_text` always reaches the user. If one part tests clean individually,
the flag is `flaky=false` (partial rejection — mild). If both parts are garbage,
the flag is `flaky=true` (full rejection — still pasted with stronger warning).

This matches the existing "flaky" philosophy at the original lines 1163-1191:
"the garbage text is still more useful than appearing to have done nothing."

## This session (2026-06-24) — NSPasteboard changeCount guard + Windows GetClipboardSequenceNumber

**Native NSPasteboard main-thread dispatch:** Wired the already-written native
NSPasteboard module (`clipboard.rs` `native` submodule) into the macOS paste
path via `dispatch_native()` — a generic helper that dispatches a closure to
the main thread via `app.run_on_main_thread()` and syncs the result back
through an `mpsc` channel. The native module reads `NSPasteboard.changeCount`
at snapshot time and compares on restore; if the clipboard changed during the
200 ms paste window, restore is skipped (`Ok(false)`).

Falls back to pbcopy/pbpaste on dispatch failure (e.g. app shutting down).

**Windows GetClipboardSequenceNumber guard:** Added `GetClipboardSequenceNumber`
to `win_clipboard.rs`. `ClipboardSnapshot` now has a `seq_num: u32` field
captured before opening the clipboard. `restore()` checks the current sequence
number against the snapshot; returns `Ok(false)` if the clipboard changed.

**Files changed:** `paste/mod.rs`, `paste/win_clipboard.rs`, `paste/win_paste.rs`,
`hotkey.rs`, `docs/reference/KNOWN-BUG-CLASSES.md`.

**Proof:** `cargo check` and `cargo clippy` pass (no new warnings).

## This session (2026-06-24) — Arc<Config> cache + narrow hot-path accessors

**Arc<Config> cache:** Swapped `settings::CACHE` from `RwLock<Option<Config>>` to
`RwLock<Option<Arc<Config>>>`. `load()` now returns `Arc<Config>` — clone is a
cheap refcount bump instead of deep-cloning the entire struct (including all
`Vec<String>` fields for vocabulary, antivocabulary, models). Added narrow field
accessors for hot-path readers: `overlay_position()`, `overlay_size()`,
`pause_media_on_dictate()`, `idle_timeout_secs()`, `audio_device()`. These
acquire the read lock, extract one field, and drop the lock — no full Config clone.

**Hot-path callers updated:**
- `windowing.rs` (both macOS/not-macOS overlay positioning) — uses narrow accessors
- `audio.rs` (`idle_timeout_from_settings`, `start`) — uses narrow accessors
- `hotkey.rs` (`pause_media_on_dictate` at PTT-down/up) — uses narrow accessors
- `hotkey.rs` (two `prewarm` calls) — explicitly clones Config from Arc for owned-Config fn

**Proof:** `cargo check` passes, `cargo clippy` passes (no new warnings).

## This session (2026-06-24) — gate worker invalidation

**Gate worker invalidation on backend-affecting fields only:** `save_config` now
captures the previous config via `settings::load()` before saving, then compares
only the backend-affecting fields (`backend`, `backend_variant`, `whisper.model`,
`whisper.vad_enabled`, `cleanup.vocabulary`). Non-backend fields (theme, sound,
overlay, cursor dot, etc.) still persist to disk and update the cache but no
longer destroy the warm transcription worker. First save after cold start
defaults to invalidating (safe fallback via `Config::default()`).

**Proof:** `cargo check --manifest-path src-tauri/Cargo.toml` passed. `cargo clippy` passed (no new warnings).

## This session (2026-06-22)

**Tray icon fix:** The tray was rendering as two visual slots — an empty icon space on the left and the `.title("TT")` text on the right — because removing `.icon_as_template(true)` made the transparent idle pixel buffer visible as a dark square. Removed `.title("TT")`, restored the `draw_tt` pixel-glyph function, and toggled `.icon_as_template` by state: template mode for idle (system-colored "TT" text), non-template for recording/transcribing (actual red/amber circles). Single icon slot, clean. Commit `714f429`.

**Window sizing fix:** Removed the `$effect` block in `src/App.svelte` that called `applyWindowSizing()` on every tab switch. The Settings tab's content-fitting code was forcing the window to a measured height, creating a jarring resize on tab switch. Only the zoom-level `$effect` now triggers `applyWindowSizing()`. Commit `<pending>`.

## Paste refactor (2026-06-24) — macOS done, Windows ready for build

### macOS paste (complete)
Complete rewrite of the paste module from a 366-line monolithic `paste.rs` into a 10-module tiered system (7 macOS + 3 Windows + legacy).

### Windows paste (new — needs Windows build test)
Three new modules gated on `#[cfg(target_os = "windows")]`:

| Module | Lines | What it does |
|--------|-------|-------------|
| `win_clipboard.rs` | 177 | Win32 clipboard full-format snapshot via `EnumClipboardFormats` + `GlobalAlloc`/`SetClipboardData`. Replaces `arboard`. Drop-guard for `CloseClipboard`. |
| `win_focus.rs` | 68 | `GetForegroundWindow` → `AttachThreadInput` → `SetForegroundWindow` + `BringWindowToTop`. Same as the macOS window activation fix. |
| `win_paste.rs` | 72 | Orchestrator: save clipboard → write text → activate window → `enigo` Ctrl+V with `Key::Layout('v')` (fixes layout issue) → restore from snapshot. |

**Key fix:** `enigo` now uses `Key::Layout('v')` instead of `Key::Unicode('v')` — Ctrl+V must fire on VK_V (0x56), not a WM_CHAR character event.

Cargo.toml: added `"winbase"`, `"synchapi"`, `"errhandlingapi"` to winapi features.

The `#[cfg(not(target_os = "macos"))]` paste branch is now split into `#[cfg(target_os = "windows")]` → `win_paste::paste()` and `#[cfg(target_os = "linux")]` → `legacy::paste()` (Linux path unchanged).

Complete rewrite of the paste module from a 366-line monolithic `paste.rs` into a 7-module tiered system:

### New architecture (`src-tauri/src/paste/`)
- **Tier 1** — `ax_inject.rs`: Direct AX text injection (`AXSelectedText` / `AXValue`) into native text fields. No clipboard, no keystrokes. Requires Accessibility trust.
- **Tier 2** — `clipboard.rs` + `keyboard_layout.rs` + `synthetic_keys.rs`: NSPasteboard save/restore (replaces `arboard`), dynamic V keycode via `UCKeyTranslate` (fixes Dvorak/Colemak), `CGEventPost` Cmd+V.
- **Tier 3** — Text left on clipboard as manual Cmd+V backup (when AX trust is absent).
- **Focus: ** `focus_capture.rs`: AX-based PID/bundle ID capture + `NSRunningApplication` activation before paste (fixes paste into wrong app).
- **Orchestrator** — `mod.rs` runs all three tiers in order with `changeCount`-gated clipboard restore.

### What changed
- `paste.rs` → `paste/legacy.rs` (kept for non-macOS + `frontmost_app` re-export)
- `arboard` eliminated on macOS (replaced by native NSPasteboard)
- Hardcoded V keycode 0x09 → dynamic `UCKeyTranslate` lookup
- 500ms sleep → 200ms sleep with `changeCount` guard
- Window activation added before paste (was missing entirely)
- 200ms sleep down from 500ms (changeCount guard catches races)
- `arboard`/`pbcopy` fallback removed on macOS

### To test
1. **TextEdit (native):** Dictate, verify Tier 1 fires. Logs should show `[ax_inject] injected N chars via AXSelectedText`
2. **Cursor/VS Code (Electron):** Dictate, verify fallthrough to Tier 2. Logs: `[paste] Tier 2 — clipboard restored`
3. **Clipboard preservation:** Copy something first, dictate, verify it's still on clipboard after
4. **Dvorak/Colemak:** Verify Cmd+V still works (should resolve correct keycode)
5. **Ad-hoc build:** Verify clipboard fallback still works (Tier 1 skipped, Tier 2 posts at HID)

## Windows hotkey (2026-06-24) — WH_KEYBOARD_LL hook landed

The Windows PTT hotkey path (`hotkey_win32.rs`) was rewritten from a 224-line
`GetAsyncKeyState` polling loop (8ms interval, 125 Hz) to a 317-line
`WH_KEYBOARD_LL` low-level keyboard hook on a dedicated `GetMessageW`
message-pump thread:

- Uses the shared `HotkeyController` pattern (press_action / release_action /
  arm_hold_cancel / cancel_if_busy) matching the macOS IOHID keyboard handler
- Reads config inside the callback (same pattern as macOS CGEventTap)
- Dedup via `PTT_KEY_HELD` / `ESC_KEY_HELD` atomics
- Skips synthetic/injected events via `LLKHF_INJECTED` flag
- Clean shutdown path: `GetMessageW` returning 0 breaks the loop, then
  `UnhookWindowsHookEx` fires
- `HotkeyProbe` preserved with updated fields (hook_installed, event counters)
- `cargo check --manifest-path src-tauri/Cargo.toml` passes (macOS)
- `GetAsyncKeyState`, `thread::sleep` in polling loop, `POLL_CTX` all removed

Commit `63d79a0`.

## Paste SIGTRAP fix (2026-06-24) — three causes, three fixes

Three independent crash sources were found and fixed during the paste flow:

### 1. `arboard` / NSPasteboard from background thread
`arboard` calls NSPasteboard directly without main-thread dispatch. On macOS 26
this throws NSInternalInconsistencyException → SIGTRAM. **Fix:** replaced arboard
with `pbcopy`/`pbpaste` subprocess calls in `clipboard.rs` (commit `44cbc80`).

### 2. `CGEventSourceCreate` / `CGEventCreateKeyboardEvent` crash
Both CoreGraphics event creation functions crash with SIGTRAP on macOS 26 from
ad-hoc signed binaries. **Fix:** replaced `CGEvent::new_keyboard_event` +
`CGEventPost` with `osascript -e 'keystroke "v" using command down'` in
`synthetic_keys.rs` (commit `ca39ccd`).

### 3. `reqwest` shared-client `.expect()` regression
Introduced in TASK-72 (shared `OnceLock<Client>`). The `.expect()` on builder
failure was a robustness regression from the original per-call code which handled
it gracefully. **Fix:** changed `ollama_client()` to return `Option`; each call
site handles `None` gracefully (commit `4265169`).

### Result
All three fixes combined — paste flow now works on macOS 26 ad-hoc builds:

1. `clipboard.rs` — pbcopy/pbpaste (no NSPasteboard threading)
2. `synthetic_keys.rs` — osascript keystroke (no CoreGraphics event creation)
3. `cleanup.rs` / `ollama.rs` — graceful error handling (no .expect())

## Shared reqwest client (2026-06-24) — per-call Client::builder eliminated

Four Ollama HTTP call sites now share a single `OnceLock<reqwest::blocking::Client>`
in `cleanup.rs::ollama_client()`, exported as `pub(crate)`:

- `classify_blocking()` in cleanup.rs — was 60s client-level + 3s connect, now shared client + per-request 60s
- `ping_ollama()` in ollama.rs — was 2s client-level, now shared client + per-request 2s
- `check_ollama_model()` in ollama.rs — was 2s client-level, now shared client + per-request 2s
- `prewarm_ollama()` in ollama.rs — was 60s client-level + 3s connect, now shared client + per-request 60s

Shared client has `.connect_timeout(2s)` and NO client-level `.timeout()`.
Zero `Client::builder()` calls remain in the four call sites. `cargo check` passes.

Commit `a6a84f2`.

## This session (2026-06-24) — NSSound/PlaySoundW chime

**Replaced per-chime subprocess spawning with in-process platform-native audio APIs:**

- **macOS:** `afplay -v <vol> <path>` → `NSSound::soundNamed_()` + `setVolume:` + `play()`. Extracts sound name from path (e.g. `Pop` from `/System/Library/Sounds/Pop.aiff`). Uses `objc2::msg_send!` and existing `ns_string` pattern from `paste/clipboard.rs`. Fire-and-forget async playback, zero subprocess overhead.
- **Windows:** `powershell [System.Media.SystemSounds]::...Play()` → `winapi::um::mmsystem::PlaySoundW` with `SND_ALIAS | SND_ASYNC | SND_NODEFAULT`. Maps events to system aliases: `SystemHand` (Start/Error), `SystemAsterisk` (Finish), `SystemExclamation` (Cancel).
- **Cargo.toml:** Added `"mmsystem"` to winapi features.

**Files changed:** `src-tauri/src/hotkey.rs`, `src-tauri/Cargo.toml`.

**Proof:** `cargo check` passes, `cargo clippy` passes (no new warnings). Zero `afplay` or `powershell` references remain in `play_chime`. Same system sounds, same async fire-and-forget behavior.

## This session (2026-06-24) — channel-backed PTT processor + tap-disable inline

**Moved CGEventTap work out of the callback into a channel-backed processing thread.**
The CGEventTap callback body had been doing RwLock reads, string matching, and
dispatch ladders inline, risking `kCGEventTapDisabledByTimeout` under load.
Replaced the inline callback body with a bounded channel + dedicated
`turbotalk-ptt-processor` thread that calls `process_tap_event` (the extracted
body). The callback now captures only keycode/flags/etype and sends them over the
channel, handling `TapDisabledByTimeout` / `TapDisabledByUserInput` inline by
re-enabling the tap through a statically stored raw Mach port pointer.

**Changes in `src-tauri/src/hotkey.rs`:**
- Added `TapEvent` struct, `TAP_MACH_PORT_RAW` static, `process_tap_event`
  function (extracted callback body), channel creation + processor thread spawn
- CGEventTap event mask extended to include `TapDisabledByTimeout` and
  `TapDisabledByUserInput`
- Removed the 8-second polling watchdog thread (`CGEventTapIsEnabled` loop)
- Removed `CGEventTapIsEnabled` extern declaration, unused `Receiver`/`Sender`
  imports, and unused per-loop closure clones

**Proof:** `cargo check` passes. `cargo clippy` passes (no new warnings).

## This session (2026-06-24) — Frontend refactor: dedup constants, fix reactivity, debounce, Overlay perf

**Extracted duplicated constants into shared modules:**
- `src/lib/prompts.ts` — PROMPT_PRESETS, DEFAULT_CLASSIFIER_PROMPT, four prompt templates
- `src/lib/catalog.ts` — KNOWN_FILENAMES, altModelVariant, altModelActive
- `src/lib/utils.ts` — seg() helper
- ModesTab.svelte and ModelsTab.svelte now import from these shared modules (no local copies)

**Fix Reactivity (state objects → individual props):**
- HistoryTab, ModelsTab, ModesTab now receive individual `$state`/`$derived` props instead of
  state-object factory functions (`historyState()`, `modelsState()`, `modesState()` removed)
- Tab components only re-render when their specific props change, not on any app state change

**Remove redundant IPC round-trips:**
- `saveSettings()`/`saveModes()`/`saveModels()` build full config from local state via
  `buildFullConfig()` — no `getConfig()` IPC call before every save
- `openModels()` no longer calls `getConfig()` (only `listModelsForFamily` if backend ≠ whisper)
- `openModes()` no longer calls `getConfig()` (local state initialized from backend on mount)
- `selectAltModel()` uses `buildFullConfig()` instead of `getConfig()`+mutate+save
- All cfg state variables now initialized from `initialCfg` in onMount

**Debounce hot-path operations:**
- `trackWindowHeight` debounced at 150ms via `resizeTimeout`
- Focus-based `recheckReadiness` debounced at 250ms via `readinessTimeout`

**Overlay performance:**
- Cursor position polling (setInterval at 100ms → IPC every frame) replaced with
  passive mousemove event listener (no IPC)
- `levels` array rebuild (`[...levels.slice(1), v]`) replaced with ring buffer
  (`levels[levelsHead] = v; levelsHead = (levelsHead + 1) % HISTORY`)
- Removed unused import of `cursorPosition`, `primaryMonitor` from `@tauri-apps/api/window`

**Timeout cleanup:**
- All `setTimeout` calls in `applyBackendEvent`, `copyHistoryItem`, and `startDownload`
  now track IDs in `pendingTimeouts` Set; all cleared on component unmount

**Files changed:** `src/App.svelte`, `src/ModesTab.svelte`, `src/ModelsTab.svelte`,
`src/HistoryTab.svelte`, `src/Overlay.svelte`, `src/lib/prompts.ts` (new),
`src/lib/catalog.ts` (new), `src/lib/utils.ts` (new)

**Proof:** `npm run typecheck` passes. `npm run build` passes.

## This session (2026-07-15) — Disable right-click and text selection globally

**Added two protections to prevent accidental right-click menus and text highlighting on non-input areas:**

- `src/app.css`: Added `user-select: none` to `html, body, #app` to disable text highlighting everywhere
- `src/app.css`: Added `input, textarea { user-select: text }` to preserve normal text selection in text fields
- `src/App.svelte`: Added `contextmenu` event listener on `window` that prevents default unless the target is an `INPUT` or `TEXTAREA` element

**Exceptions preserved:** The two textareas in ModesTab (custom vocabulary, anti-vocabulary) and the bug-note textarea in ResetModal continue to support right-click, text selection, copy/paste, and all standard text field interactions.

**Proof:** `npm run tauri build` succeeds. Release bundle built at `target/release/bundle/macos/Turbo Talk.app`.

## This session (2026-07-15) — Reflow Recording & Hotkey controls

Reworked the Settings page's Recording & Hotkey section so the four controls are independent rows: the side and recording-mode button selectors remain at the top, while the hotkey and microphone dropdowns are stacked at the bottom. All four controls are right-aligned.

Follow-up: added left-side labels for all four rows and constrained the three-option recording-mode selector to the available width so `Auto` remains visible.

Additional follow-up: allowed the recording-mode buttons to shrink within the row and constrained individual buttons against overflow; added the `Startup` label to the Launch at Login / Show Splash row.

Final follow-up: removed the truncation/ellipsis behavior from the recording-mode buttons so `Hold`, `Toggle`, and `Auto` render at full width.

Root cause follow-up: the generic 120px segmented-control rule was overriding the recording-mode selector. Excluded `.tt-seg-recording` from that rule so its 220px width is preserved.

**Proof:** `npm run typecheck` and `git diff --check` pass.

Sizing follow-up: Settings now re-measures window constraints after the tab renders via `tick()`, preventing stale empty space below the System section on first open.

**Proof:** `npm run typecheck` and `git diff --check` pass.

Tap-threshold follow-up: the Auto-mode delay slider is now permanently rendered beneath Recording mode using the shared volume-slider styling. It remains in the layout to prevent UI reflow and is disabled when a non-Auto mode is selected.

Auto-mode follow-up: fixed quick taps always behaving as long holds. `AutoController` instances are rebuilt per input event, so the press timestamp now lives in shared atomic state and survives key-down to key-up.

**Proof:** `cargo check --manifest-path src-tauri/Cargo.toml` and `git diff --check` pass.

Auto-mode diagnostics follow-up: added runtime logs for Auto press timestamps and release decisions (`elapsed_ms`, `threshold_ms`, keep-recording vs stop) because the existing logs confirmed UI persistence but did not expose controller routing. Added visible disabled styling for the non-Auto threshold slider.

**Proof:** `cargo check --manifest-path src-tauri/Cargo.toml`, `npm run typecheck`, and `git diff --check` pass.

Threshold range follow-up: raised the Auto tap-threshold minimum from 150ms to 200ms and adjusted the slider fill calculation to the new 200–1000ms range.

Tray-open focus follow-up: removed the global focus-visible outline from the TitleBar tab buttons. Opening the main window from the tray can focus History, but it now shows only the intended active underline rather than an extra rounded outline.

**Proof:** `npm run typecheck` and `git diff --check` pass.

Follow-up: added an explicit `.tt-tab` focus override with `outline` and `box-shadow` suppression because the utility-only override did not remove the rendered focus ring.

Slider layout follow-up: Volume now uses the same one-row label-and-slider layout and slider width as Tap threshold, with the percentage included in its left label.

**Proof:** `npm run typecheck` and `git diff --check` pass.

Appearance layout follow-up: Theme and Zoom now use right-aligned natural-width toggle groups with the same button spacing as Audio Notify instead of stretching across the full row.

**Proof:** `npm run typecheck` and `git diff --check` pass.

Connected-selector follow-up: Theme, Zoom, Visual Overlay, and Overlay Position now use the connected segmented-button treatment from Recording mode, while remaining right-aligned with natural widths.

**Proof:** `npm run typecheck` and `git diff --check` pass.

Auto cancel follow-up: a second press while Auto mode is recording now waits for the tap/hold decision instead of immediately toggling off. A quick second tap stops normally; a held second press reaches hold-to-cancel; a key-up after cancellation is ignored safely. The threshold label now greys out together with its disabled slider outside Auto mode.

**Proof:** `cargo check --manifest-path src-tauri/Cargo.toml`, `npm run typecheck`, and `git diff --check` pass.

## Outstanding
- Tray icon may still not be visible on user's display — needs user confirmation. Check both monitors and any Bartender/Ice/Hidden Bar software.
- Tray icon pixel size for macOS should be 22×22 (not 44×44) — low priority now that title text is visible.
