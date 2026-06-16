# TurboTalk — Cross-Platform Compatibility

Last updated: 2026-06-16. This is the living reference for every platform-specific
split in the codebase — what exists, why it exists, what's deferred, and what
needs proving.

---

## Architecture overview

TurboTalk runs on three platforms, with three entirely different approaches to
the single hardest problem: **global push-to-talk hotkey.**

| Platform | Hotkey mechanism | Paste mechanism | Status |
|----------|-----------------|-----------------|--------|
| macOS | CGEventTap + IOHIDManager (event-driven) | CGEventPost Cmd+V (native) | **Golden — proven** |
| Windows | `GetAsyncKeyState` polling (125 Hz) | `enigo` Ctrl+V | Built, unvalidated on real hardware |
| Linux/X11 | `rdev` XRecord hook | `enigo` Ctrl+V | Not validated on real hardware |
| Linux/Wayland | Explicitly unsupported | Explicitly unsupported | Deferred to 2.0 |

The rest of the pipeline — audio capture (`cpal`), transcription (whisper-server
sidecar), LLM cleanup (Ollama via reqwest), and settings persistence (`dirs`
crate) — is **fully cross-platform.** Only the hotkey trigger and paste injection
are platform-specific.

---

## Hotkey — three implementations, one shared lifecycle

All platform-specific hotkey code calls into a **shared `common` module**
(`hotkey.rs:15-1370`) that owns the recorder state machine, cancel-pending
races, toggle/hold mode, focus tracking, and UI event emission. The per-OS
implementations only own "is the key currently pressed/released?"

### macOS — `#[cfg(target_os = "macos")]` (`hotkey.rs:1376-1991`)

- **CGEventTap** at `kCGHIDEventTap` level — intercepts all keyboard events
- CFRunLoop-based event processing on a dedicated thread
- Carbon key codes: `kVK_RightOption` (0x3D), `kVK_RightControl` (0x3E),
  `kVK_F13`–`kVK_F19` (0x69–0x50)
- **IOHIDManager** for raw HID mouse button events — reads Button usage values
  at the IOKit level, bypassing driver software (Logi Options+, etc.)
- Accessibility trust check: `AXIsProcessTrusted()` (macOS TCC)
- Sound chimes: `afplay /System/Library/Sounds/Pop.aiff`
- Requires `macOSPrivateApi: true` in `tauri.conf.json`

**Why CGEventTap and not rdev:** macOS 26 enforces `dispatch_assert_queue` on TSM
APIs; rdev crashes on its background thread. CGEventTap is the Apple-supported
API for this purpose.

### Windows — `hotkey_win32.rs` (`#[cfg(target_os = "windows")]`)

- `GetAsyncKeyState` polling at ~125 Hz (8 ms interval) via `winapi`
- VK codes: modifier keys (0xA0–0xA5, 0x5B–0x5C), F13–F24 (0x7C–0x87),
  mouse buttons (0x04–0x06)
- Sound chimes via PowerShell `[System.Media.SystemSounds]`
- `accessibility_trusted()` always returns `true` (no equivalent gate)
- Diagnostic probe: `HotkeyProbe` struct with listener_alive, poll_loops,
  matched_down_count, last_matched_vk

**Why polling and not rdev:** `WH_KEYBOARD_LL` (the low-level keyboard hook
rdev uses on Windows) often receives zero events in packaged Tauri builds.
Polling `GetAsyncKeyState` is reliable on all Windows configurations.

**Known limitations of polling:**
- Only modifier keys are reliable — F-keys and mouse buttons rely on
  edge-timing that a 125 Hz poll can miss
- Inherent 8–16 ms latency (CGEventTap is sub-millisecond)
- CPU cost of a spinning polling thread (minor but real)
- Toggle mode detection for non-modifier keys is less reliable

**Recommended upgrade path for v1.0:** Replace polling with `RegisterHotKey`
(Win32 API) for key combos, and `RegisterRawInputDevices` for mouse buttons.
Both are event-driven (zero polling) and OS-managed. Tauri 2's
`tauri-plugin-global-shortcut` wraps `RegisterHotKey` natively.

### Linux/X11 — `#[cfg(not(target_os = "macos"))] #[cfg(target_os = "linux")]` (`hotkey.rs:1991-2226`)

