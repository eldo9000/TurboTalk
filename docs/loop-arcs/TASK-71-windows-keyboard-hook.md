# Arc Log — TASK-71: WH_KEYBOARD_LL hook replacement

## Gate
Replace Windows `GetAsyncKeyState` polling (8ms interval, 125 Hz) with a
`WH_KEYBOARD_LL` low-level keyboard hook on a dedicated thread running a
`GetMessageW` message pump.

## Premise declarations

### Premise 1 (initial)
- **SYMPTOM:** Windows PTT hotkey uses documented-wrong pattern — `GetAsyncKeyState` polling at 125 Hz, burns CPU, can miss events, modifier `up` detection unreliable.
- **PREMISE:** Replacing the polling loop with a `WH_KEYBOARD_LL` hook + dedicated message-pump thread will produce correct, low-latency global hotkey behavior on Windows, matching the macOS CGEventTap/IOHIDManager path's reliability.
- **DERIVATION:** The hook is the documented-correct Windows API (`WH_KEYBOARD_LL` + `SetWindowsHookExW` + `GetMessageW` loop on owning thread). The task header's stated concern ("WH_KEYBOARD_LL often installs successfully but receives zero events") is traced to missing message loop — the thread that owns the hook MUST pump messages for the callback to fire.
- **FALSIFICATION:** If the macOS build breaks (`cargo check` fails on macOS) due to incorrect `#[cfg(windows)]` gating, the premise that this can be done without harming the existing macOS path is false.
- **FALSIF-RESULT:** not yet run
- **DISPOSITION:** <pending>
