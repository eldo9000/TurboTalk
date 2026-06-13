// Ollama helper commands — backend plumbing for the guided Ollama setup flow.
//
// Four commands:
//   ping_ollama        — GET /api/version, returns { reachable, version }
//   check_ollama_model — GET /api/tags, returns bool (model present?)
//   open_url           — open a validated https://*.ollama.com URL in the browser
//   pull_ollama_model  — POST /api/pull, streams NDJSON progress, emits
//                        `ollama-pull-progress` events to the frontend
//
// Timeout pattern mirrors cleanup.rs::classify_blocking: 2-second connect +
// read cap via reqwest::blocking::Client. open_url does not time out — it just
// spawns the OS browser opener and returns. pull_ollama_model uses a connect
// timeout but NO read timeout — pulls are multi-GB and take several minutes.

use serde::{Deserialize, Serialize};
use std::io::BufRead;

// ── Response structs ──────────────────────────────────────────────────────────

/// Returned by `ping_ollama`. `reachable: false` covers all expected failure
/// modes (URL invalid, connection refused, timeout, non-2xx) so the frontend
/// can render a "not detected" state without bouncing through typedError.
#[derive(Debug, Serialize, specta::Type)]
pub struct Reachable {
    pub reachable: bool,
    pub version: Option<String>,
}

// Partial-deserialize structs for Ollama API responses — we only need the
// fields the commands care about.

#[derive(Deserialize)]
struct OllamaVersionResponse {
    version: String,
}

#[derive(Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModel>,
}

#[derive(Deserialize)]
struct OllamaModel {
    name: String,
}

/// Progress payload emitted on the `ollama-pull-progress` event during a model
/// pull. `pct` is monotonically non-decreasing (never resets to 0 on
/// transitional status lines). `status` mirrors the Ollama API status string.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct OllamaPullProgress {
    pub model: String,
    pub pct: u8,
    pub status: String,
}

/// Partial deserialize of a single NDJSON line from `POST /api/pull`.
/// Fields are optional — not all lines carry all fields.
#[derive(Deserialize)]
struct PullLine {
    status: Option<String>,
    total: Option<u64>,
    completed: Option<u64>,
}

// ── Timeout constant (mirrors cleanup.rs::OLLAMA_TIMEOUT) ─────────────────────

const OLLAMA_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

// ── Connect-only timeout used for pull (no read timeout — pulls are multi-GB) ─

const PULL_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

// ── Commands ──────────────────────────────────────────────────────────────────

/// Ping the configured Ollama instance by hitting GET {ollama_url}/api/version.
/// Returns `Ok(Reachable { reachable: false, version: None })` for all expected
/// failure modes — only panics or Tauri framework issues produce `Err`.
#[tauri::command]
#[specta::specta]
pub fn ping_ollama() -> Result<Reachable, String> {
    let cfg = crate::settings::load();
    let base = match crate::cleanup::validate_ollama_url(&cfg.cleanup.ollama_url) {
        Ok(u) => u,
        Err(_) => {
            return Ok(Reachable {
                reachable: false,
                version: None,
            })
        }
    };

    let endpoint = match base.join("api/version") {
        Ok(u) => u,
        Err(_) => {
            return Ok(Reachable {
                reachable: false,
                version: None,
            })
        }
    };

    let client = match reqwest::blocking::Client::builder()
        .timeout(OLLAMA_TIMEOUT)
        .connect_timeout(OLLAMA_TIMEOUT)
        .build()
    {
        Ok(c) => c,
        Err(_) => {
            return Ok(Reachable {
                reachable: false,
                version: None,
            })
        }
    };

    let resp = match client.get(endpoint).send() {
        Ok(r) => r,
        Err(_) => {
            return Ok(Reachable {
                reachable: false,
                version: None,
            })
        }
    };

    if !resp.status().is_success() {
        return Ok(Reachable {
            reachable: false,
            version: None,
        });
    }

    match resp.json::<OllamaVersionResponse>() {
        Ok(body) => Ok(Reachable {
            reachable: true,
            version: Some(body.version),
        }),
        // JSON shape changed or parse error — still reachable but no version.
        Err(_) => Ok(Reachable {
            reachable: true,
            version: None,
        }),
    }
}