- `rdev::listen` — global keyboard hook via X11 `XRecord`
- Key mapping includes ControlLeft/Right, ShiftLeft/Right, MetaLeft/Right,
  Alt/AltGr, numpad keys, F13–F19
- Wayland detection via `XDG_SESSION_TYPE` env var — emits
  `hotkey-unsupported` ui-error without binding
- Sound chimes silently no-op
- `accessibility_trusted()` returns `false` on Wayland, `true` on X11

### Linux/Wayland — explicitly unsupported

Wayland intentionally prevents global key grabs as a security design choice.
The current approach of fast-failing with a clear error message is correct
for now. Long-term paths depend on the compositor:
- **KDE 6**: Global Shortcuts portal via D-Bus
- **wlroots** (Sway, Hyprland): `zwp_input_method_v2` +
  `zwp_virtual_keyboard_v1` protocols
- **GNOME**: no equivalent API as of 2026
- Cross-desktop `org.freedesktop.impl.portal.GlobalShortcuts` D-Bus portal
  being standardized, adoption still early

**Deferred to the 2.0 track.**

---

## Paste injection — two implementations

### macOS — CGEventPost (`paste.rs:149-189`)

- Clipboard: `arboard::Clipboard` for read/write
- Frontmost app detection: `NSWorkspace.sharedWorkspace.frontmostApplication.localizedName` (sub-millisecond)
- Keystroke injection: `CGEvent::new_keyboard_event` + `CGEventPost` to `CGEventTapLocation::HID`
- Cmd+V with kVK_ANSI_V (0x09)
- 500ms delay before clipboard restoration
- AX role query for diagnostics only — does not block paste

### Windows + Linux — enigo (`paste.rs:201-250`)

- Clipboard: `arboard::Clipboard`
- Keystroke injection: `enigo` — `Ctrl+V` (press Ctrl, click 'v', release Ctrl)
- 50ms pre-delay, 150ms post-delay before clipboard restore
- Wayland: rejected with error containing `"unsupported platform"` literal
- `frontmost_app()` returns `None` on non-macOS

---

## Permission model

| Permission | macOS | Windows | Linux |
|-----------|-------|---------|-------|
| Accessibility (hotkey) | `AXIsProcessTrusted()` | Returns `Unsupported` | Returns `Unsupported` |
| Input Monitoring | `IOHIDCheckAccess()` | Returns `Unsupported` | Returns `Unsupported` |
| Microphone | TCC + native prompt | Returns `Unsupported` (cpal prompts) | Returns `Unsupported` |

On Windows and Linux, all three permission checks return `Unsupported`, which
is treated as non-blocking in the readiness gate. This is correct — cpal prompts
naturally on Windows, and global hooks don't require explicit permission.

---

## Audio capture — fully cross-platform

Uses `cpal` 0.15 which abstracts all platforms. Three sample format paths
(F32, I16, U16) cover every platform.

**macOS-specific concern:** `cpal::Stream` is `!Send` due to a missing bound
on the `_disconnect_listener` closure. `audio.rs` uses `unsafe impl Send for
AudioCapture` after threading the needle with controlled access — this is the
only `unsafe` block in the audio module and is documented inline.

Platform-aware mic permission help text strings exist for macOS and Windows;
Linux gets a generic fallback.

---

## Transcription — whisper-server sidecar

### Binary discovery

Uses `TARGET_TRIPLE` (compile-time env var) to build the sidecar filename:
- All platforms: `whisper-server`, `whisper-server-{triple}`
- Windows appends `.exe` suffix

### Subprocess management

| Concern | Unix (macOS/Linux) | Windows |
|---------|-------------------|---------|
| No-console-window flag | N/A | `CREATE_NO_WINDOW` (0x08000000) |
| Process group isolation | `setsid()` | N/A (default behavior) |
| Orphan cleanup | `pkill -f <path>` | `taskkill /F /IM whisper-server.exe` |
| Stderr log path | `std::env::temp_dir()` | `std::env::temp_dir()` |

### Sidecar bundling

- **macOS arm64**: Committed directly in `src-tauri/binaries/` — `whisper-cli`
  + 3 dylibs. `@executable_path/../Resources` rpath resolves at runtime.
- **Windows x64**: `npm run fetch-sidecars` downloads upstream whisper.cpp
  v1.8.4, extracts `whisper-cli.exe` + 4 DLLs into `src-tauri/binaries/`.
  Declared in `tauri.conf.json` `bundle.resources`.
