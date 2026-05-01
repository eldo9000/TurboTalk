// Chaperone Layer — LLM postprocessor.
//
// Pipes the raw whisper transcript through a small local LLM (Ollama at
// http://localhost:11434 by default) running as a CLASSIFIER, not a rewriter.
//
// Pipeline:
//   1. Classify the utterance into a mode: prose | code | command | raw
//   2. Route to a deterministic handler for that mode
//   3. Handler produces the final text
//
// Closed action space, open input space. The LLM never freely rewrites text —
// it picks among hand-written handlers.
//
// If Ollama is unreachable or cleanup mode is `off`, fall through to raw.

use serde::{Deserialize, Serialize};

/// Voice command actions detected before classification.
#[derive(Debug, PartialEq)]
enum VoiceCommand {
    ScratchThat,
    NewParagraph,
    None,
}

/// Output modes the classifier can pick.
#[derive(Debug, PartialEq)]
enum Mode {
    Prose,
    Code,
    Command,
    Raw,
}

/// Entry point called from transcribe.rs after whisper produces raw text.
/// Returns the final string to paste (or an empty string for "scratch that").
pub fn process(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Voice command detection runs before any cleanup.
    match detect_voice_command(trimmed) {
        VoiceCommand::ScratchThat => return String::new(),
        VoiceCommand::NewParagraph => return "\n\n".to_string(),
        VoiceCommand::None => {}
    }

    let cfg = crate::settings::load();
    match cfg.cleanup.mode.as_str() {
        "off" => handle_raw(trimmed),
        "regex" => handle_prose(trimmed),
        "chaperone" => {
            match classify_blocking(trimmed, &cfg.cleanup) {
                Ok(mode) => route(trimmed, mode),
                Err(e) => {
                    tracing::warn!("[chaperone] classify failed, falling back to prose: {e}");
                    handle_prose(trimmed)
                }
            }
        }
        other => {
            tracing::warn!("[chaperone] unknown mode {other:?}, using raw");
            handle_raw(trimmed)
        }
    }
}

// ── Voice command detection ───────────────────────────────────────────────────

fn detect_voice_command(text: &str) -> VoiceCommand {
    let lower = text.to_lowercase();
    let lower = lower.trim_end_matches(['.', '!', '?', ',']);
    match lower {
        "scratch that" | "delete that" | "never mind" => VoiceCommand::ScratchThat,
        "new paragraph" | "new line" => VoiceCommand::NewParagraph,
        _ => VoiceCommand::None,
    }
}

// ── Mode routing ──────────────────────────────────────────────────────────────

fn route(text: &str, mode: Mode) -> String {
    match mode {
        Mode::Prose => handle_prose(text),
        Mode::Code => handle_code(text),
        Mode::Command => handle_command(text),
        Mode::Raw => handle_raw(text),
    }
}

// ── Deterministic handlers ────────────────────────────────────────────────────

fn handle_prose(text: &str) -> String {
    capitalize_first(text)
}

fn handle_code(text: &str) -> String {
    // Strip trailing period whisper often adds, preserve casing.
    let s = text.trim_end_matches('.');
    s.to_string()
}

fn handle_command(text: &str) -> String {
    // Shell commands: strip trailing punctuation, lowercase.
    text.trim_end_matches(['.', '!', '?'])
        .to_lowercase()
}

fn handle_raw(text: &str) -> String {
    text.to_string()
}

fn capitalize_first(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap().to_uppercase().to_string();
    first + chars.as_str()
}

// ── Ollama classifier ─────────────────────────────────────────────────────────

#[derive(Serialize)]
struct OllamaRequest<'a> {
    model: &'a str,
    prompt: String,
    stream: bool,
}

#[derive(Deserialize)]
struct OllamaResponse {
    response: String,
}

fn classify_blocking(
    text: &str,
    cfg: &crate::settings::CleanupConfig,
) -> anyhow::Result<Mode> {
    let prompt = format!(
        "Classify this voice dictation into exactly one word: prose, code, command, or raw.\n\
         Rules:\n\
         - prose: natural language sentences (emails, notes, messages)\n\
         - code: identifiers, snippets, technical syntax (camelCase, snake_case, brackets)\n\
         - command: shell commands or CLI invocations (starts with a verb like run/git/ls/cd)\n\
         - raw: anything else\n\
         Reply with only the single word, lowercase, no punctuation.\n\n\
         Text: {text}"
    );

    let body = OllamaRequest {
        model: &cfg.classifier_model,
        prompt,
        stream: false,
    };

    let url = format!("{}/api/generate", cfg.ollama_url.trim_end_matches('/'));

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    let resp: OllamaResponse = client
        .post(&url)
        .json(&body)
        .send()?
        .json()?;

    Ok(parse_mode(resp.response.trim()))
}

fn parse_mode(s: &str) -> Mode {
    match s.to_lowercase().as_str() {
        "prose" => Mode::Prose,
        "code" => Mode::Code,
        "command" => Mode::Command,
        _ => Mode::Raw,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_commands() {
        assert_eq!(detect_voice_command("scratch that"), VoiceCommand::ScratchThat);
        assert_eq!(detect_voice_command("Scratch that."), VoiceCommand::ScratchThat);
        assert_eq!(detect_voice_command("new paragraph"), VoiceCommand::NewParagraph);
        assert_eq!(detect_voice_command("hello world"), VoiceCommand::None);
    }

    #[test]
    fn prose_handler() {
        assert_eq!(handle_prose("hello world"), "Hello world");
        assert_eq!(handle_prose("already capitalized"), "Already capitalized");
    }

    #[test]
    fn code_handler() {
        assert_eq!(handle_code("myFunction."), "myFunction");
        assert_eq!(handle_code("git status"), "git status");
    }

    #[test]
    fn command_handler() {
        assert_eq!(handle_command("Run npm install."), "run npm install");
    }

    #[test]
    fn parse_mode_fuzzy() {
        assert_eq!(parse_mode("prose"), Mode::Prose);
        assert_eq!(parse_mode("CODE"), Mode::Code);
        assert_eq!(parse_mode("garbage"), Mode::Raw);
    }
}
