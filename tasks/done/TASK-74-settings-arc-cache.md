# TASK-74: Settings cache: Arc<Config> + narrow hot-path accessors

## Goal
Eliminate full `Config` clones on hot paths by swapping the settings cache from `RwLock<Option<Config>>` to `RwLock<Arc<Config>>` and adding narrow field accessors for the 20 Hz / 10 Hz / per-PTT-press paths that currently clone the entire config.

## Context
The process-wide settings cache in `src-tauri/src/settings.rs:590-624` is `RwLock<Option<Config>>`. Every call to `settings::load()` acquires a read lock and `.clone()`s the entire `Config` struct. This struct contains `Vec<String>` fields (vocabulary, antivocabulary, models list) plus nested structs. The clone happens on:

- **Every 50ms** — the level thread in `lib.rs:1594` calls `cursor_dot_indicator_enabled()` which acquires the read lock. (This one already has a narrow accessor, but it still goes through the RwLock.)
- **Every PTT-down** — `windowing.rs:291` calls `settings::load()` to read `overlay_position` and `overlay_size`. Full clone of Config including all Vecs.
- **Every IPC command** that touches config — `get_config`, `save_config`, `load_history`, `delete_backend_model`, `prewarm_model`, `model_present` in `permissions.rs:287-302`.
- **Every 1s during onboarding** — `check_readiness()` → `model_present()` → `settings::load()` (full clone).

The correct idiom: `RwLock<Arc<Config>>` so hot-path readers do a lock-free `Arc::clone` (atomic refcount bump, ~10ns) instead of a deep clone of the whole struct. Writers swap the `Arc` for a new one. Additionally, narrow field accessors (like the existing `cursor_dot_indicator_enabled()`) should be added for the other hot-path fields so the level thread and PTT-down path don't even need to touch the `Arc`.

## In scope
- `src-tauri/src/settings.rs` — change cache type, update `load()`, `update_cache()`, add narrow accessors
- `src-tauri/src/lib.rs:1594` — level thread hot path (may use the new narrow accessor)
- `src-tauri/src/windowing.rs:291` — PTT-down hot path
- `src-tauri/src/permissions.rs:287-302` — `model_present()` readiness poll
- `SESSION-STATUS.md`

## Out of scope
- Changing the `Config` struct definition
- Changing the TOML serialization format
- The `save_config` worker-invalidation gating (TASK-73 handles that)
- Frontend changes
- History caching (separate concern)

## Steps
1. Read `src-tauri/src/settings.rs:580-630` to understand the current cache: `static CACHE: RwLock<Option<Config>>`. `load()` checks the cache, returns a clone if warm, falls back to disk. `update_cache()` writes a new `Config` into the cache.
2. Change the cache type to `RwLock<Option<Arc<Config>>>`. Update `load()` to return `Arc<Config>` (clone the Arc, not the Config). Callers that need an owned `Config` can call `(*arc).clone()` or deref.
3. Update `update_cache()` to wrap the new config in `Arc::new()`.
4. Audit all `settings::load()` callers. Most can take `&Config` via `Arc::as_ref()` or deref. The ones that need an owned `Config` (e.g. `save_config` which serializes it) can still clone — they're not hot paths.
5. Add narrow field accessors for the hot-path fields that the level thread and PTT-down path read. Model these after the existing `cursor_dot_indicator_enabled()`:
   - `overlay_position() -> OverlayPosition` (or whatever the field type is — read `windowing.rs:291` to see what it reads)
   - `overlay_size() -> OverlaySize` (same)
   - These should acquire the read lock, extract the single field, drop the lock. No clone of the full Config.
6. Update `windowing.rs:291` to call `settings::overlay_position()` and `settings::overlay_size()` instead of `settings::load()`.
7. Update `permissions.rs:287-302` (`model_present()`) to read only `backend` + `backend_variant` via narrow accessors or by taking an `&Config` from the `Arc` without cloning the whole struct.
8. The level thread at `lib.rs:1594` already uses `cursor_dot_indicator_enabled()` — verify it still works with the new `Arc` cache (it should, since the accessor just reads one field under the lock).
9. Run `cargo check --manifest-path src-tauri/Cargo.toml` and `cargo clippy`.
10. Run `npm run typecheck`.
11. Update `SESSION-STATUS.md`.

## Success signal
- `cargo check` and `cargo clippy` pass.
- `settings::load()` returns `Arc<Config>`, not `Config`.
- The hot-path callers (level thread, PTT-down, `model_present`) do NOT clone the full `Config` struct — they either use `Arc` deref or narrow accessors.
- `grep -rn "settings::load()" src-tauri/src/ | grep -v "Arc"` shows no callers expecting an owned `Config` from the hot paths.
- Toggling settings while recording does not cause a Config Vec allocation on the level thread.

## Notes
- `Arc::clone()` is a single atomic increment (~10ns on ARM64/x86_64). `Config::clone()` deep-clones every Vec and String — potentially hundreds of nanoseconds to microseconds depending on vocabulary size.
- Writers (settings updates) are rare (user toggles). Readers are frequent (20 Hz level thread, per-PTT-press). `RwLock<Arc<Config>>` is the standard pattern for this read-heavy/write-rare cache.
- The cold-cache fallback (`load_detailed()` from disk) should still work — it reads from disk, wraps in `Arc::new()`, writes to cache, returns the `Arc`.
- Be careful with the `serial_test` dev-dependency — tests that mutate the cache (`settings.rs` tests) will need to use `Arc` instead of owned `Config` in their assertions. Use `(*arc).clone()` or compare fields directly.
- The `parking_lot::RwLock` is already used (not std). parking_lot RwLock is faster than std for this read-heavy pattern. No change needed.
