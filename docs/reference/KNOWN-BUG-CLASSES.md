## cancel-epoch-TOCTOU
Cancel (Escape / Ctrl+Alt hold) increments `CANCEL_EPOCH` while the hotkey
thread is inside `rec.stop()` or its Mutex::take() calls. If the epoch is
read *after* `stop()` returns, the observed value may already include the
cancel — and all subsequent `job_cancelled_since` checks never fire, so the
text is pasted even though the user cancelled.

**Fix (TASK-23):** capture `cancel_epoch()` into a local *before* calling
`rec.stop()`. The pre-stop epoch is the one checked throughout the rest of
the handler. The same pattern is used for the salvaged-transcript path.

**Related:** `finish_guarded()` in `recorder.rs` handles the inverse race —
if a cancel + rapid re-press arrive between `rec.stop()` succeeding and
the finish call, calling `finish()` would corrupt the new job's state.
`finish_guarded()` skips the `Ready` transition when the recorder is
already in `Recording`.

## paste-delay-tuning
Cmd+V keystroke injection and clipboard restoration race on busy main
threads. Heavyweight apps (Electron, Xcode, etc.) with a saturated main
thread can miss the paste if clipboard is restored too soon — the app
reads the restored content instead of the pasted text.

**Tuning history:** 150 ms was too tight. 500 ms was chosen because the
full dictation cycle is ~2-3 s anyway (transcription + cleanup), so the
extra 350 ms is imperceptible. The paste delay comment should state the
invariant ("500 ms to let the paste land before restoring clipboard"),
not the tuning history.

## CoreAudio-buffer-sleep
After setting `is_recording = false`, the hotkey thread sleeps briefly
to let the last in-flight CoreAudio callback finish before stopping the
stream. If the sleep is too short, the final samples may be lost.

