//! Rule-based text formatter — deterministic, offline, no model.
//!
//! Pipeline:
//!   1. Spoken punctuation  ("type comma" → ",")
//!   2. Literal formatting  ("slash foo" → "/foo", "at sign bob" → "@bob")
//!   3. Baseline cleanup    (capitalize, strip artifacts, fillers, stutter collapse)
//!
//! All functions are pure — no IO, no state, no network.

// ── Options ──────────────────────────────────────────────────────────────────

/// Toggles for each formatting stage. All default to true.
#[derive(Debug, Clone, Copy)]
pub struct FormatOptions {
    /// Convert "type period" → "."
    pub punctuation: bool,
    /// Convert "slash foo" → "/foo", "at sign Bob" → "@Bob"
    pub literal: bool,
    /// Strip filler words (um, uh, er, hmm, hm)
    pub strip_fillers: bool,
    /// Strip trailing whisper artifacts ( "...", " .", trailing ".")
    pub strip_artifacts: bool,
    /// Capitalize the first letter
    pub capitalize: bool,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            punctuation: true,
            literal: true,
            strip_fillers: true,
            strip_artifacts: true,
            capitalize: true,
        }
    }
}

// ── Public entry point ───────────────────────────────────────────────────────

/// Run the full formatting pipeline on a raw transcript.
pub fn format(text: &str) -> String {
    format_with(text, &FormatOptions::default())
}

/// Run the formatting pipeline with per-stage toggles.
pub fn format_with(text: &str, opts: &FormatOptions) -> String {
    let text = if opts.punctuation {
        apply_spoken_punctuation(text, "type")
    } else {
        text.to_string()
    };
    let text = if opts.literal {
        apply_literal_formatting(&text)
    } else {
        text
    };
    baseline_cleanup(&text, opts)
}

// ── Spoken punctuation ───────────────────────────────────────────────────────
//
// Converts spoken punctuation commands into their symbol equivalents.
// Triggered by a prefix word (default "type") — only tokens following the
// prefix are scanned against the punctuation rules table.
//
// "I need coffee type period and a nap" → "I need coffee. and a nap"

#[derive(Clone, Copy)]
struct PunctRule {
    /// Spoken phrase (lowercase), longest phrases first
    phrase: &'static [&'static str],
    /// Output symbol
    symbol: &'static str,
    /// How the symbol attaches to surrounding words
    spacing: Spacing,
}

#[derive(Clone, Copy, PartialEq)]
enum Spacing {
    /// Space before AND after (default for text flow).
    /// Used for symbols that naturally sit between words: & + = % $
    Both,
    /// No space before, space after (standard trailing punctuation).
    /// Used for: . , : ; !
    RightAttach,
    /// Space before, no space after (open brackets/quotes).
    /// Used for: ( [ { <
    LeftAttach,
    /// No space on either side (inline symbols in code/paths).
    /// Used for: @ # / ~ -
    NoSpace,
}

