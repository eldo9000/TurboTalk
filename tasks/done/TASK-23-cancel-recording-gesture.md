# TASK-23: User-cancellable in-flight recording (Ctrl+Alt hold + optional Esc)

## Goal
While a recording is active (or already transcribing), the user can press
**Ctrl+Alt held alone for ~300 ms** to cancel — the audio is dropped, any
in-flight whisper-cli subprocess is killed, the overlay clears immediately,
no transcript event fires, no paste happens. **Esc is available as an
optional second gesture** behind a settings toggle (off by default because
Esc is widely overloaded). Both toggles can be enabled simultaneously.

## Context
TurboTalk currently auto-discards recordings shorter than ~100 ms via VAD
trimming (TASK-6/13/22), emitting `recording-discarded`. There is no
user-initiated cancel — once you start a recording, the only way out is to
let it finish and waste 2–3 seconds of whisper cycles. The user has asked
for a deliberate cancel gesture, modeled on the existing too-short-discard
shape (just emit the same kind of "thrown-away" signal, but on user input
rather than VAD verdict).

Why Ctrl+Alt and not the PTT key:
- In PTT mode the hotkey is *held*. A "double-tap" gesture would require
  release+repress within a tight window — awkward and easy to misfire.
- Esc is unambiguous but widely overloaded (modal dialogs, search bars,
  vim-style editors). False cancels of real dictations are the worst
  failure mode here, so Esc is opt-in.
- Ctrl+Alt held alone (no third key, no other modifier) is a rare gesture
  in normal typing — `Ctrl+Alt+<letter>` produces accented characters or
  app shortcuts, but those always have a third key arriving quickly.
  Requiring a 300 ms hold with no other key event filters out the
  accidental brush.

Pre-existing infrastructure:
- `Recorder::cancel()` exists from TASK-6 (added for the device-lost path).
  It drops the active stream, clears samples, returns state to `Ready`
  without producing a WAV. Already correct for the `Recording` state.
- `recording-discarded` event already exists. We add a sibling
  `recording-cancelled` so the UI can distinguish user-initiated cancels
  (no banner needed) from too-short discards (which already get a subtle
  hint).
- The Overlay already clears to idle on `recording-discarded` and
  `recording-too-short`. Adding `recording-cancelled` is one more listener.
- `TranscriptionWorker` from TASK-20 spawns `whisper-cli` per recording
  via `std::process::Command::output()`. Today there is no handle on the
  in-flight Child, so cancel cannot reach it. Needs a small refactor to
  hold the active Child in a Mutex so `cancel()` can `kill()` it.
- `HotkeyConfig` and `CleanupConfig` are already specta-derived (TASK-8),
  so adding two bools surfaces them in the typed bindings automatically.

## In scope
- `src-tauri/src/settings.rs` — add `cancel_via_ctrl_alt: bool` (default
  `true`) and `cancel_via_esc: bool` (default `false`) to `HotkeyConfig`.
  Both are serde-defaulted so existing config files don't break.
- `src-tauri/src/hotkey.rs` — chord detection. Add a small state machine
  alongside the existing PTT detection that:
  - Watches `FlagsChanged` events for transitions into the
    `(Control | Alternate)` flag combination, with no other modifier
    bits set (Cmd, Shift, etc. must be off).
  - On entry into the chord, captures `Instant::now()` and starts a
    timer (or schedules a deadline check).
  - If 300 ms elapse with no `KeyDown` event and no `FlagsChanged`
    transition removing the chord, fires the cancel path.
  - Skips the timer entirely if `cancel_via_ctrl_alt` is false.
  - Watches `KeyDown` for Esc (keycode `0x35`) and fires the cancel
    path immediately if `cancel_via_esc` is true.
- `src-tauri/src/recorder.rs` — extend `cancel()` to also work from the
  `Transcribing` state. From `Recording`: existing behavior. From
  `Transcribing`: call `worker.abort()` (new method below), then
  transition to `Ready`. From any other state: log at debug level and
  return Ok (idempotent, no-op).
- `src-tauri/src/transcribe.rs` — `TranscriptionWorker` gains a
  `Mutex<Option<Child>>` field tracking the active subprocess. The
  `transcribe()` method stores the Child immediately after spawn and
  clears it on completion. New `abort()` method takes the Mutex,
  pulls out any active Child, calls `child.kill()` on it (best effort,
  ignore errors — process may have already exited).
- `src-tauri/src/lib.rs` — register a new `cancel_recording` Tauri
  command (in case the frontend later wants a UI cancel button). The
  hotkey thread does not go through this command — it calls into the
  Recorder directly.
- `src/App.svelte` — settings tab gains a "Cancel recording" section
  with two checkboxes ("Ctrl+Alt held briefly" / "Esc key"). Renders
  a small note next to Esc warning that it may conflict with other
  apps. Uses the typed specta bindings.