**Tuning history:** was 25 ms for headroom; reduced to 10 ms because
one full CoreAudio buffer cycle (~10 ms at default settings) is
sufficient. The comment should state the invariant ("wait for the last
in-flight callback"), not the old value.

## whisper-temperature-inc-hallucination
`whisper-server` temperature-fallback retry (`temperature_inc > 0`)
produces "same phrase 3×" repetition output on short or silent audio.
This was carried over from the old `whisper-cli` config (commit
`55cfa21`) and was lost-then-re-discovered during the TASK-47 server
transition.

**Fix:** set `temperature_inc=0` explicitly in the HTTP POST form to
disable the fallback.

## quick-tap-race
In hold-to-talk mode, a very quick press/release can race: the key-up
thread is scheduled before the key-down thread's `rec.start()` call
completes. Without a synchronization mechanism, `ptt_up` sees the
recorder still in `Ready`, fails its `stop()` call, and the overlay
stays stuck forever.

**Fix (TASK-2):** a `CANCEL_PENDING` atomic flag. `ptt_up` sets it on
IllegalTransition from Ready; `ptt_down` checks the flag immediately
after `rec.start()` succeeds and cancels the recording instead of
showing the overlay. A second arming cancel path (`CANCEL_ARMING`) exists
for the toggle-mode arming phase, where the key-down thread is still
polling prewarm readiness.

A stale-flag variant: orphaned key-up from a previous cancelled arming
can leave `CANCEL_PENDING` / `CANCEL_ARMING` set. Every new `ptt_down`
clears both flags at entry before checking prewarm state.

## device-lost-deferred-cancel
When the audio device disappears mid-recording, the level-broadcast
thread detects it and calls `recorder.cancel()`. In hold mode, the
subsequent key-up should be a no-op — but after a device-lost cancel,
the `ptt_up` suppression arm was skipped, so the trailing key-up hit
IllegalTransition and set `CANCEL_PENDING`, which would fire on the
*next* press instead of being silent.

**Fix:** arm `ptt_up` suppression in the device-lost block in `lib.rs`,
mirroring the existing `trigger_cancel` callers.

## SVE-cmake-probe-hang
check_cxx_source_runs for ARM SVE hangs on macOS arm64 (Apple M4/M3/M2/M1 have no SVE).
Fix: patch check_cxx_source_runs → check_cxx_source_compiles in ggml/src/ggml-cpu/CMakeLists.txt.
Applies to: whisper.cpp cmake, any ggml-based cmake build on macOS arm64.

## WH_KEYBOARD_LL-needs-message-pump
`SetWindowsHookExW(WH_KEYBOARD_LL, ...)` installs a hook whose callback fires
only when the installing thread pumps messages via `GetMessageW` / `PeekMessageW`.
If the thread never enters a message loop (e.g. it calls `thread::sleep` in a
polling loop), the callback silently never fires — the hook is installed and
`UnhookWindowsHookEx` succeeds, but zero events are delivered.

**Previous workaround:** `GetAsyncKeyState` polling at ~125 Hz was used
because the hook appeared broken. The real problem was the missing message pump.

**Fix (TASK-78):** Replace the polling loop with a dedicated thread that:
1. Calls `SetWindowsHookExW(WH_KEYBOARD_LL, callback, NULL, 0)`
2. Runs `GetMessageW` in a loop (the callback fires inside this call)
3. On shutdown, posts `WM_QUIT` to break the loop, then `UnhookWindowsHookEx`

**Key detail:** `WH_KEYBOARD_LL` is a *system-global* hook (the thread ID is 0).
The callback receives a `KBDLLHOOKSTRUCT` with `vkCode`, `flags` (including
`LLKHF_INJECTED` to detect synthetic input), and the `WM_KEYDOWN`/`WM_KEYUP`/
`WM_SYSKEYDOWN`/`WM_SYSKEYUP` message types. The hook handle parameter to
`CallNextHookEx` is ignored for low-level hooks (can pass NULL).

## save-config-unconditional-worker-rebuild
`save_config` unconditionally called `transcribe::invalidate_worker()` + `transcribe::prewarm()` on every config save, destroying the warm whisper-server/Parakeet worker even when only non-backend fields changed (theme, sound, overlay, cursor dot, etc.).

**Fix (TASK-73):** Capture the previous config via `settings::load()` before `settings::update_cache()`, then gate the invalidation on a before/after comparison of only the five backend-affecting fields: `backend`, `backend_variant`, `whisper.model`, `whisper.vad_enabled`, `cleanup.vocabulary`. All other fields persist to disk and update the cache normally but skip the worker rebuild.

**Key detail:** `cfg.whisper.bin` is never read in production code (`WhisperBackend::from_config` calls `find_whisper_server("whisper-server")` with a hardcoded string). First save after cold start still invalidates (safe fallback — `Config::default()` comparison).

## unconditional-clipboard-restore
The pbcopy/pbpaste-based clipboard path always restores unconditionally (no
changeCount guard), silently clobbering the user's clipboard if they copy during
the 200 ms paste window. On macOS the `clipboard.rs` `restore_if_untouched` only
checks whether `prior_text` is `Some` — it never checks the NSPasteboard changeCount.
On Windows `win_clipboard::restore` has no sequence-number check.

**Fix (TASK-N/A):** macOS now dispatches the already-written native NSPasteboard
module (`clipboard::native`) to the main thread via `app.run_on_main_thread()`
with a channel-based sync. The native module reads `changeCount` at snapshot time
and compares on restore. Falls back to pbcopy/pbpaste on dispatch failure.

Windows now captures `GetClipboardSequenceNumber()` at snapshot time and checks
it before restoring. Returns `Ok(false)` when the clipboard changed.

## chime-subprocess-spawn
`play_chime` spawned `afplay` (macOS) or `powershell` (Windows) on every
chime event. Subprocess startup overhead (~10-30ms afplay, ~150-400ms
powershell) is unnecessary for short system sounds with native APIs
available.

**Fix:** macOS: `NSSound::soundNamed_()` + `setVolume:` + `play()` via
`objc2::msg_send!`. Windows: `PlaySoundW` with `SND_ALIAS | SND_ASYNC |
SND_NODEFAULT`. Both are fire-and-forget, no subprocess spawn.

## config-clone-not-a-bug-class
`settings::load()` used to deep-clone the entire `Config` struct on every call.
Swapped to `RwLock<Option<Arc<Config>>>` + narrow field accessors for hot-path
readers (overlay positioning, audio device/idle timeout, pause media toggle).
This is a performance optimisation, not a bug fix — no functional change.
`load()` now returns `Arc<Config>`; callers that need owned `Config` explicitly
clone via `(*load()).clone()`. Auto-deref handles field access for the rest.

## callback-mutex-realloc-ring-buffer
The cpal audio callback acquired `parking_lot::Mutex` on `samples: Vec<f32>` and
called `extend_from_slice(data)` which can reallocate the backing buffer. On a
real-time audio thread this is a latency source (lock contention if the feeder
thread also accesses the Vec, and allocation jitter).

**Fix:** Replace `Mutex<Vec<f32>>` with an `rtrb` lock-free SPSC ring buffer
(`Producer` / `Consumer`). The callback calls `Producer::push_partial_slice()`
— a lock-free memcpy into pre-allocated slots, no allocation, no realloc. The
feeder thread reads from the `Consumer` side. The ring is sized at
`RING_CAPACITY` (480k f32 samples ≈ 5 s at 48 kHz stereo). Recordings longer
than the ring are fine — the feeder drains the consumer every ~10 ms.

**Secondary fix:** Add `Stream::pause()` after `stop()` / `cancel()` and
`Stream::play()` on `start()` warm-stream reuse. When idle the cpal callback
stops firing entirely (no wakeups, no preroll accumulation, no mutex traffic).
The watchdog timeout is still used as a safety net to eventually drop the
stream.

**Batch fallback path:** The feeder accumulates all chunks into
`samples_accum: Arc<Mutex<Vec<f32>>>`. On streaming path degradation,
`stop()` reconstructs the full recording from `samples_accum` + any remaining
data in the ring consumer. No recording is lost.

## dedup-reactivity-constants
Duplicated TypeScript constants (PROMPT_PRESETS, KNOWN_FILENAMES, seg helper)
across App.svelte/ModesTab/ModelsTab cause drift risk. Save handlers call
redundant getConfig IPC. Tab switch handlers re-fetch config unnecessarily.
trackWindowHeight not debounced. Overlay polls cursorPosition via IPC every
100ms. Overlay rebuilds levels array per frame (slice+spread).

**Fix (TASK-79):**
- Extract shared constants into `src/lib/prompts.ts`, `src/lib/catalog.ts`,
  `src/lib/utils.ts`; all consumers import from there.
- Replace state-object factory functions (`historyState()`, `modelsState()`,
  `modesState()`) with individual $state/$derived props on tab components.
- Build full config from local state in save handlers (no getConfig IPC).
- Debounce trackWindowHeight at 150ms and focus-based recheckReadiness at 250ms.
- Replace Overlay cursorPosition polling with mousemove event listener.
- Replace Overlay levels slice+spread with ring buffer (levelsHead index).
- Track all setTimeout IDs in `pendingTimeouts` Set; clear on unmount.

## CoreML-dyld-init-hang
Building whisper.cpp with `WHISPER_COREML=1` links `libwhisper.coreml.dylib` into
`libwhisper.1.dylib`, which pulls in `CoreML.framework` at **dyld load time** — before
`main()`. On Apple Silicon this can block process startup for ~60 s on every cold start,
even when no `.mlmodelc` encoder artifact is present and CoreML is never used.

**Symptom:** `whisper-server` or any binary loading the CoreML-linked `libwhisper.1.dylib`
appears hung immediately after spawn; no transcription logs yet.

**Mitigation (shipped):**
- Default bundle uses **Metal-only** Homebrew whisper (`npm run refresh-whisper-server`).
- `scripts/preflight.mjs` and `scripts/refresh-whisper-server.mjs` reject binaries whose
  dylib chain references `CoreML.framework` or `libwhisper.coreml.dylib`.
- Optional CoreML path (TASK-48 phase 2) must use a **separate sidecar** spawned only when
  the user opts in — never replace the default Metal `libwhisper.1.dylib`.

**Re-attempt gate:** Documented optional sidecar design in `docs/reference/COREML-BLOCKER.md`.
Do not merge a CoreML-linked default sidecar until a timed proof shows dyld init ≤ 2 s without
`.mlmodelc`, or the opt-in sidecar path is implemented and bench-validated.

**Related upstream:** whisper.cpp issues on ANECompilerService hangs during CoreML *model*
load (distinct from this dyld-init class, but same ANE subsystem).
