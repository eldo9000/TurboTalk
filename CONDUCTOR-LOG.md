# Conductor Log — Paste Refactor (AX Tiered Strategy)

**Started:** 2026-06-24
**Goal:** Replace clipboard + Cmd+V paste with a 3-tier strategy (AX injection → layout-aware clipboard → clipboard fallback) across `src-tauri/src/paste/` modules.
**Progress metric:** number of paste module files compiling + integrated into tiered `paste()` call
**Tripwire threshold:** 3 consecutive steps with no metric advance → Stuck-Loop Protocol

## Kickoff Brief

### Phase 1A — `clipboard.rs`
Native NSPasteboard snapshot/write/restore with `changeCount` guard. Replaces `arboard` on macOS.

**File:** `src-tauri/src/paste/clipboard.rs`
**Deps:** `objc2-app-kit 0.3`, `objc2-foundation 0.3`
**Interface:**
- `PasteboardSnapshot` (opaque struct)
- `fn snapshot() -> PasteboardSnapshot` — saves all items, all UTIs, all data + current changeCount
- `fn write_text(text: &str) -> Result<()>` — clearContents + setString:forType:
- `fn restore_if_untouched(snapshot: &PasteboardSnapshot) -> Result<bool>` — checks changeCount first, restores only if unchanged

### Phase 1B — `keyboard_layout.rs`
Dynamic V keycode resolution via `UCKeyTranslate`. Caches in `AtomicU16`.

**File:** `src-tauri/src/paste/keyboard_layout.rs`
**Deps:** `core-foundation 0.10` (already present), Carbon framework
**Interface:**
- `fn v_keycode() -> u16` — thread-safe, cached once
- `fn resolve_v_keycode() -> u16` — uncached lookup (for testing)

### Phase 1C — `focus_capture.rs`
AX-based focus capture (PID + bundle ID + app name) + NSRunningApplication activation.

**File:** `src-tauri/src/paste/focus_capture.rs`
**Deps:** `objc2-app-kit 0.3`, `objc2-foundation 0.3`, `core-foundation 0.10`
**Interface:**
- `FocusSnapshot { pid, bundle_id, app_name }`
- `fn snapshot() -> FocusSnapshot` — captures current focused element/app
- `fn activate_app(snapshot: &FocusSnapshot)` — brings the target app to front

### Phase 2 — `ax_inject.rs`
Direct AXUIElementSetAttributeValue text injection (Tier 1).

**File:** `src-tauri/src/paste/ax_inject.rs`
**Deps:** `core-foundation 0.10` (already present)
**Interface:**
- `fn try_inject_text(text: &str) -> Result<Option<()>>` — None = incompatible element, Some(Ok) = injected, Some(Err) = failure

### Phase 3 — Integration
Wire all modules into `paste/mod.rs` as tiered `paste()` function. Update `hotkey.rs` focus capture calls. Remove legacy arboard clipboard path.

**Files:** `paste/mod.rs`, `hotkey.rs`, `paste/legacy.rs` (remove)

## Step 1 — 2026-06-24 — Phase 0: skeleton + Cargo.toml

- **Status:** CLOSED
- **Mission:** Create paste/ directory, move paste.rs → legacy.rs, write skeleton mod.rs, add objc2-app-kit dep
- **Worker did:** Manual setup by conductor
- **Conductor verified:** `cargo check` passes clean
- **Repo delta:** `paste/` dir with mod.rs + legacy.rs; Cargo.toml adds `objc2-app-kit`
- **Metric:** 2 files compiling (mod.rs, legacy.rs)
- **Next:** Phase 1A + 1B + 1C

## Step 2 — 2026-06-24 — Phase 1A/B/C: clipboard, keyboard_layout, focus_capture

- **Status:** CLOSED
- **Mission:** Write clipboard.rs, keyboard_layout.rs, focus_capture.rs in parallel
- **Worker did:**
  - Agent A: wrote clipboard.rs (192 lines) — NSPasteboard snapshot/write/restore with changeCount
  - Agent B: wrote keyboard_layout.rs (109 lines) — UCKeyTranslate keycode resolver, AtomicU16 cache
  - Agent C: wrote focus_capture.rs (167 lines) — AX focus capture, FocusSnapshot, activate_app
- **Conductor verified:** All three files reviewed and correct. No arboard/pbcopy in clipboard. Correct CFRelease hygiene. NSRunningApplication activation with Sonoma yieldActivationToApplication.
- **Repo delta:** 3 new files, mod.rs updated with cfg-gated declarations
- **Metric:** 5 files compiling
- **Next:** Phase 2

## Step 3 — 2026-06-24 — Phase 2: ax_inject + synthetic_keys