- `src/Overlay.svelte` — add `listen('recording-cancelled', ...)` that
  sets `mode = 'idle'` immediately. Same shape as the existing
  `recording-discarded` listener.

## Out of scope
- Cancel during `Cleaning` or `Pasting` states — both are sub-second on
  the happy path; a cancel mid-paste would risk leaving a partial
  paste in the focused app, which is worse than letting it finish.
- Frontend UI cancel button (X icon on the overlay, etc.). Out of
  scope for this task; the Tauri command exists for future use.
- Configurable hold duration. 300 ms is a good default; expose later
  if anyone asks.
- Configurable cancel chord (e.g. user-picked modifier combo). Same.
- Changing the existing PTT chord-detection logic.
- Killing or aborting the `cleanup::process()` HTTP call to Ollama.
  It already has a 2 s timeout per TASK-4.
- Touching the streaming finalizer worker (TASK-22) directly. The
  Recorder::cancel() already drops the audio buffer; the streaming
  worker observes the buffer ending and shuts down cleanly.

## Steps

1. **Settings.** In `src-tauri/src/settings.rs`, add to `HotkeyConfig`:
   ```rust
   #[serde(default = "default_true")]
   pub cancel_via_ctrl_alt: bool,
   #[serde(default)]
   pub cancel_via_esc: bool,
   ```
   Plus `fn default_true() -> bool { true }`. Update the `Default` impl
   accordingly. Verify specta-generated `bindings.ts` regenerates
   correctly (it should, since the struct is already `#[derive(Type)]`).

2. **TranscriptionWorker abort path.** In `src-tauri/src/transcribe.rs`:
   - Add `active: parking_lot::Mutex<Option<std::process::Child>>` to the
     worker struct.
   - In `transcribe()`, replace `Command::output()` with the equivalent
     spawn-then-wait pattern: `Command::spawn()`, store the Child in
     `self.active`, call `child.wait_with_output()` to read result,
     then clear `self.active`. The behavior is identical for the
     happy path; the only new observable is that another thread can
     reach into `self.active` and kill the child.
   - Add `pub fn abort(&self)` that takes the Mutex and, if a Child is
     present, calls `child.kill()` (best-effort, log at warn on
     failure but do not return Err).

3. **Recorder cancel from Transcribing.** In `src-tauri/src/recorder.rs`:
   - Extend `cancel()` to accept the `Transcribing` state. Implementation:
     call into the held `Arc<TranscriptionWorker>` (or however the
     worker is reached from Recorder — follow the existing wire) and
     invoke `worker.abort()`. Then transition to `Ready` and clear
     internal state.
   - From any state outside `Recording` and `Transcribing`, log at
     debug and return Ok. Cancel is idempotent.