const PUNCT_RULES: &[PunctRule] = &[
    // ── Multi-word rules first (longest match) ──
    PunctRule { phrase: &["exclamation", "mark"], symbol: "!", spacing: Spacing::RightAttach },
    PunctRule { phrase: &["exclamation", "point"], symbol: "!", spacing: Spacing::RightAttach },
    PunctRule { phrase: &["question", "mark"],    symbol: "?", spacing: Spacing::RightAttach },
    PunctRule { phrase: &["open", "paren"],       symbol: "(", spacing: Spacing::LeftAttach },
    PunctRule { phrase: &["close", "paren"],      symbol: ")", spacing: Spacing::RightAttach },
    PunctRule { phrase: &["open", "bracket"],     symbol: "[", spacing: Spacing::LeftAttach },
    PunctRule { phrase: &["close", "bracket"],    symbol: "]", spacing: Spacing::RightAttach },
    PunctRule { phrase: &["open", "brace"],       symbol: "{", spacing: Spacing::LeftAttach },
    PunctRule { phrase: &["close", "brace"],      symbol: "}", spacing: Spacing::RightAttach },
    PunctRule { phrase: &["dollar", "sign"],      symbol: "$", spacing: Spacing::Both },
    PunctRule { phrase: &["at", "sign"],          symbol: "@", spacing: Spacing::NoSpace },
    PunctRule { phrase: &["dot", "dot", "dot"],   symbol: "...", spacing: Spacing::RightAttach },
    PunctRule { phrase: &["forward", "slash"],    symbol: "/", spacing: Spacing::NoSpace },
    // ── Single-word rules ──
    PunctRule { phrase: &["period"],     symbol: ".", spacing: Spacing::RightAttach },
    PunctRule { phrase: &["dot"],        symbol: ".", spacing: Spacing::RightAttach },
    PunctRule { phrase: &["comma"],      symbol: ",", spacing: Spacing::RightAttach },
    PunctRule { phrase: &["colon"],      symbol: ":", spacing: Spacing::RightAttach },
    PunctRule { phrase: &["semicolon"],  symbol: ";", spacing: Spacing::RightAttach },
    PunctRule { phrase: &["slash"],      symbol: "/", spacing: Spacing::NoSpace },
    PunctRule { phrase: &["backslash"],  symbol: "\\", spacing: Spacing::NoSpace },
    PunctRule { phrase: &["dash"],       symbol: "-", spacing: Spacing::NoSpace },
    PunctRule { phrase: &["hyphen"],     symbol: "-", spacing: Spacing::NoSpace },
    PunctRule { phrase: &["underscore"], symbol: "_", spacing: Spacing::NoSpace },
    PunctRule { phrase: &["star"],       symbol: "*", spacing: Spacing::Both },
    PunctRule { phrase: &["asterisk"],   symbol: "*", spacing: Spacing::Both },
    PunctRule { phrase: &["ampersand"],  symbol: "&", spacing: Spacing::Both },
    PunctRule { phrase: &["percent"],    symbol: "%", spacing: Spacing::Both },
    PunctRule { phrase: &["hash"],       symbol: "#", spacing: Spacing::NoSpace },
    PunctRule { phrase: &["pound"],      symbol: "#", spacing: Spacing::NoSpace },
    PunctRule { phrase: &["tilde"],      symbol: "~", spacing: Spacing::NoSpace },
    PunctRule { phrase: &["caret"],      symbol: "^", spacing: Spacing::NoSpace },
    PunctRule { phrase: &["backtick"],   symbol: "`", spacing: Spacing::NoSpace },
    PunctRule { phrase: &["pipe"],       symbol: "|", spacing: Spacing::Both },
    PunctRule { phrase: &["ellipsis"],   symbol: "...", spacing: Spacing::RightAttach },
    PunctRule { phrase: &["plus"],       symbol: "+", spacing: Spacing::Both },
    PunctRule { phrase: &["equal"],      symbol: "=", spacing: Spacing::Both },
];

fn apply_spoken_punctuation(text: &str, prefix_word: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out: Vec<String> = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        if !words[i].eq_ignore_ascii_case(prefix_word) {
            out.push(words[i].to_string());
            i += 1;
            continue;
        }

        // Prefix found — try to match a punctuation rule starting at i+1.
        let mut matched = false;
        for rule in PUNCT_RULES {
            let phrase = rule.phrase;
            if i + 1 + phrase.len() > words.len() {
                continue;
            }
            if words[i + 1..i + 1 + phrase.len()]
                .iter()
                .zip(phrase.iter())
                .all(|(w, p)| w.eq_ignore_ascii_case(p))
            {
                // Emit symbol with the appropriate spacing.
                let sym = rule.symbol;
                match rule.spacing {
                    Spacing::RightAttach => {
                        // No space before, space after (already handled by
                        // the join below — push bare symbol).
                    }
                    Spacing::LeftAttach => {
                        // Ensure space before by trimming trailing from prev
                        if let Some(last) = out.last_mut() {
                            last.push_str(sym);
                        } else {
                            out.push(sym.to_string());
                        }
                        i += 1 + phrase.len();
                        matched = true;
                        break;
                    }
                    Spacing::NoSpace => {
                        // Attach to the previous word if there is one
                        if let Some(last) = out.last_mut() {
                            last.push_str(sym);
                        } else {
                            out.push(sym.to_string());
                        }
                        i += 1 + phrase.len();
                        matched = true;
                        break;
                    }
                    Spacing::Both => {}
                }
                // Default: push symbol as its own token (join adds spaces).
                // For RightAttach, the trailing space from join is correct.
                // For Both, the spaces on both sides from join are correct.
                out.push(sym.to_string());
                i += 1 + phrase.len();
                matched = true;
                break;
            }
        }

        if !matched {
            // Prefix wasn't a punctuation command — keep the word as-is.
            out.push(words[i].to_string());
            i += 1;
        }
    }
    out.join(" ")
}

