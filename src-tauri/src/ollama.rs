// Ollama helper commands — backend plumbing for the guided Ollama setup flow.
//
// Three commands:
//   ping_ollama        — GET /api/version, returns { reachable, version }
//   check_ollama_model — GET /api/tags, returns bool (model present?)
//   open_url           — open a validated https://*.ollama.com URL in the browser
//
// Timeout pattern mirrors cleanup.rs::classify_blocking: 2-second connect +
// read cap via reqwest::blocking::Client. open_url does not time out — it just
// spawns the OS browser opener and returns.

use serde::{Deserialize, Serialize};

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

// ── Timeout constant (mirrors cleanup.rs::OLLAMA_TIMEOUT) ─────────────────────

const OLLAMA_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

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
    let parsed = url::Url::parse(url.trim())
        .map_err(|e| format!("invalid URL: {e}"))?;

    if parsed.scheme() != "https" {
        return Err("URL must use https".into());
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| "URL has no host".to_string())?;

    // Allowlist: ollama.com and any subdomain.
    let allowed = host == "ollama.com"
        || host.ends_with(".ollama.com");

    if !allowed {
        return Err(format!(
            "URL host {host:?} is not on the allowlist (only ollama.com and *.ollama.com)"
        ));
    }

    // ── Open ──────────────────────────────────────────────────────────────────
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url.trim())
            .spawn()
            .map_err(|e| format!("failed to open URL: {e}"))?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", "", url.trim()])
            .spawn()
            .map_err(|e| format!("failed to open URL: {e}"))?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url.trim())
            .spawn()
            .map_err(|e| format!("failed to open URL: {e}"))?;
    }

    Ok(())
}