- **Status:** CLOSED
- **Mission:** Write ax_inject.rs (Tier 1 AX injection) + synthetic_keys.rs (layout-aware Cmd+V)
- **Worker did:**
  - Wrote ax_inject.rs (127 lines) — AXSelectedText/AXValue setAttribute, role logging, clean fallthrough
  - Wrote synthetic_keys.rs (24 lines) — CGEventPost Cmd+V via keyboard_layout::v_keycode()
- **Conductor verified:** Both files reviewed. AX injection uses proper CFString + TCFType trait. Synthetic keys uses HID tap. Clean error paths.
- **Repo delta:** 2 new files, mod.rs updated
- **Metric:** 7 files compiling
- **Next:** Phase 3

## Step 4 — 2026-06-24 — Phase 3: mod.rs integration

- **Status:** CLOSED
- **Mission:** Write the tiered paste() orchestrator in mod.rs
- **Worker did:** Manual by conductor
- **Conductor verified:** Tiered strategy: try AX injection first, fall through to clipboard+synthetic keys, clipboard restore gated on changeCount. Non-macOS path delegates to legacy::paste(). 200ms sleep (down from 500ms). `cargo build` passes clean.
- **Repo delta:** mod.rs rewritten (128 lines), clipboard.rs + empty() constructor, legacy.rs + allow(dead_code) suppression
- **Metric:** 7 files compiling, all wired into paste() function
- **Next:** Windows refactor (prompt from user)

## Plan Revision — 2026-06-24

Added Windows paste refactor to scope: `win_clipboard.rs`, `win_focus.rs`, `win_paste.rs`.

### Phase 1W — `win_clipboard.rs`
Win32 clipboard snapshot/write/restore via `OpenClipboard` / `EnumClipboardFormats` / `SetClipboardData` / `GlobalAlloc`. Full format preservation.

### Phase 2W — `win_focus.rs`
`GetForegroundWindow` / `SetForegroundWindow` / `AttachThreadInput` for window activation.

### Phase 3W — `win_paste.rs`
Orchestrator: save clipboard → write text → activate window → enigo Ctrl+V → restore.

### Phase 4W — Integration
Split `#[cfg(not(target_os = "macos"))]` into separate `#[cfg(target_os = "windows")]` and `#[cfg(target_os = "linux")]` branches.

## Step 5 — 2026-06-24 — Phase 1W: win_clipboard.rs

- **Status:** CLOSED
- **Mission:** Write Win32 clipboard module with full format save/restore
- **Worker did:** Agent wrote win_clipboard.rs (177 lines) — ClipboardGuard drop-guard, snapshot() enumerates all formats, write_text() encodes UTF-16 → CF_UNICODETEXT, restore() reconstructs all formats
- **Conductor verified:** Correct GlobalAlloc/GlobalLock/SetClipboardData pattern. Proper HGLOBAL ownership: SetClipboardData takes ownership on success, GlobalFree on failure. Drop-guard for CloseClipboard.
- **Repo delta:** win_clipboard.rs (new)
- **Metric:** 1 new Windows file
- **Next:** Phase 2W

## Step 6 — 2026-06-24 — Phase 2W + 3W: win_focus.rs, win_paste.rs

- **Status:** CLOSED
- **Mission:** Write window focus module + Windows paste orchestrator
- **Worker did:**
  - Agent 2W: win_focus.rs (68 lines) — foreground_hwnd(), foreground_pid(), activate_hwnd() with AttachThreadInput pattern
  - Agent 3W: win_paste.rs (72 lines) — full sequence: snapshot → write_text → activate_hwnd → enigo Ctrl+V with Key::Layout('v') → restore
- **Conductor verified:** Window activation uses correct thread-attach dance. enigo uses Key::Layout('v') (virtual-key code) not Key::Unicode (WM_CHAR). Ctrl release on error path.
- **Repo delta:** win_focus.rs, win_paste.rs (new)
- **Metric:** 3 Windows files
- **Next:** Phase 4W

## Step 7 — 2026-06-24 — Phase 4W: mod.rs integration

- **Status:** CLOSED
- **Mission:** Wire Windows modules into mod.rs; split non-macOS branch into Windows + Linux
- **Worker did:** Manual by conductor
- **Conductor verified:** mod.rs updated: declares win_* modules under `#[cfg(target_os = "windows")]`, splits paste() into `#[cfg(target_os = "windows")]` (calls win_paste::paste) and `#[cfg(target_os = "linux")]` (calls legacy::paste). `cargo check` clean on macOS.
- **Repo delta:** mod.rs updated
- **Metric:** All 10 paste/ files declared, 3 active per platform
- **Next:** User testing
