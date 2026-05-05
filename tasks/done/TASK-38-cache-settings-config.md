# TASK-38: Cache settings config to skip per-press file I/O

## Goal
Reading the user's configured audio device on each PTT-down stops re-parsing the on-disk config file. The config is loaded once at app start, kept in memory, and refreshed only when `save_config` writes a change.

## Context
`src-tauri/src/audio.rs::AudioCapture::start()` calls `crate::settings::load()` on every press to read `cfg.audio.device`. `settings::load()` re-reads and re-parses the JSON config file from disk every call — small (5–50 ms typical, faster on hot OS cache) but unnecessary work on the recording-start critical path.

This task is small and independent of TASK-36 and TASK-37. The wins are:
- A few ms shaved off every PTT-down (the OS file cache makes most reads cheap, but not all).
- Eliminates a real-but-rare failure mode: if the config file is being rewritten by `save_config` exactly when `start()` reads it, the partial-read could surface as a parse error. (This is unlikely given the small file size and atomic-write guarantees, but caching removes the question.)

The right shape: an `Arc<RwLock<Config>>` (or `parking_lot::RwLock<Config>`) managed as Tauri app state. Pre-existing `HotkeyState` in `src-tauri/src/lib.rs:630` already follows this exact pattern — copy it.

`save_config` in `src-tauri/src/lib.rs:62` already takes a `tauri::State<'_, HotkeyState>`. Add a parallel `tauri::State<'_, ConfigState>` that holds the full config and is updated alongside `hotkey_state` on each save.

`AudioCapture` does not have access to Tauri state directly. The cleanest route: make `settings::load()` return from the cache when one is initialized, and fall back to disk read when not. Use a `OnceCell` or a `parking_lot::RwLock<Option<Config>>` static for the cache. Initialize from the setup function in `lib.rs`. `save_config` updates the cache in addition to writing to disk.

Failure mode to design out: if the cache is never initialized (e.g. `AudioCapture::start()` is called before `lib.rs::run()` has populated it), `settings::load()` should still work — fall back to the disk read. So make it strictly an optimization, never a hard dependency.

## In scope
- `src-tauri/src/settings.rs` — add a process-wide cache (RwLock<Option<Config>>), make `load()` consult the cache, add `prime_cache()` and `update_cache()` helpers
- `src-tauri/src/lib.rs` — call `prime_cache()` once in setup; update the cache inside `save_config` after the disk write succeeds

## Out of scope
- `src-tauri/src/audio.rs` — no call-site changes needed; `settings::load()` keeps the same signature
- `src-tauri/src/recorder.rs` — no changes
- Any other consumer of `settings::load()` — they all transparently benefit
- Changing the on-disk format
- Atomic-write guarantees of `settings::save()` — out of scope here

## Steps
1. Open `src-tauri/src/settings.rs`. Find `pub fn load() -> Config`.
2. At module scope, add:
   ```rust
   use parking_lot::RwLock;
   use std::sync::OnceLock;
   static CACHE: OnceLock<RwLock<Option<Config>>> = OnceLock::new();
   fn cache() -> &'static RwLock<Option<Config>> {
       CACHE.get_or_init(|| RwLock::new(None))
   }
   ```
3. Modify `load()`:
   - Check the cache. If `Some(cfg)`, return a clone.
   - Else fall through to today's disk-read logic; on success, populate the cache and return the value.
4. Add `pub fn prime_cache()`:
   - Call `load()` once. The first call will hit disk and populate the cache. Subsequent reads are RAM.
5. Add `pub fn update_cache(cfg: &Config)`:
   - Replace cache contents with `Some(cfg.clone())`. Call this from `save_config` after `settings::save(&cfg)` succeeds.
6. Add `pub fn invalidate_cache()`:
   - Replace cache contents with `None`. Call this from any path that knows the on-disk file has been changed externally (none today, but useful for tests and for future auto-reload).
7. In `src-tauri/src/lib.rs`:
   - In the setup closure, immediately after the existing `let cfg = settings::load();` (or its equivalent), call `settings::prime_cache();` if `load()` itself does not populate the cache on first call. (If `load()` always populates the cache as part of step 3, this is a no-op — just call `prime_cache()` defensively. It's idempotent.)
   - In `save_config`, after `settings::save(&cfg).map_err(|e| e.to_string())?;` succeeds, call `settings::update_cache(&cfg);`. Place the call before the `apply_overlay_visibility` and `transcribe::invalidate_worker()` calls, so subsequent reads in those paths see the new value.
8. Build: `cargo build --manifest-path src-tauri/Cargo.toml`. Resolve compile errors.
9. Run tests: `cargo test --manifest-path src-tauri/Cargo.toml`. All existing tests pass.
10. Add a small test in `settings.rs`:
    - `prime_cache()` then `load()` returns the same struct.
    - `update_cache(&modified)` then `load()` returns the modified struct.
    - `invalidate_cache()` then `load()` re-reads from disk.
    - The test must `use serial_test::serial` if multiple cache-touching tests exist, since `CACHE` is process-wide. Add `serial_test = "3"` to `[dev-dependencies]` if not already present.
11. Manual smoke test: change the audio device in Settings, save, then press PTT. Verify the new device is used (the existing tracing log `[audio] opening stream: "DeviceName"` will show the new name).

## Success signal
- `cargo build --manifest-path src-tauri/Cargo.toml` exits 0.
- `cargo test --manifest-path src-tauri/Cargo.toml` exits 0; new cache tests pass.
- Manual test: changing the input device in Settings → Audio and pressing PTT immediately uses the new device on the very next press (no app restart needed). The tracing log `[audio] opening stream: "<new name>"` confirms.
- A microbenchmark or just a tracing line at `start()` shows that the per-press `settings::load()` time is now sub-microsecond (RAM read) on warm-cache calls; the original disk-read time only applies on the first press of an app session.

## Notes
- The cache is a process-wide static. Tests that mutate it must be serial (`serial_test` crate) to avoid interleaving.
- `Config` is `Clone` already (see existing usage). Cloning on read is a few hundred bytes; cheap.
- `parking_lot::RwLock` is already a dependency (used elsewhere). No new crate needed.
- `OnceLock` is `std::sync::OnceLock` — stable since 1.70. The repo's MSRV should be fine; if not, swap for `once_cell::sync::OnceCell` (likely already a transitive dep).
- This task can run in any order relative to TASK-36 and TASK-37. Doing it before TASK-36 is fine; doing it after is also fine. It is not a blocker for either.
