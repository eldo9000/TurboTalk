# Arc Log — TASK-74: Settings cache: Arc&lt;Config&gt; + narrow hot-path accessors

## Gate
Swap the settings cache from `RwLock<Option<Config>>` to `RwLock<Option<Arc<Config>>>`
and add narrow field accessors for hot-path readers (level thread, PTT-down, audio
device switch) so they avoid deep-cloning the entire `Config` struct.

## Premise declarations

### Premise 1 (initial)
- **SYMPTOM:** `settings::load()` deep-clones the entire `Config` struct (including
  `Vec<String>` fields for vocabulary, antivocabulary, models) on every call. This
  happens on the 20 Hz level thread (cursor dot), every PTT-down (overlay positioning),
  every per-second readiness poll, and every audio start (device/idle-timeout read).
- **PREMISE:** Changing the cache to `RwLock<Option<Arc<Config>>>` so `load()` returns
  an `Arc<Config>` (atomic refcount bump, ~10ns) instead of a deep `Config::clone()`,
  and adding narrow field accessors for the hot-path fields (overlay_position,
  overlay_size, pause_media_on_dictate, idle_timeout_secs, audio_device), will
  eliminate expensive deep clones on all hot paths while preserving correct behavior.
- **DERIVATION:** `Arc::clone()` is ~10ns atomic increment. `Config::clone()` allocates
  every Vec and String — potentially hundreds of ns to µs. The read-heavy/write-rare
  access pattern (50ms level thread, per-PTT-press positioning vs. rare user toggles)
  matches the standard `RwLock<Arc<T>>` idiom.
- **FALSIFICATION:** If `cargo check` fails with type errors at any call site, or if
  a hot-path caller still deep-clones Config (verified by grep for load().field pattern
  that wasn't converted to narrow accessor), the premise is false.
- **FALSIF-RESULT:** `cargo check` clean (pre-existing keyboard_layout.rs warnings only), `cargo clippy` clean (no new warnings). 20+ call sites compile.
- **DISPOSITION:** CONFIRMED — dispatch 1 green. Commit 2d149c7.