/// Check whether `model_name` is present in the configured Ollama instance's
/// pulled-model list. Returns `Ok(false)` for all network / URL failures.
/// Only returns `Err` for panics or framework-level issues.
#[tauri::command]
#[specta::specta]
pub fn check_ollama_model(model_name: String) -> Result<bool, String> {
    let cfg = crate::settings::load();
    let base = match crate::cleanup::validate_ollama_url(&cfg.cleanup.ollama_url) {
        Ok(u) => u,
        Err(_) => return Ok(false),
    };

    let endpoint = match base.join("api/tags") {
        Ok(u) => u,
        Err(_) => return Ok(false),
    };

    let client = match reqwest::blocking::Client::builder()
        .timeout(OLLAMA_TIMEOUT)
        .connect_timeout(OLLAMA_TIMEOUT)
        .build()
    {
        Ok(c) => c,
        Err(_) => return Ok(false),
    };

    let resp = match client.get(endpoint).send() {
        Ok(r) => r,
        Err(_) => return Ok(false),
    };

    if !resp.status().is_success() {
        return Ok(false);
    }

    match resp.json::<OllamaTagsResponse>() {
        Ok(body) => Ok(body.models.iter().any(|m| m.name == model_name)),
        Err(e) => {
            // JSON shape changed — this is unexpected enough to surface.
            Err(format!("failed to parse /api/tags response: {e}"))
        }
    }
}

/// Download a model into the user's local Ollama instance by streaming
/// `POST {ollama_url}/api/pull`. Emits incremental `ollama-pull-progress`
/// events to the frontend as each NDJSON line arrives. Returns `Ok(())` when
/// Ollama reports `"success"` and `Err(message)` on any unrecoverable failure.
///
/// **No read timeout** — model pulls are multi-GB and can take several minutes
/// on a slow connection. The connect timeout is 5 seconds (loopback should
/// connect near-instantly). Cancellation is out of scope for this task.
#[tauri::command]
#[specta::specta]
pub async fn pull_ollama_model(app: tauri::AppHandle, model_name: String) -> Result<(), String> {
    // reqwest::blocking creates its own tokio runtime internally and must not
    // run on a tokio async thread — doing so panics. Offload to spawn_blocking
    // so the streaming download runs on a dedicated OS thread outside the
    // async executor, identical to how download_model handles this.
    tokio::task::spawn_blocking(move || pull_ollama_model_blocking(app, model_name))
        .await
        .map_err(|e| format!("pull task panicked: {e}"))?
}

fn pull_ollama_model_blocking(app: tauri::AppHandle, model_name: String) -> Result<(), String> {
    use tauri::Emitter;

    tracing::info!("[ollama-pull] starting pull for model={model_name}");

    let cfg = crate::settings::load();
    let base = crate::cleanup::validate_ollama_url(&cfg.cleanup.ollama_url)
        .map_err(|e| format!("invalid Ollama URL: {e}"))?;

    let endpoint = base
        .join("api/pull")
        .map_err(|e| format!("could not build pull endpoint: {e}"))?;

    tracing::info!("[ollama-pull] POST {endpoint}");

    // Connect timeout only — read must be unbounded for multi-GB transfers.
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(PULL_CONNECT_TIMEOUT)
        // Explicitly no .timeout() — default is no timeout.
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))?;

    let body = serde_json::json!({
        "model": &model_name,
        "stream": true,
    });

    let resp = client.post(endpoint).json(&body).send().map_err(|e| {
        tracing::error!("[ollama-pull] request failed: {e}");
        format!("pull request failed: {e}")
    })?;

    let http_status = resp.status();
    tracing::info!("[ollama-pull] HTTP {http_status}");

    if !http_status.is_success() {
        let body_text = resp.text().unwrap_or_default();
        tracing::error!("[ollama-pull] server error: {body_text}");
        return Err(format!("Ollama returned HTTP {http_status}: {body_text}"));
    }

    // Wrap the response body in a BufReader so we can read line-by-line.
    let reader = std::io::BufReader::new(resp);

    let mut last_pct: u8 = 0;
    let mut last_status = String::new();
    let mut last_emit = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs(1))
        .unwrap_or_else(std::time::Instant::now);

    for line_result in reader.lines() {
        let line = line_result.map_err(|e| format!("stream read error: {e}"))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parsed: PullLine = match serde_json::from_str(line) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("[ollama-pull] failed to parse NDJSON line: {e} — line: {line}");
                continue;
            }
        };

        let status_str = parsed.status.unwrap_or_default();

        // Compute percentage from layer fields when present; keep last-known otherwise.
        let new_pct = if let (Some(total), Some(completed)) = (parsed.total, parsed.completed) {
            if total > 0 {
                ((completed.saturating_mul(100)) / total).min(100) as u8
            } else {
                last_pct
            }
        } else {
            last_pct
        };

        // Clamp to monotonically non-decreasing.
        let pct = new_pct.max(last_pct);

        // Check if we should emit: always emit on status change, otherwise
        // throttle to at most 10 Hz (100ms between events).
        let status_changed = status_str != last_status;
        let elapsed = last_emit.elapsed();
        let should_emit = status_changed || elapsed >= std::time::Duration::from_millis(100);

        if should_emit {
            let payload = OllamaPullProgress {
                model: model_name.clone(),
                pct,
                status: status_str.clone(),
            };
            if let Err(e) = app.emit("ollama-pull-progress", &payload) {
                tracing::warn!("[ollama-pull] failed to emit progress event: {e}");
            }
            last_emit = std::time::Instant::now();
            last_status = status_str.clone();
        }

        last_pct = pct;

        if status_str == "success" {
            tracing::info!("[ollama-pull] complete — model={model_name}");
            let payload = OllamaPullProgress {
                model: model_name.clone(),
                pct: 100,
                status: "success".to_string(),
            };
            let _ = app.emit("ollama-pull-progress", &payload);
            return Ok(());
        }
    }

    let msg = "pull stream ended without a success confirmation";
    tracing::error!("[ollama-pull] {msg} model={model_name}");
    Err(msg.into())
}

