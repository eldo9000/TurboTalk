# TASK-66: Clean up small native security surfaces

## Goal
Remove two small but real native security footguns: shell-based URL opening on Windows and build-time embedding of Telegram bug-report credentials in public binaries.

## Context
The audit found:

| Audit item | Severity | Surface | Current gap |
|------------|----------|---------|-------------|
| #8 | Low | `src-tauri/src/ollama.rs` | Windows `open_url()` calls `cmd /c start` with a URL. Even with a strict host allowlist, shell metacharacter handling is an unnecessary risk. |
| #10 | Low | `src-tauri/src/diagnostic_log.rs` | Telegram bot token and chat ID are embedded with `option_env!()` if CI sets them at build time. Public binaries can reveal those strings with `strings`. |

These are not release blockers by themselves, but both are quick cleanup items and reduce the surprise surface before code and binaries become public.

## In scope
- Replace Windows shell-based URL opening in `ollama::open_url()` with a non-shell opener.
- Check for other `cmd /c start` URL-opening paths and either fix them or document why constant URLs are acceptable.
- Decide and implement the bug-report credential policy:
  - No embedded Telegram token in public builds, or
  - Relay service endpoint, or
  - Local-only bug report save with manual send instructions.
- Update docs and CI so public release builds do not accidentally embed the Telegram token.

## Out of scope
- Rebuilding the entire diagnostics UX.
- Adding a full backend service unless the user explicitly chooses that path.
- Changing Ollama host allowlist semantics beyond what is required for safer opening.
- Removing local diagnostic report generation.

## Files to inspect first
- `src-tauri/src/ollama.rs`
- `src-tauri/src/lib.rs` for `open_releases_page()`
- `src-tauri/src/diagnostic_log.rs`
- `.github/workflows/release.yml`
- `docs/PRIVACY.md`
- `docs/RELEASING.md`
- `docs/RELEASE-READINESS.md`
- `package.json` and `src-tauri/Cargo.toml` for available opener/Tauri APIs

Useful searches:

```bash
rg -n "cmd|start|open_url|open_releases_page|ShellExecute|opener|TURBOTALK_BUGREPORT|Telegram|TG_TOKEN|TG_CHAT" src-tauri .github docs package.json
```

## Steps

### 1. Replace Windows URL opening without shell interpretation
In `src-tauri/src/ollama.rs`, replace the Windows branch in `open_url()`:

```rust
std::process::Command::new("cmd")
    .args(["/c", "start", "", url.trim()])
```

with a non-shell API.

Preferred options:

1. Use Tauri's opener plugin/API if it is already available in this Tauri version and command context.
2. Use the Rust `open` crate if already present.
3. Use Windows `ShellExecuteW` directly behind `#[cfg(target_os = "windows")]`.

Do not add a new dependency if Tauri or Windows APIs already solve it cleanly.

Preserve validation:
- URL must parse.
- Scheme must be `https`.
- Host must be `ollama.com` or `*.ollama.com`.

Add tests if possible:
- Allow `https://ollama.com/...`.
- Allow `https://download.ollama.com/...`.
- Reject non-HTTPS.
- Reject `ollama.com.evil.example`.
- Reject or safely handle paths/query strings with `&`, `|`, `<`, `>`, `^`.

The key property is not "strip all metacharacters"; it is "never pass the URL through a shell."

### 2. Review other open commands
Inspect `open_releases_page()` in `src-tauri/src/lib.rs`.

It opens a constant GitHub URL, so command injection risk is much lower than `open_url(url: String)`. Still, decide whether to:

- Leave it as-is and document that it is a constant URL, or
- Move it to the same non-shell opener helper for consistency.

Recommendation:
- Create a small `open_external_url()` helper if it keeps the code simple.
- Use it for both Ollama install URLs and GitHub releases.
- Keep validation at call sites where the URL is user/config influenced.

### 3. Decide bug-report upload policy
Current behavior in `diagnostic_log.rs`:

- Always saves a local copy first.
- If `TURBOTALK_BUGREPORT_TG_TOKEN` and `TURBOTALK_BUGREPORT_TG_CHAT` are embedded at compile time, uploads to Telegram.
- If not configured, returns an error telling the user the report was saved locally.

Recommended pre-public-release policy:
- Public CI/release builds must not set these env vars.
- The app should still save local reports.
- UI/docs should tell users where to find the local report and how to send it manually.
- If automatic upload is important, build a small server-side relay later so the public app talks to your endpoint, not directly to Telegram with an embedded bot token.

Implementation options:

Option A - Disable public upload now:
- Remove or feature-gate the `option_env!()` upload path.
- Ensure `submit_bug_report()` always saves locally and returns a clear "saved locally" result/message.
- Remove release CI secrets for Telegram if present.

Option B - Keep dev-only upload:
- Gate `option_env!()` behind a Cargo feature that is never enabled in release CI.
- Name the feature clearly, for example `dev-telegram-bugreport`.
- Ensure default builds cannot embed the token accidentally.
- Document that public builds save reports locally only.

Option C - Relay:
- Out of scope unless the user explicitly asks for it.

Recommendation for this task:
- Use Option B if the developer still wants private/dev convenience.
- Use Option A if simplicity is more important than dev upload.

### 4. Update CI and docs
Inspect `.github/workflows/release.yml`:

- Confirm it does not set `TURBOTALK_BUGREPORT_TG_TOKEN` or `TURBOTALK_BUGREPORT_TG_CHAT`.
- If it does, remove those env vars from release builds.
- Add a guard/check that fails public release builds if bug-report token env vars are present, unless the explicit dev feature is enabled.

Update docs:
- `docs/PRIVACY.md`: accurately describe diagnostic report behavior and whether automatic upload exists in public builds.
- `docs/RELEASING.md`: mention that public release builds must not embed Telegram credentials.
- `docs/RELEASE-READINESS.md`: add/check a gate for no embedded bug-report secrets in release binaries.

### 5. Verify
Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml ollama
cargo test --manifest-path src-tauri/Cargo.toml diagnostic
cargo test --manifest-path src-tauri/Cargo.toml
npm run preflight
```

If a packaged binary exists, optionally verify no credentials are embedded:

```bash
strings path/to/binary | rg "TURBOTALK_BUGREPORT|api.telegram.org|bot[0-9]+:"
```

Do not commit or print real secrets.

## Success signal
- User/config-influenced URLs are never opened via `cmd.exe` or shell parsing on Windows.
- Ollama install URL validation remains narrow.
- Public release builds cannot accidentally embed Telegram bot credentials.
- Bug-report local-save behavior still works.
- Privacy and release docs match the implemented behavior.

## Notes
- The URL-opening fix is small and should be done even if the token policy takes longer.
- If direct `ShellExecuteW` is used, keep it isolated behind a helper and write the conversion from Rust string to wide string carefully.
- The final report should state which bug-report policy was chosen and why.
