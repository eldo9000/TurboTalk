# TASK-5: Audio buffer optimization — clone deferral, helper reuse, heap alloc fix

## Goal
Eliminate unnecessary memory allocation and copying in the audio pipeline: defer the native sample buffer clone, reuse the warm-stream helper instead of duplicating code, and fix a heap-allocation-in-callback violation.

## Context

### Issue 1: stop() clones native sample buffer unconditionally (audio.rs:1018)
`stop()` clones the full native sample buffer regardless of whether the batch fallback path uses it. For a 2-minute 48kHz stereo recording, that's a ~44MB copy on the post-release critical path, wasted whenever streaming succeeds (the normal case). The `samples` buffer isn't cleared until the next `start()`, so the clone is safe to defer into the fallback branch only.

Fix: move the clone from the top of `stop()` into the branch where the batch fallback is actually used.

### Issue 2: stop() duplicates arm_or_close_warm_stream() (audio.rs:994-1004)
`stop()` has an inline copy of the `arm_or_close_warm_stream()` logic. The helper exists precisely so these two sites can't drift. Replace the inline code with a call to the helper.

### Issue 3: i16/u16 cpal callbacks heap-allocate Vec per callback (audio.rs:598,615)
The i16 and u16 `cpal` callback arms allocate a fresh `Vec` on every audio callback invocation. The f32 path honors a CALLBACK-ALLOWED-OPS discipline (no heap allocations in the audio thread). Most macOS devices use f32 so this rarely triggers, but it's a latent violation. Fix with a reusable scratch buffer (e.g., a pre-allocated `Vec` with capacity, cleared rather than reallocated).

## In scope
- `/Users/eldo/Downloads/Github/TurboTalk/src-tauri/src/audio.rs`

## Out of scope
- Any other files
- `audio_finalizer.rs` (the resampled_buf retention issue is a separate concern)
- `recorder.rs`

## Steps
1. Read `audio.rs` fully. Locate:
   a. The `stop()` function — specifically the clone of the sample buffer (~line 1018) and the inline warm-stream logic (~line 994-1004).
   b. The `arm_or_close_warm_stream()` helper function.
   c. The cpal callback handler, specifically the i16/u16 sample format arms (~line 598, 615).
2. **Fix Issue 1**: Move the `samples.clone()` from its current position in `stop()` into the specific branch where the batch fallback path uses the cloned buffer. Ensure the clone only happens when streaming failed and batch is the fallback.
3. **Fix Issue 2**: Replace the inline warm-stream logic in `stop()` with a call to `arm_or_close_warm_stream()`. Verify the helper's signature matches the call site's needs.
4. **Fix Issue 3**: In the i16/u16 callback arms, replace the per-callback `Vec::new()` with a pre-allocated buffer. Options:
   - Use a reusable `Vec<f32>` stored alongside the callback state, cleared with `.clear()` each callback.
   - If the callback state is already behind an `Arc<Mutex<>>`, add the scratch buffer there.
   - If restructuring is too invasive, at minimum convert `Vec::new()` to `Vec::with_capacity(samples.len())` to avoid reallocation on push — though the ideal is zero allocation per callback.
5. Run `cargo check` in `src-tauri/` to verify compilation.

## Success signal
`cargo check` passes. The native sample buffer clone only happens in the batch-fallback branch. `stop()` calls `arm_or_close_warm_stream()` instead of inlining. The i16/u16 callback paths either use a reusable scratch buffer or at minimum pre-allocate with capacity.

## Notes
- The callback-allowed-ops discipline means no `Arc` clone, no `Mutex` lock, no allocation in the audio callback thread. f32 path already honors this.
- If the scratch buffer refactor requires changing the callback state struct, that's acceptable — just keep it minimal.
- `arm_or_close_warm_stream` might have a different name in the actual code — search for the helper that manages the warm stream lifecycle.