- **Linux**: No upstream binary. Users need `whisper-server` on PATH
  (distro package or build from source). Excluded from release matrix.

---

## Settings paths

| Platform | Data directory | Config file | Permissions |
|----------|---------------|-------------|-------------|
| macOS | `~/.config/turbotalk/` | `config.toml` | Unix 0o700/0o600 |
| Linux | `~/.config/turbotalk/` | `config.toml` | Unix 0o700/0o600 |
| Windows | `%APPDATA%/turbotalk/` | `config.toml` | Rely on %APPDATA% scope |

Uses `dirs::config_dir()`. Legacy path `~/.config/librewin/turbotalk/` is
auto-migrated on first load.

---

## Default settings per platform

| Setting | macOS | Windows | Linux |
|---------|-------|---------|-------|
| Hotkey key | `right_option` | `right_control` | `right_control` |
| Hotkey mode | `hold` | `toggle` | `hold` |

**Rationale:** macOS Right Option (⌥) is uncommitted by most apps. Windows
keyboards with US layout lack Right Alt as a distinct physical key, so Right
Control is the safer default. Toggle mode on Windows avoids the polling
limitation for hold detection.

---

## UI / windowing

- macOS overlay uses `set_ignore_cursor_events` for click-through.
  Not available on Windows/Linux — overlay behavior differs.
- macOS uses `ActivationPolicy::Accessory` to hide from Dock.
- Windows tray icon uses 32×32 (avoids semi-transparent fringe on Win32
  compositor). macOS/Linux use 44×44.
- Window positioning math is platform-agnostic; the safety clamp uses
  logical coordinates consistently.

---

## Conditional compilation inventory

See the codebase itself for exact locations. This section documents the
architectural splits by purpose, not exhaustive line numbers.

### Module-level splits

| Module | macOS | Windows | Linux |
|--------|-------|---------|-------|
| `hotkey.rs` imp | CGEventTap (line 1376) | `hotkey_win32.rs` via `#[path]` (line 2228) | rdev (line 1991) |
| `paste.rs` | CGEventPost (line 149) | enigo (line 201, shared with Linux) | enigo (line 201, shared with Windows) |

### Feature-level splits

- **Permission checks** (permissions.rs): All three macOS-only. Return
  `Unsupported` elsewhere.
- **Sound chimes** (hotkey.rs): macOS `afplay`, Windows PowerShell
  SystemSounds, Linux no-op.
- **OS version** (diagnostic_log.rs): macOS `sw_vers`, others stub.
- **File permissions** (settings.rs): Unix `mode()` owner-only,
  Windows no-op.
- **Subprocess flags** (transcribe.rs): Windows `CREATE_NO_WINDOW`,
  Unix `setsid()`.
- **Orphan cleanup** (transcribe.rs): Windows `taskkill`,
  Unix `pkill`.
- **Window management** (windowing.rs): Multiple macOS-only calls
  (`set_ignore_cursor_events`, `set_activation_policy`).

---

## Known gaps and deferred items

### Windows

- **Hotkey uses polling, not hooks.** See "Recommended upgrade path" above.
  The polling approach works but has inherent limitations (latency, F-key
  reliability, CPU cost). Plan: replace with `RegisterHotKey` for Windows
  v1.0 release.
- **No real-hardware proof.** Windows has only been tested in UTM/QEMU VMs
  where global hook APIs have known virtualization limitations. A packaged
  build must be exercised on a physical Windows machine with a real keyboard,
  microphone, and target app.
- **No Windows hardware in CI.** Release build passes but runtime behavior
  is unvalidated.

### Linux

- **X11 only.** Wayland is explicitly unsupported (see Wayland section above).
- **No upstream whisper-server binary.** Users must supply their own.
- **No real-hardware validation** of hotkey (XRecord) or paste (enigo).
- **Deferred to the 2.0 track.**

### Both

- No accessibility or input-monitoring permission concept. These return
  `Unsupported` and are treated as non-blocking in readiness — correct
  for now but should be reconsidered if platform permission models evolve.
- No diagnostic probe for macOS or Linux hotkey listeners (Windows-only
  `HotkeyProbe`).

---

## Testing strategy per platform

### macOS (reference platform)