4. **Hotkey chord detection.** In `src-tauri/src/hotkey.rs`:
   - Add a new helper `cancel_chord_active(flags: CGEventFlags) -> bool`
     that returns true iff exactly `Control|Alternate` are set with no
     other modifier bits.
   - Inside the existing CGEventTap callback, on every `FlagsChanged`
     event:
     - If the new flag state activates the cancel chord and the prior
       state didn't, record `Instant::now()` in a thread-local cell
       (or a `Cell<Option<Instant>>` captured in the closure).
     - If the new flag state deactivates the cancel chord, clear the
       cell.
   - On every `KeyDown` event, clear the cell (any other key arriving
     means this isn't a deliberate Ctrl+Alt-alone gesture). Also, if
     `cancel_via_esc` is true and the keycode is `0x35` (Esc), fire
     the cancel path immediately.
   - In a separate small thread (or piggybacking on the existing
     audio-level broadcast loop in `lib.rs`, which already polls every
     50 ms while recording), check: if the cell holds an Instant >=
     300 ms old AND the flags still match the chord AND
     `cancel_via_ctrl_alt` is true AND the recorder is in `Recording`
     or `Transcribing`, fire cancel and clear the cell.
   - The cancel path: call `recorder.cancel()`, log at info, emit
     `recording-cancelled` (no payload), reset tray to Idle.

5. **`cancel_recording` Tauri command.** In `src-tauri/src/lib.rs`,
   register a new command:
   ```rust
   #[tauri::command]
   fn cancel_recording(state: tauri::State<...>) -> Result<(), String> { ... }
   ```
   Body just calls into the recorder. Add it to the `invoke_handler`
   macro and to the specta builder so it appears in `bindings.ts`.
   No frontend caller in this task — it's wired for future UI use.

6. **Frontend settings UI.** In `src/App.svelte`, find the existing
   Hotkey settings section. Below it, add a "Cancel recording"
   subsection with two labeled checkboxes bound to
   `cfg.hotkey.cancel_via_ctrl_alt` and `cfg.hotkey.cancel_via_esc`.
   Use the same row/checkbox pattern already in the file. Add a small
   `text-[var(--text-secondary)]` note under the Esc checkbox saying
   "Esc may conflict with modal dialogs and other apps."

7. **Overlay listener.** In `src/Overlay.svelte`, add:
   ```js
   listen('recording-cancelled', () => {
     mode = 'idle';
     draw();
   }).then(u => uns.push(u));
   ```
   Same shape as the existing `recording-discarded` listener.

8. **Tests.** In `src-tauri`:
   - `cargo test --manifest-path src-tauri/Cargo.toml`
   - `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`
   - Unit test `cancel_chord_active` against canned `CGEventFlags`
     values: returns true for exactly `Control|Alternate`, false for
     `Control|Alternate|Shift`, false for `Control` alone, false for
     `Alternate` alone, false for empty.
   - Unit test that `HotkeyConfig::default()` has
     `cancel_via_ctrl_alt = true` and `cancel_via_esc = false`.
   - Unit test that `TranscriptionWorker::abort()` on a worker with no
     active Child is a no-op (returns cleanly, no panic).
   - Skip integration tests for the actual chord-debounce timing —
     that requires a real CGEventTap and can't be exercised
     headlessly. Document this gap in the worker's return notes.

## Success signal
- `cargo build`, `cargo test`, `cargo clippy -D warnings` all green for
  `src-tauri`. `npm run build` also green for the frontend.
- `bindings.ts` regenerates with `cancel_via_ctrl_alt` and
  `cancel_via_esc` fields on `HotkeyConfig`.
- `grep -n "recording-cancelled" src-tauri/src` shows at least one
  emit site.
- `grep -n "recording-cancelled" src` shows at least one frontend
  listener.
- `grep -n "abort" src-tauri/src/transcribe.rs` shows the new method.
- Settings UI has two new checkboxes under Hotkey, defaults match
  spec (Ctrl+Alt on, Esc off).

## Manual verification (defer to user)
The full UX cannot be verified by an agent — it requires keyboard input
and observation of the overlay clearing in real time. After landing,
user should test:

1. **Ctrl+Alt cancel during Recording.** Hold the PTT key, speak briefly,
   then while still recording press and hold Ctrl+Alt for ~400 ms with
   no other key. Expected: overlay clears immediately, no transcript
   appears in history, dev terminal shows
   `[recorder] Recording → Ready (cancel)` and an emitted
   `recording-cancelled` event.

2. **Ctrl+Alt cancel during Transcribing.** Record a normal short
   utterance and release. Immediately after release, while
   `[recorder] Recording → FinalizingAudio` and `Transcribing` are
   visible in the dev terminal, hold Ctrl+Alt for 400 ms. Expected:
   whisper-cli subprocess is killed (visible in Activity Monitor or
   `ps`), no transcript event, no paste, recorder returns to Ready.

3. **Ctrl+Alt false-positive guard.** Record normally. Type
   `Ctrl+Alt+T` in your editor (or any app shortcut). Expected: the
   chord doesn't fire because the third key (T) arrives within 300 ms;
   recording proceeds and transcribes normally.

4. **Esc cancel (with toggle on).** Enable `cancel_via_esc` in
   settings. Start recording, press Esc once. Expected: instant
   cancel, no 300 ms hold required.

5. **Esc with toggle off (default).** Verify pressing Esc during a
   recording does *not* cancel — the recording completes normally.

6. **Both toggles on.** Verify both gestures work and don't interfere.

## Notes
- **PTT-key collision edge case.** If the user's PTT key is
  `right_option`, the `Alternate` flag is held for the full duration
  of every recording. Pressing Ctrl on top of that would briefly
  produce `Control|Alternate` flags. The 300 ms debounce + "no other
  key during the hold" guard should prevent the typical accidental
  trip — to actually fire cancel, the user has to hold both Ctrl and
  Right Option together for 300 ms with their hands deliberately
  posed. Document this in the task notes; if it turns out to be a
  real friction point, the future fix is to compute the cancel chord
  dynamically from the configured PTT key (e.g. fall back to
  Ctrl+Cmd if PTT is Alt).
- **Why not also kill cleanup.rs's HTTP call.** The Ollama call
  already has a 2 s timeout (TASK-4). Wiring an abort through reqwest
  is fiddly and the worst-case wait is brief. Out of scope.
- **Why hold Child handle in a Mutex.** The `transcribe()` thread is
  inside `wait_with_output()` waiting on the subprocess. The cancel
  thread needs a reachable handle to kill the Child. Mutex around an
  Option is the cheapest correct primitive; we don't need lock-free
  here because cancel is rare and the contention is for nanoseconds.
- **Re-run the burn-in protocol after landing.** If you've been
  using the app daily (the user has, ~2 days), test the cancel
  gesture in the real-world workflow before declaring done. The
  failure mode that matters most is *false-positive cancel of a real
  dictation*, which is invisible in unit tests.
- Multi-agent review reference: this task picks up the
  `recording-discarded` / overlay-clearing pattern from TASK-5/6/22
  and the Recorder typed-state machine from TASK-5.