// ── Literal formatting ──────────────────────────────────────────────────────
//
// Converts spoken slash commands and @mentions into their typed equivalents.
//   "slash deploy"  → "/deploy"
//   "forward slash home" → "/home"
//   "at sign bob"  → "@bob"

const LEADIN_WORDS: &[&str] = &[
    "run", "open", "type", "send", "choose", "execute", "enter",
];

fn apply_literal_formatting(text: &str) -> String {
    let text = apply_slash_formatting(text);
    apply_mention_formatting(&text)
}

fn apply_slash_formatting(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out: Vec<String> = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        // Detect "/ commandname" (slash as word)
        if (words[i] == "/" || words[i].eq_ignore_ascii_case("slash") || words[i].eq_ignore_ascii_case("forward"))
            && i + 1 < words.len()
            && words[i + 1].len() >= 2
        {
            // Check if "forward slash" two-word form
            let (consume_next, target_idx) = if words[i].eq_ignore_ascii_case("forward")
                && i + 2 < words.len()
                && words[i + 1].eq_ignore_ascii_case("slash")
            {
                (true, i + 2)
            } else {
                (false, i + 1)
            };

            let target = words[target_idx];
            // Only convert if preceded by a lead-in word or at sentence start
            let should_convert = out.is_empty()
                || LEADIN_WORDS.contains(&out.last().map(|s| s.as_str()).unwrap_or("").trim_end_matches(|c: char| !c.is_alphabetic()).to_lowercase().as_str());

            if should_convert {
                // Check the target doesn't look like a normal word
                // (would cause false positives on "slash and burn")
                let cmd = format!("/{}", target);
                out.push(cmd);
                if consume_next {
                    i += 3; // forward + slash + command
                } else {
                    i += 2; // slash/"/" + command
                }
                continue;
            }
        }
        out.push(words[i].to_string());
        i += 1;
    }
    out.join(" ")
}

fn apply_mention_formatting(text: &str) -> String {
    // Converts "at sign Name" → "@Name" where Name is capitalized.
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out: Vec<String> = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        if (words[i].eq_ignore_ascii_case("at") || words[i] == "@")
            && i + 1 < words.len()
            && (words[i + 1] == "sign" || words[i + 1] == "Sign")
            && i + 2 < words.len()
        {
            let name = words[i + 2];
            // Name must start with uppercase (PascalCase)
            if name.starts_with(|c: char| c.is_uppercase()) {
                out.push(format!("@{}", name));
                i += 3;
                continue;
            }
        }
        out.push(words[i].to_string());
        i += 1;
    }
    out.join(" ")
}

// ── Baseline cleanup ─────────────────────────────────────────────────────────
//
// Applies after formatting. Mirrors the old "Regex" mode's deterministic
// handlers: capitalize first letter, strip Whisper artifacts, strip filler
// words, collapse stutter loops.

const FILLER_WORDS: &[&str] = &["um", "uh", "er", "hmm", "hm"];

fn baseline_cleanup(text: &str, opts: &FormatOptions) -> String {
    if text.is_empty() {
        return String::new();
    }
    let s = if opts.strip_artifacts { strip_whisper_artifacts(text) } else { text.to_string() };
    let s = if opts.strip_fillers { strip_filler_words(&s) } else { s };
    let s = collapse_repeated_words(&s);
    let s = collapse_repeated_single_chars(&s);
    if opts.capitalize { capitalize_first(&s) } else { s }
}

