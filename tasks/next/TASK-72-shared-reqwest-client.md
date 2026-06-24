# TASK-72: Reuse a single reqwest::blocking::Client across cleanup and Ollama commands

## Goal
Eliminate per-call `reqwest::blocking::Client` construction in the Chaperone cleanup path and the Ollama IPC commands by sharing a process-lifetime client.

## Context
TurboTalk makes HTTP calls to a local Ollama instance (loopback) for LLM cleanup and health checks. Four call sites each build a fresh `reqwest::blocking::Client` on every invocation:

1. `src-tauri/src/cleanup.rs:523-526` — `classify_blocking()` builds a client with `.timeout(Duration::from_secs(60))` inside the function body. Called on every Chaperone-mode dictation.
2. `src-tauri/src/ollama.rs:103` — `ping_ollama()` builds a client with 2s timeout. Called from Settings-tab health checks.
3. `src-tauri/src/ollama.rs:164` — `check_ollama_model()` builds a client with 2s timeout. Called from Settings-tab.
4. `src-tauri/src/ollama.rs:356` — `prewarm_ollama()` builds a client with 10s timeout. Called on app startup.

Each client construction initializes TLS state, a connection pool, and a DNS resolver. Since all calls go to `127.0.0.1`, the TLS overhead and pool are pure waste when rebuilt per-call. A shared client reuses the warm connection pool to loopback.

The challenge: the four call sites use different timeouts (2s, 10s, 60s). `reqwest::blocking::Client` has a client-level timeout, but also supports per-request timeouts via `RequestBuilder::timeout()`. The correct pattern is one shared client with no client-level timeout, and each call site sets `.timeout(...)` on the request builder.

The codebase already uses `parking_lot::Mutex` and `std::sync::OnceLock` elsewhere (e.g. `settings.rs:590`, `vad.rs:190`). `OnceLock` is the correct lazy-init for a process-lifetime singleton.

## In scope
- `src-tauri/src/cleanup.rs` — replace per-call `Client::builder()` with shared client + per-request timeout
- `src-tauri/src/ollama.rs` — same for all three Ollama command functions
- `SESSION-STATUS.md`

## Out of scope
- Switching from `reqwest::blocking` to async `reqwest` (the blocking client is fine for loopback calls; the cleanup path runs on a worker thread, not the async runtime)
- Changing the Ollama API surface or command signatures
- The `transcribe.rs` whisper-server HTTP client (that's a different concern with different timeout requirements — it has its own 120s inference client and 400ms poll client)
- Any frontend changes

## Steps
1. In `cleanup.rs`, create a module-level `static OLLAMA_CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();` and a helper `fn ollama_client() -> &'static reqwest::blocking::Client` that initializes it once with no client-level timeout (or a generous 120s ceiling). 
2. In `classify_blocking()`, replace the `Client::builder().timeout(60s).build()` call with `ollama_client().post(url).timeout(Duration::from_secs(60))` on the request builder.
3. In `ollama.rs`, create the same pattern: a shared `OnceLock<Client>` (or import the one from `cleanup.rs` if you prefer a single shared client — both paths hit the same Ollama instance). Add a small helper.
4. Replace the per-call client builders in `ping_ollama`, `check_ollama_model`, and `prewarm_ollama` with the shared client + per-request `.timeout()` on the request builder.
5. Ensure the shared client is built with `.connect_timeout(Duration::from_secs(2))` so that connection failures to a dead Ollama don't hang for the full request timeout. This is a client-level setting that applies to all requests — the per-request `.timeout()` still governs the total request duration.
6. Run `cargo check --manifest-path src-tauri/Cargo.toml` and `cargo clippy --manifest-path src-tauri/Cargo.toml -- -W clippy::all`.
7. Run `npm run typecheck` (no frontend changes expected, but verify nothing broke).
8. Update `SESSION-STATUS.md`.

## Success signal
- `cargo check` and `cargo clippy` pass with no new warnings.
- `grep -rn "Client::builder" src-tauri/src/cleanup.rs src-tauri/src/ollama.rs` returns zero results (no per-call client construction).
- A single `OnceLock<reqwest::blocking::Client>` is shared across all four call sites.
- Each call site sets its own per-request `.timeout()` matching its original value (2s, 2s, 10s, 60s).
- The shared client has a `.connect_timeout(2s)` so dead-Ollama connections fail fast.

## Notes
- `reqwest::blocking::Client::builder()` does not have a `connect_timeout` method directly — use `.connect_timeout()` on the builder, which is available in reqwest 0.12. Verify the API.
- The client is `Send + Sync` and safe to share across threads via `&'static` reference from `OnceLock::get`.
- Do not set a client-level `.timeout()` on the shared client — that would override per-request timeouts. Leave it unset (no client-level timeout) and rely on per-request `.timeout()` for each call.
- The `pull_ollama_model` command in `ollama.rs:201` already uses `tokio::task::spawn_blocking` with a streaming body — that's a different pattern (long-lived streaming) and should keep its own client or use the shared one with a long per-request timeout. Either is fine; the streaming body doesn't benefit as much from pool reuse since it's a single long request.
