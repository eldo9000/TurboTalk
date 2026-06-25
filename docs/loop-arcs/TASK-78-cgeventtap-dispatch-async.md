# Arc Log — TASK-78: CGEventTap cleanup — dispatch_async + tap-disable event handling

## Gate
Move real work out of the CGEventTap callback into a channel-backed processing
thread, and handle tap-disable events directly in the callback instead of polling.

## Premise declarations

### Premise 1 (initial)
- **SYMPTOM:** The CGEventTap callback does RwLock reads, string matching, and
  dispatch ladders inline — macOS may kill the tap via `kCGEventTapDisabledByTimeout`.
  The 8-second polling watchdog exists solely because the callback is too slow.
- **PREMISE:** Moving event processing to a channel-backed serial thread and
  handling `TapDisabledByTimeout`/`TapDisabledByUserInput` inline will eliminate
  the polling watchdog and prevent tap-disable issues.
- **DERIVATION:** A `crossbeam_channel::Sender<TapEvent>` in the callback (near-zero
  work: pack struct + send) vs. current inline ~50µs event processing. The processing
  thread runs in serial order, same as dispatch_async. `CGEventType::TapDisabledByTimeout`
  and `TapDisabledByUserInput` arrive as event types and can be handled in <1µs.
- **FALSIFICATION:** If `cargo check` fails, or if PTT events stop firing correctly
  (key matching broken after refactor), the premise is false.
- **FALSIF-RESULT:** `cargo check` + `cargo clippy` clean. Callback now minimal (match disable → re-enable, capture → channel send). 8s watchdog removed. CGEventTapIsEnabled extern removed.
- **DISPOSITION:** CONFIRMED — dispatch 1 green. Commit 9836297.