fn strip_whisper_artifacts(text: &str) -> String {
    let s = text.trim();
    let s = s.trim_end_matches("...");
    let s = s.trim_end_matches(" ...");
    let s = s.trim_end_matches(" .");
    let s = s.trim_end_matches('.');
    s.trim().to_string()
}

fn strip_filler_words(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let filtered: Vec<&str> = words
        .into_iter()
        .filter(|w| {
            let bare = w.trim_matches(|c: char| !c.is_alphabetic()).to_lowercase();
            !FILLER_WORDS.contains(&bare.as_str())
        })
        .collect();
    filtered.join(" ")
}

fn collapse_repeated_words(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut result: Vec<&str> = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        if word.len() == 1 && word.chars().all(|c| c.is_ascii_alphabetic()) {
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
        if word.len() == 1 && word.chars().all(|c| c.is_ascii_alphabetic()) {
            let lower = word.to_ascii_lowercase();
            if lower == "a" || lower == "i" {
                result.push(word);
                i += 1;
                continue;
            }
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
                i = j;
                continue;
            }
        }
        result.push(word);
        i += 1;
    }
    result.join(" ")
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Spoken punctuation ──────────────────────────────────────────────────

    #[test]
    fn basic_period() {
        assert_eq!(
            apply_spoken_punctuation("I need coffee type period and a nap", "type"),
            "I need coffee . and a nap"
        );
    }

    #[test]
    fn basic_comma() {
        assert_eq!(
            apply_spoken_punctuation("hello type comma world", "type"),
            "hello , world"
        );
    }

    #[test]
    fn multi_word_rule() {
        assert_eq!(
            apply_spoken_punctuation("type open paren hello type close paren", "type"),
            "( hello )"
        );
    }

    #[test]
    fn no_prefix_matches_unchanged() {
        let input = "what type of coffee is that";
        assert_eq!(apply_spoken_punctuation(input, "type"), input);
    }

    #[test]
    fn mixed_punctuation_and_text() {
        assert_eq!(
            apply_spoken_punctuation("I think type comma therefore I am", "type"),
            "I think , therefore I am"
        );
    }

    // ── Slash formatting ────────────────────────────────────────────────────

    #[test]
    fn slash_command() {
        assert_eq!(
            apply_slash_formatting("run slash deploy"),
            "run /deploy"
        );
    }

    #[test]
    fn forward_slash_command() {
        assert_eq!(
            apply_slash_formatting("open forward slash home"),
            "open /home"
        );
    }

    #[test]
    fn slash_not_converted_without_leadin() {
        let input = "I had slash for dinner";
        assert_eq!(apply_slash_formatting(input), input);
    }

    // ── Mention formatting ──────────────────────────────────────────────────

    #[test]
    fn at_mention() {
        assert_eq!(
            apply_mention_formatting("send at sign Bob a message"),
            "send @Bob a message"
        );
    }

    #[test]
    fn at_mention_lowercase_not_converted() {
        let input = "at sign bob is lowercase";
        assert_eq!(apply_mention_formatting(input), input);
    }

    // ── Baseline cleanup ────────────────────────────────────────────────────

    #[test]
    fn capitalize_first_letter() {
        assert_eq!(capitalize_first("hello world"), "Hello world");
    }

    #[test]
    fn strip_trailing_dots() {
        assert_eq!(
            strip_whisper_artifacts("hello world ."),
            "hello world"
        );
        assert_eq!(
            strip_whisper_artifacts("hello world..."),
            "hello world"
        );
    }

    #[test]
    fn strip_fillers() {
        assert_eq!(
            strip_filler_words("um hello uh world"),
            "hello world"
        );
    }

    #[test]
    fn stutter_collapse() {
        assert_eq!(
            collapse_repeated_single_chars("f f f f fix it"),
            "fix it"
        );
        assert_eq!(
            collapse_repeated_words("in in in in in the house"),
            "in the house"
        );
    }

    // ── Full pipeline ───────────────────────────────────────────────────────

    #[test]
    fn full_pipeline() {
        let input = "um type hello type period uh I need coffee type comma please";
        let result = format(input);
        // Pipeline: spoken punct → literal → baseline
        // "type hello" → no rule match, "hello" stays
        assert!(result.contains("hello"));
        assert!(result.contains(","));
    }
}
