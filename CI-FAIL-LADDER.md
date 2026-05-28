# CI Fail Ladder — TurboTalk

## Fail #1 — 2026-05-28 — Windows build: E0505 borrow in hotkey.rs

- **Q1 in-last-commit:** yes — `src-tauri/src/hotkey.rs`
- **Q2 named-error:** yes — `error[E0505]: cannot move out of hotkey_state because it is borrowed` at hotkey.rs:1236
- **Q3 seen-before:** no — first entry this arc
- **Q4 broken-vs-missing:** broken
- **Verdict:** QUICK (budget: 1 attempt)
- **Hypothesis:** Startup log reads `hotkey_state` then moves it into the rdev closure in the same scope; drop the read guard before the move.
- **Next:** `src-tauri/src/hotkey.rs:1217` — read key/mode into locals, release guard, then move `hotkey_state`
