# Arc Log — TASK-72: Shared reqwest::blocking::Client

## Gate
Replace per-call `reqwest::blocking::Client::builder()` construction at 4 call
sites (cleanup.rs:523, ollama.rs:103, ollama.rs:164, ollama.rs:356) with a single
`OnceLock<reqwest::blocking::Client>` shared across all Ollama interactions.

## Premise declarations

### Premise 1 (initial)
- **SYMPTOM:** Four call sites each build a fresh `reqwest::blocking::Client` on
  every invocation, wasting TLS init and losing the connection pool to loopback.
- **PREMISE:** A single `OnceLock<reqwest::blocking::Client>` with no client-level
  timeout + per-request `.timeout()` will reduce overhead and reuse the warm
  connection pool across all four call sites without changing behavior.
- **DERIVATION:** `reqwest::blocking::Client` is `Send + Sync`, its connection pool
  is the primary performance benefit, and per-request `.timeout()` on the
  `RequestBuilder` correctly overrides the absence of a client-level timeout.
- **FALSIFICATION:** If any call site's per-request timeout doesn't match its
  original timeout value (2s, 2s, 60s, 60s), the premise that behavior is
  unchanged is false.
- **FALSIF-RESULT:** not yet run
- **DISPOSITION:** <pending>
