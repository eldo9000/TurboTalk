# CI Fail Ladder — TurboTalk

## Fail #1 — 2026-05-28 — Windows build: E0505 borrow in hotkey.rs

- **Q1 in-last-commit:** yes — `src-tauri/src/hotkey.rs`
- **Q2 named-error:** yes — `error[E0505]: cannot move out of hotkey_state because it is borrowed` at hotkey.rs:1236
- **Q3 seen-before:** no — first entry this arc
- **Q4 broken-vs-missing:** broken
- **Verdict:** QUICK (budget: 1 attempt)
- **Hypothesis:** Startup log reads `hotkey_state` then moves it into the rdev closure in the same scope; drop the read guard before the move.
## Fail #2 — 2026-05-28 — Windows build: E0583 hotkey_win32 module not found

- **Q1 in-last-commit:** yes — `src-tauri/src/hotkey.rs` (+ `hotkey_win32.rs`)
- **Q2 named-error:** yes — `error[E0583]: file not found for module hotkey_win32` at hotkey.rs:1344
- **Q3 seen-before:** no — different error class from Fail #1 (module path vs borrow)
- **Q4 broken-vs-missing:** broken
- **Verdict:** QUICK (budget: 1 attempt)
- **Hypothesis:** `mod hotkey_win32` inside `hotkey.rs` resolves to `hotkey/hotkey_win32.rs`; file lives at `src/hotkey_win32.rs`. macOS CI skip hid this.
- **Next:** `hotkey.rs:1344` — add `#[path = "hotkey_win32.rs"]` on the mod declaration

## Fail arc closed — 2026-05-28 — 2 entries — green CI 26606830689