/// Fire-and-forget: loads the configured classifier model into Ollama's memory
/// so the first real dictation doesn't cold-start. Returns immediately — the
/// generate runs on a background thread and its result is discarded.
///
/// Only does anything when cleanup mode is Chaperone; safe to call at any time.
#[tauri::command]
#[specta::specta]
pub async fn prewarm_ollama() {
    let cfg = crate::settings::load();
    if cfg.cleanup.mode != crate::settings::CleanupMode::Chaperone {
        return;
    }
    let model = cfg.cleanup.classifier_model.clone();
    let url = cfg.cleanup.ollama_url.clone();

    // Spawn on the blocking thread pool and immediately drop the handle —
    // caller returns before the generate completes.
    std::mem::drop(tokio::task::spawn_blocking(move || {
        let base = match crate::cleanup::validate_ollama_url(&url) {
            Ok(u) => u,
            Err(_) => return,
        };
        let endpoint = match base.join("api/generate") {
            Ok(u) => u,
            Err(_) => return,
        };
        let client = match reqwest::blocking::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(3))
            .timeout(std::time::Duration::from_secs(60))
            .build()
        {
            Ok(c) => c,
            Err(_) => return,
        };
        let body = serde_json::json!({
            "model": model,
            "prompt": "hi",
            "stream": false,
            "options": { "num_predict": 1 },
        });
        match client.post(endpoint).json(&body).send() {
            Ok(_) => tracing::info!("[ollama-prewarm] model loaded"),
            Err(e) => tracing::debug!("[ollama-prewarm] skipped: {e}"),
        }
    }));
}

/// Scan `~/.ollama/models/blobs/` for any `*-partial*` files, which are left
/// behind when an `ollama pull` is interrupted mid-download. Returns `true` if
/// any partial blobs are found — the model manifest may exist but the model
/// is unusable until the blobs are complete.
#[tauri::command]
#[specta::specta]
pub fn check_ollama_partial_blobs() -> bool {
    let blobs_dir = match dirs::home_dir() {
        Some(h) => h.join(".ollama/models/blobs"),
        None => return false,
    };
    match std::fs::read_dir(&blobs_dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains("-partial")),
        Err(_) => false,
    }
}

/// Open a validated `https://*.ollama.com` URL in the user's default browser.
///
/// Validation:
///   - Must parse as a valid URL.
///   - Must use `https`.
///   - Host must be `ollama.com` or a subdomain (`*.ollama.com`).
///
/// The allowlist is intentionally narrow — the only call site this task enables
/// is the "Install Ollama" button. Any other URL fails loudly.
#[tauri::command]
#[specta::specta]
pub fn open_url(url: String) -> Result<(), String> {
    // ── Validate ──────────────────────────────────────────────────────────────
    let parsed = url::Url::parse(url.trim()).map_err(|e| format!("invalid URL: {e}"))?;

    if parsed.scheme() != "https" {
        return Err("URL must use https".into());
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| "URL has no host".to_string())?;

    // Allowlist: ollama.com and any subdomain.
    let allowed = host == "ollama.com" || host.ends_with(".ollama.com");

    if !allowed {
        return Err(format!(
            "URL host {host:?} is not on the allowlist (only ollama.com and *.ollama.com)"
        ));
    }

    // ── Open ──────────────────────────────────────────────────────────────────
    // Use the `open` crate for cross-platform non-shell URL opening.
    // On macOS this calls `open`, on Windows `ShellExecuteW`, on Linux `xdg-open`
    // — all without passing through cmd.exe or a shell interpreter.
    open::that_in_background(url.trim());
    Ok(())
}
