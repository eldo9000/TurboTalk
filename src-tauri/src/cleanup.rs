//! Text post-processor — cleanup and formatting pass over raw transcripts.
//!
//! Pipeline:
//!   1. Strip non-speech annotations (Whisper artifacts like "(sigh)")
//!   2. Detect and execute voice commands ("scratch that", "new paragraph")
//!   3. Apply mode-specific cleanup (raw, regex, or text_formatter)
//!   4. Apply anti-vocabulary replacements (user-defined word substitution)
//!
//! The `TextFormatter` mode runs the rule-based pre-formatter
//! (spoken punctuation, literal formatting, baseline cleanup).

/// Voice command actions detected before any cleanup.
#[derive(Debug, PartialEq)]
enum VoiceCommand {
    ScratchThat,
    NewParagraph,
    None,
}

/// Entry point called from transcribe.rs after whisper produces raw text.
/// Returns the final string to paste (or an empty string for "scratch that").
pub fn process(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Strip non-speech annotation tokens Whisper emits for breaths, sighs, etc.
    // --suppress-nst doesn't reliably catch all of these across whisper.cpp versions.
    let cleaned = strip_non_speech_annotations(trimmed);
    let trimmed = cleaned.trim();
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
    let is_whisper = cfg.backend == crate::settings::BackendFamily::Whisper;

    // Apply mode-specific cleanup.
    // TextFormatter rules are Whisper-only — Parakeet full-capture produces
    // well-formed punctuation and capitalization directly; running deterministic
    // rules on top strips correctly-placed periods and fights the model's output.
    let result = match cfg.cleanup.mode {
        crate::settings::CleanupMode::TextFormatter if is_whisper => {
            let opts = crate::pre_format::FormatOptions {
                punctuation: cfg.cleanup.format_punctuation,
                literal: cfg.cleanup.format_literal,
                strip_fillers: cfg.cleanup.format_strip_fillers,
                strip_artifacts: cfg.cleanup.format_strip_artifacts,
                capitalize: cfg.cleanup.format_capitalize,
            };
            crate::pre_format::format_with(trimmed, &opts)
        }
        _ => handle_raw(trimmed),
    };

    // Anti-vocabulary applies in TextFormatter mode, or unconditionally for
    // Parakeet (where every mode is effectively Off and replacements are still
    // useful for correcting persistent ASR misspellings).
    if cfg.cleanup.mode != crate::settings::CleanupMode::Off || !is_whisper {
        apply_antivocabulary(&result, &cfg.cleanup.antivocabulary)
    } else {
        result
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

// ── Deterministic helpers ─────────────────────────────────────────────────────

fn strip_non_speech_annotations(text: &str) -> String {
    // Whisper outputs these annotation tokens for non-speech sounds.
    // Listed in lowercase; matched case-insensitively against the input.
    const ANNOTATIONS: &[&str] = &[
        "(sigh)",
        "(sighs)",
        "(sighing)",
        "(exhale)",
        "(exhales)",
        "(exhaling)",
        "(inhale)",
        "(inhales)",
        "(inhaling)",
        "(breath)",
        "(breathes)",
        "(breathing)",
        "(cough)",
        "(coughs)",
        "(coughing)",
        "(laugh)",
        "(laughs)",
        "(laughing)",
        "(chuckle)",
        "(chuckles)",
        "(chuckling)",
        "[blank_audio]",
        "[noise]",
        "[music]",
        "[applause]",
        "[laughter]",
    ];

    let lower = text.to_lowercase();
    let mut to_remove: Vec<(usize, usize)> = Vec::new();

    for ann in ANNOTATIONS {
        let mut start = 0;
        while let Some(pos) = lower[start..].find(ann) {
            let abs = start + pos;
            to_remove.push((abs, abs + ann.len()));
            start = abs + ann.len();
        }
    }

    if to_remove.is_empty() {
        return text.to_string();
    }

    // Remove in reverse order so earlier indices remain valid.
    to_remove.sort_by(|a, b| b.0.cmp(&a.0));
    to_remove.dedup_by(|a, b| a.0 == b.0);

    let mut result = text.to_string();
    for (start, end) in to_remove {
        result.replace_range(start..end, "");
    }

    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Collapse sequences of 3+ identical single-alphabetic characters that
/// are not valid standalone English words. Whisper often decodes stuttering
/// as repeated single letters ("f f f f f fix") — this collapses them so
/// the intended word (usually the following token) surfaces.
///
/// Valid single-letter words "a" and "I" are preserved; all other repeated
/// single-character tokens are removed entirely.
/// Collapse sequences of 3+ consecutive identical words to a single instance.
/// Catches Parakeet/Whisper stutter loops like "in in in in in in in" → "in"
/// and "now, now, now, now, now" → "now, now". Words are compared by stripping
/// trailing punctuation so "now," and "now." match "now".
/// Single-letter words are handled separately by `collapse_repeated_single_chars`.
fn collapse_repeated_words(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut result: Vec<&str> = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        if word.len() == 1 && word.chars().all(|c| c.is_ascii_alphabetic()) {
            // Single-letter word — handled by collapse_repeated_single_chars
            result.push(word);
            i += 1;
            continue;
        }
        let stem = word.trim_end_matches(|c: char| !c.is_alphanumeric());
        let mut repeat_count = 1;
        let mut j = i + 1;
        while j < words.len() {
            let next_stem = words[j].trim_end_matches(|c: char| !c.is_alphanumeric());
            if next_stem.eq_ignore_ascii_case(stem) {
                repeat_count += 1;
                j += 1;
            } else {
                break;
            }
        }
        if repeat_count >= 3 {
            result.push(word);
            i = j;
        } else {
            result.push(word);
            i += 1;
        }
    }
    result.join(" ")
}

fn collapse_repeated_single_chars(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut result: Vec<&str> = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        // Only consider single alphabetic characters.
        if word.len() == 1 && word.chars().all(|c| c.is_ascii_alphabetic()) {
            let lower = word.to_ascii_lowercase();
            // Preserve valid English single-letter words.
            if lower == "a" || lower == "i" {
                result.push(word);
                i += 1;
                continue;
            }
            // Count consecutive identical single-char tokens (case-insensitive).
            let mut repeat_count = 1;
            let mut j = i + 1;
            while j < words.len() {
                if words[j].len() == 1
                    && words[j].chars().all(|c| c.is_ascii_alphabetic())
                    && words[j].eq_ignore_ascii_case(word)
                {
                    repeat_count += 1;
                    j += 1;
                } else {
                    break;
                }
            }
            if repeat_count >= 3 {
                // Stutter artifact — skip the entire run.
                i = j;
                continue;
            }
        }
        result.push(word);
        i += 1;
    }
    result.join(" ")
}

fn handle_raw(text: &str) -> String {
    let s = collapse_repeated_words(text);
    collapse_repeated_single_chars(&s)
}

// ── Anti-vocabulary ───────────────────────────────────────────────────────────
//
// Post-processing replacements applied after all cleanup modes. Each entry is
// either a bare word (removed entirely as a word-level token) or a "from = to"
// pair (replaced). Case-insensitive matching on word boundaries.

struct ReplRule {
    re: regex::Regex,
    replacement: String,
}

fn apply_antivocabulary(text: &str, rules: &[String]) -> String {
    if rules.is_empty() || text.is_empty() {
        return text.to_string();
    }
    let compiled = compile_repl_rules(rules);
    let mut result = text.to_string();
    for rule in &compiled {
        result = rule.re.replace_all(&result, rule.replacement.as_str()).to_string();
    }
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn compile_repl_rules(rules: &[String]) -> Vec<ReplRule> {
    let mut compiled = Vec::new();
    for rule in rules {
        let rule = rule.trim();
        if rule.is_empty() {
            continue;
        }
        // Syntax: "from = to" (replace) or bare "word" (remove).
        // Spaces around "=" are optional and stripped.
        let (from, to) = if let Some(pos) = rule.find('=') {
            (rule[..pos].trim().to_string(), rule[pos + 1..].trim().to_string())
        } else {
            (rule.to_string(), String::new())
        };
        if from.is_empty() {
            continue;
        }
        let pattern = format!(r"(?i)(?<!\w){}(?!\w)", regex::escape(&from));
        if let Ok(re) = regex::Regex::new(&pattern) {
            compiled.push(ReplRule { re, replacement: to });
        }
    }
    compiled
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_speech_annotation_stripping() {
        assert_eq!(strip_non_speech_annotations("(sigh)"), "");
        assert_eq!(strip_non_speech_annotations("(Sigh)"), "");
        assert_eq!(
            strip_non_speech_annotations("(sigh) hello world"),
            "hello world"
        );
        assert_eq!(
            strip_non_speech_annotations("hello (exhales) world"),
            "hello world"
        );
        assert_eq!(strip_non_speech_annotations("[BLANK_AUDIO]"), "");
        assert_eq!(strip_non_speech_annotations("[blank_audio]"), "");
        assert_eq!(strip_non_speech_annotations("hello world"), "hello world");
        assert_eq!(
            strip_non_speech_annotations("(sigh) I was saying (exhales) something"),
            "I was saying something"
        );
    }

    #[test]
    fn voice_commands() {
        assert_eq!(
            detect_voice_command("scratch that"),
            VoiceCommand::ScratchThat
        );
        assert_eq!(
            detect_voice_command("Scratch that."),
            VoiceCommand::ScratchThat
        );
        assert_eq!(
            detect_voice_command("new paragraph"),
            VoiceCommand::NewParagraph
        );
        assert_eq!(detect_voice_command("hello world"), VoiceCommand::None);
    }

    // ── collapse_repeated_single_chars tests ───────────────────────────────

    #[test]
    fn repeated_single_char_collapse_basic() {
        // The user's reported case: repeated "f" stutter before "fix".
        let input = "attempting to f f f f f f f f f f f f f f f f f f f f fix the download";
        let expected = "attempting to fix the download";
        assert_eq!(collapse_repeated_single_chars(input), expected);
    }

    #[test]
    fn repeated_single_char_mixed_case() {
        // Case-insensitive matching: "F" and "f" are the same character.
        let input = "trying to F f F f f fix it";
        let expected = "trying to fix it";
        assert_eq!(collapse_repeated_single_chars(input), expected);
    }

    #[test]
    fn repeated_single_char_only_two_preserved() {
        // Two consecutive single chars is not enough to trigger collapse.
        let input = "type s s for save";
        assert_eq!(collapse_repeated_single_chars(input), "type s s for save");
    }

    #[test]
    fn repeated_s_char_stutter() {
        // Common: "s s s" stutter.
        let input = "s s s s so I was thinking";
        let expected = "so I was thinking";
        assert_eq!(collapse_repeated_single_chars(input), expected);
    }

    #[test]
    fn valid_single_word_a_preserved() {
        // "a" is a valid English word and must not be collapsed.
        let input = "this is a a a test sentence";
        assert_eq!(
            collapse_repeated_single_chars(input),
            "this is a a a test sentence"
        );
    }

    #[test]
    fn valid_single_word_i_preserved() {
        // "I" is a valid English word and must not be collapsed.
        let input = "I I I think so";
        assert_eq!(collapse_repeated_single_chars(input), "I I I think so");
    }

    #[test]
    fn valid_single_word_i_lowercase_preserved() {
        // Lowercase "i" is also common in informal transcription.
        let input = "i i i don't know";
        assert_eq!(collapse_repeated_single_chars(input), "i i i don't know");
    }

    #[test]
    fn repeated_single_char_at_end() {
        // Stutter at the very end of the text.
        let input = "hello world f f f f f";
        let expected = "hello world";
        assert_eq!(collapse_repeated_single_chars(input), expected);
    }

    #[test]
    fn repeated_single_char_at_start() {
        // Stutter at the very beginning.
        let input = "f f f f hello world";
        let expected = "hello world";
        assert_eq!(collapse_repeated_single_chars(input), expected);
    }

    #[test]
    fn multiple_different_stutter_runs() {
        // Two separate stutter sequences in the same text.
        let input = "f f f f fix the s s s s system";
        let expected = "fix the system";
        assert_eq!(collapse_repeated_single_chars(input), expected);
    }

    #[test]
    fn normal_text_passes_through() {
        // Normal text with no repeated single chars should be unchanged.
        let input = "hello world, this is a normal dictation.";
        assert_eq!(collapse_repeated_single_chars(input), input);
    }

}