- Build + run: `npm run tauri dev`
- Packaged: `npm run package` → install DMG → Accessibility grant → dictate
- Proof signal: "I held Right Alt, said 'hello world', and
  'hello world' appeared in TextEdit."

### Windows (next priority)

**Pre-requisites for a valid test:**
- Physical Windows machine (x64 or ARM64 via Parallels), **not a VM**
- Microphone connected and working
- Packaged build installed (not `tauri dev`)
- A text editor open and focused (Notepad is fine)

**Test session protocol (one round-trip):**
1. Launch TurboTalk on Windows
2. Open Settings → Developer → Export diagnostic (saves report to logs folder)
3. Configure hotkey to `right_control` + `toggle` mode
4. Press hotkey → confirm overlay appears (red dot pulses)
5. Speak a short phrase ("hello world")
6. Press hotkey again to stop → wait for transcription → confirm text appears
7. Repeat steps 4–6 three times with different phrases
8. Export diagnostic report again
9. Copy both report files + logs folder → transfer back for analysis

**What each report must tell us:**
- Did the hotkey polling thread start? (`listener_alive`)
- How many poll cycles ran? (`poll_loops`)
- How many key-downs were detected? (`matched_down_count`)
- Did the mic open? (audio_input_available)
- Did whisper-server start? (sidecar probe)
- How many dictations succeeded vs. failed? (session metrics)
- Did paste succeed? (paste metrics)
- What's the full session log? (log tail)

### Linux (2.0 track)

- Must be tested on real X11 hardware
- User must install whisper-server via distro package or source
- Wayland path needs monitoring as compositor APIs mature

---

## Platform-specific gotchas

1. **Windows: `WH_KEYBOARD_LL` silent failure in packaged builds.**
   This is why `hotkey_win32.rs` uses polling. Documented inline. If we
   ever switch to `RegisterHotKey`, this gotcha goes away.

2. **macOS: `cpal::Stream` is `!Send`.**
   The `unsafe impl Send for AudioCapture` in `audio.rs` works because
   the disconnect listener closure is only touched on the cpal callback
   thread. If `AudioCapture` is ever cloned or shared across threads in
   a new way, re-audit this.

3. **macOS 26: `CGEventTap` requires Accessibility trust.**
   If the user revokes Accessibility permission while the app is running,
   the tap silently stops. The macOS hotkey module has a periodic
   `CGEventTapIsEnabled` check for recovery.

4. **Windows: `arboard` clipboard race.**
   The paste module writes to clipboard, synthesizes Ctrl+V, waits 150ms,
   then restores prior clipboard. If another app writes to clipboard in
   that window, TurboTalk restores the *other app's* content. This is rare
   but possible. The 150ms delay is deliberately shorter than macOS's 500ms
   because Windows apps process Ctrl+V faster on average.

5. **Linux: `XDG_SESSION_TYPE` must be checked at runtime.**
   The same binary can run under X11 or Wayland. Both hotkey and paste
   check this env var at launch and on each call. If the user switches
   sessions without restarting, behavior is undefined.

---

## Build configuration

From `src-tauri/tauri.conf.json`:

| Setting | Value |
|---------|-------|
| `macOSPrivateApi` | `true` (required for CGEventTap, ActivationPolicy) |
| macOS signing | Ad-hoc (`signingIdentity: "-"`) |
| macOS hardenedRuntime | `false` |
| Windows installer | NSIS (`installMode: "currentUser"`) |
| `externalBin` | `["binaries/whisper-cli", "binaries/whisper-server"]` |
| Updater artifacts | `false` in committed config; enabled in CI release via `--config` |
| Release targets | macOS arm64 + Windows x64; Linux excluded |

---

## Decision log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-05 | Drop rdev on macOS | macOS 26 `dispatch_assert_queue` crashes rdev. Replaced with direct CGEventTap. |
| 2026-06 | Windows polling (`hotkey_win32.rs`) | `WH_KEYBOARD_LL` received zero events in packaged Tauri builds. Polling is reliable. |
| 2026-06 | Linux deferred to 2.0 | X11 unvalidated; Wayland unsupported. Gating on real hardware access. |
| 2026-06 | Wayland explicit fast-fail | Better to show a clear error than silently fail or crash on Wayland. |
| 2026-06 | Moonshine retired (2026-06-16) | No longer a supported backend. Legacy configs normalize to Parakeet. |
