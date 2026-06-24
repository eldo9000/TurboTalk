# TurboTalk — Session Status

**Last updated:** 2026-06-22 (tray icon consolidation)  
**Current state:** IOHID keyboard fallback is active and user-proven for the ad-hoc `/Applications/Turbo Talk.app`. Right Option dictation works through Input Monitoring. Ad-hoc macOS auto-paste is user-proven via Session tap. Large overlay mode now has an audio-driven glyph/text preview: live speech appears as word-shaped pills, paused segment commits become readable text, and the live pill cursor continues from the committed edge. Live pill widths use a lorem-ipsum word-length sequence, preview/audio panels are both fixed to 984px, and the live pill rate now adapts after segment commits instead of assuming one speaking speed.

## Next action

Run one large-overlay dictation by eye: speak slow and fast pause-separated lines; success signal is live pill count stays plausible across speaking speeds, no leading red cursor pill, compact preview bubble, aligned top/bottom panel edges, committed text appears on pauses, and dictation still pastes normally.

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
- **Moonshine** — retired; legacy configs normalize to Parakeet

## Recent commits

- `e5aae45` — docs: log v0.9.8 release CI signing env failure in CI-FAIL-LADDER
- `b40fda9` — fix(ci): only set Apple notarization env vars when Developer ID credentials present
- `3521733` — fix(ci): fall back to ad-hoc identity instead of empty string in release workflow
- `9c6b3ca` — chore(release): bump to 0.9.8

## This session

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

## Outstanding
- Tray icon may still not be visible on user's display — needs user confirmation. Check both monitors and any Bartender/Ice/Hidden Bar software.
- Tray icon pixel size for macOS should be 22×22 (not 44×44) — low priority now that title text is visible.
