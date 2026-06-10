# TASK-64: Harden local privacy file and directory permissions

## Goal
TurboTalk's local config, history, model, and diagnostic files should be created under owner-private directories where the platform supports it. In particular, `history.json` stores verbatim dictation and must not be readable by other local users on shared machines.

## Context
The audit found:

| Audit item | Severity | Surface | Current gap |
|------------|----------|---------|-------------|
| #5 | Medium | `history.json` | Verbatim dictation history is written with default permissions under `~/.config`, and parent directories may be traversable/readable by other local users depending on umask and platform. |

Important correction from the audit:
- Debug transcript logs do not ship in release. `record_transcript` is installed only under `#[cfg(debug_assertions)]`, so this task should not treat debug transcript logging as a release privacy bug.
- Predictable temp names are lower risk on macOS/Windows because temp dirs are per-user, but using private dirs consistently is still good hygiene.

## In scope
- Ensure TurboTalk's app data directory is owner-private on Unix/macOS (`0o700`).
- Ensure sensitive files such as `history.json`, config, and diagnostic logs are owner-readable/writable only on Unix/macOS (`0o600` where appropriate).
- Use Windows-safe behavior that does not break normal `%APPDATA%` use.
- Add focused tests for Unix permission helpers where feasible.
- Update privacy/release docs if they currently imply weaker or stronger behavior than the code provides.

## Out of scope
- Encrypting history at rest.
- Removing history functionality.
- Changing UI history limits or retention policy unless needed to preserve current behavior.
- Reworking model storage paths unless an existing helper already covers it.
- Changing debug-only transcript logging.

## Files to inspect first
- `src-tauri/src/settings.rs`
- `src-tauri/src/diagnostic_log.rs`
- `src-tauri/src/lib.rs`, especially model download directory creation
- `src-tauri/src/transcribe.rs`, only for temp/log path sanity
- `docs/PRIVACY.md`
- `docs/RELEASE-READINESS.md`
- `docs/SMOKE-TEST.md`

## Steps

### 1. Inventory sensitive paths
Build a short list of paths TurboTalk writes locally and classify them:

| Path/function | Contents | Sensitivity | Required permissions |
|---------------|----------|-------------|----------------------|
| `settings::history_path()` | Verbatim dictation history | High | parent `0o700`, file `0o600` on Unix |
| `settings::config_path()` | Preferences, cleanup vocabulary | Medium | parent `0o700`, file `0o600` on Unix |
| diagnostic logs | Error/runtime diagnostics, possible user note | Medium/high | parent `0o700`, file `0o600` on Unix |
| model directories | Downloaded public model files | Low/medium | parent private enough to avoid leaking user app layout; exact file mode less critical |

Do not assume all files are equally sensitive, but prefer private defaults for app-owned data.

### 2. Add permission helpers
In Rust, add small platform-gated helpers rather than scattering permission code.

Suggested shape in `settings.rs` or a small shared module:

```rust
fn create_private_dir_all(path: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn write_private_file(path: &Path, contents: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        create_private_dir_all(parent)?;
    }
    // Prefer create/truncate with explicit mode on Unix to avoid a permissive
    // umask-created window where feasible.
}
```

On Unix/macOS, prefer `OpenOptionsExt::mode(0o600)` for newly created files, then set permissions after write as a belt-and-suspenders measure.

On Windows, do not try to hand-roll ACLs unless the project already has a helper. `%APPDATA%` is user-profile scoped; leave ACL hardening as a future Windows-specific task if needed.

### 3. Apply helpers to history writes
Update `save_history_at()` in `src-tauri/src/settings.rs`:

- Create parent directory with private permissions.
- Write `history.json` with owner-only file permissions on Unix.
- Preserve existing trimming behavior and JSON format.
- Preserve tests that use temp paths.

Add or update tests:
- Saving history creates the file.
- On Unix, parent mode is owner-private enough and file mode does not include group/other permissions.
- Existing malformed history handling remains unchanged.

### 4. Apply helpers to config writes
Find the config save path in `settings.rs` and apply the same private write behavior.

Preserve:
- Config format.
- Cache invalidation/update behavior.
- Any legacy path migration logic.

If config contains cleanup vocabulary, remember that vocabulary may include private names/terms and should be treated as sensitive.

### 5. Apply helpers to diagnostic logs
Inspect `diagnostic_log.rs`:

- Ensure log directory creation is private on Unix/macOS.
- Ensure generated bug report local copies are written `0o600` on Unix/macOS.
- Ensure normal failure paths still save a local copy when upload is disabled or fails.

Do not change Telegram upload behavior here; TASK-66 covers the embedded-token decision.

### 6. Apply helpers to model/download dirs where sensible
In `download_model()` and alt-backend download code, ensure app-owned model dirs are created under the same private base directory helper if possible.

Be careful:
- Public model files themselves are not secret.
- This step should not destabilize model downloads.
- If model paths are already protected by a private app data directory after previous Windows path work, just document that and move on.

### 7. Update docs
Update `docs/PRIVACY.md` if needed:
- Say history is stored locally and protected with owner-only permissions on Unix/macOS.
- Do not claim encryption unless implemented.
- Mention that disabling or clearing history, if available, is the retention control.

Update release/readiness docs if there is a privacy gate for local files.

## Suggested commands

```bash
cargo test --manifest-path src-tauri/Cargo.toml settings
cargo test --manifest-path src-tauri/Cargo.toml diagnostic
cargo test --manifest-path src-tauri/Cargo.toml
npm run preflight
```

## Success signal
- `history.json` is written with owner-only permissions on Unix/macOS.
- Config and diagnostic report files are also private on Unix/macOS.
- Parent app data/log directories are owner-private on Unix/macOS.
- Windows behavior remains normal and compiles.
- Tests cover the permission helpers where platform support allows.
- Privacy docs match the implemented behavior without overstating it.

## Notes
- Be cautious with `std::fs::write`; it is convenient but does not let you set a creation mode before opening on Unix.
- If a file already exists with permissive permissions, this task should tighten it on the next write.
- Do not break temp-directory-based tests by assuming paths always live under the real app data directory.
