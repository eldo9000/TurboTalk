# SPEC: Rule-based pre-formatter before AI cleanup

## Overview

Add a deterministic, local-only text formatting layer that runs between
transcription and the Chaperone LLM cleanup. No network, no model — pure
rule-based text transformation for common dictation patterns: spoken punctuation,
slash commands, @mentions, and literal formatting.

The goal is to handle the ~90% of formatting cases instantly and offline,
so the LLM only needs to handle ambiguity and context the rules can't resolve.

## Current TurboTalk pipeline

```
Whisper transcript (raw text)
  → Chaperone LLM (classifier → handler → formatted text)
  → Paste
```

All formatting is done by the LLM. There's no local pre-processing step.

## Target TurboTalk pipeline

```
Whisper transcript (raw text)
  → Rule-based pre-formatter (this spec)
    → spoken punctuation ("comma" → ",")
    → literal formatting ("slash foo" → "/foo")
    → mention formatting ("at sign bob" → "@bob")
    → terminal autocomplete spacing cleanup
  → Chaperone LLM (classifier → handler → formatted text)
  → Paste
```

The pre-formatter is optional (gated by settings toggles). When disabled, the
pipeline runs as it does today.

## Formatting stages (applied in sequence)

### Stage 1: Spoken punctuation formatting

Convert spoken punctuation phrases into their symbol equivalents. Gated by
`autoConvertPunctuationEnabled` setting.

Trigger prefix: a configurable "punctuation command word" (default: "type").
Only text following this prefix is scanned for punctuation rules.

**Example flow:**
- Input: "I need coffee type period and a nap"
- Tokenize: ["I", " ", "need", " ", "coffee", " ", "type", " ", "period", " ", "and", " ", "a", " ", "nap"]
- Find prefix "type" → scan remaining tokens for punctuation rules
- Match "period" → emit "."
- Output: "I need coffee. and a nap"

**Context guards** (rules only fire in certain contexts):
- `requiresDotContext`: only convert "dot" if surrounded by domain-like tokens
  (e.g., "api dot google dot com") or numeric operands
- `requiresSymbolContext`: only convert "plus" / "equal" / "minus" if adjacent
  to short alphanumeric tokens (coding context)
- `requiresSlashPathContext`: only convert "slash" if adjacent to path-like tokens
  ("src", "home", "api", "usr")
- `requiresAtSignContext`: only convert "at sign" in coding/terminal/chat apps
  (VS Code, Terminal, Slack, Discord, etc.)

**Built-in symbol map:**
```
"period" → "."
"comma" → ","
"question mark" → "?"
"exclamation mark" → "!"
"colon" → ":"
"semicolon" → ";"
"slash" → "/"
"backslash" → "\"
"dash" → "-"
"hyphen" → "-"
"underscore" → "_"
"star" / "asterisk" → "*"
"dot" → "."
"at sign" → "@"
"ampersand" → "&"
"percent" → "%"
"dollar sign" → "$"
"hash" / "pound" → "#"
"tilde" → "~"
"caret" → "^"
"backtick" → "`"
"pipe" → "|"
"open paren" → "("
"close paren" → ")"
"open bracket" → "["
"close bracket" → "]"
"open brace" → "{"
"close brace" → "}"
"ellipsis" → "..."
```

Each rule can specify spacing behavior:
- `rightAttached` — no space before, space after (most punctuation: ".", ",", ":", ";")
- `leftAttached` — space before, no space after (open brackets/parens)
- `noSpaceAround` — no space on either side ("@", "#", "$", "/" in paths)
- `spaceAround` — space on both sides ("&", "+", "=")
- `toggleDoubleQuote` / `toggleSingleQuote` — alternating open/close per session

**Comma noise cleanup**: after rendering, remove spurious commas adjacent to other
punctuation (e.g., `,+` → `+`).

**Multi-match ordering**: rules are grouped by first word and sorted longest-first
so "exclamation mark" is matched before "exclamation" would match the wrong symbol.
Use a `HashMap<first_word, Vec<Rule>>` structure pre-grouped at build time.

### Stage 2: Spoken literal formatting

Gated by `literalDictationFormattingEnabled` setting. Two sub-formatters:

**Slash command formatting** — two-phase regex matching:
1. Matching "/ commandname" → collapses to "/commandname" (spoken literal slash)
2. Matching "slash commandname" / "forward slash commandname" → collapses to
   "/commandname" (spoken word "slash")

Blacklist: common words that start with "slash/" meanings but aren't commands
("slashdot", "slash and burn", etc.). Blacklist is a simple HashSet<String>.

Context guard: only convert if preceded by a lead-in word indicating a command
context: "run", "open", "type", "send", "choose", "execute", "enter", or
end-of-sentence boundary. Prevents false positives on "I had slash for dinner."

**Mention formatting** — two-phase regex matching:
1. Strict: "at sign Name" / "tag Name" / "mention Name" → "@Name". Only when Name
   is PascalCase (capitalized).
2. Relaxed (Slack/Discord/Teams only): "at PascalName" → "@PascalName". Only when
   preceded by lead-in words "send", "dm", "ping", "cc", "message", "tell", "ask".

Blacklist: avoid converting incidental "at" uses ("at the office", "at noon", "at least").

### Stage 3: Terminal autocomplete spacing (bonus)

When a slash command is the last token and the text ends with trailing whitespace
(e.g., "/fix "), strip the trailing space so the terminal's tab-completion kicks in.

Same concept for "@mention " in Slack/Discord — strip trailing space so the
mention picker opens.

## App-context awareness

Both formatters accept `(appName: &str, bundleID: &str, windowTitle: &str)` to
vary behavior per application.

TurboTalk already tracks the frontmost app for paste targeting (`ActiveAppInfo`
in `paste.rs` or similar). Reuse that data.

Behavior variants:
- **Coding apps** (VS Code, Xcode, Terminal, iTerm2): strict `@` matching,
  strict `slash` matching, relax `dot` context for code identifiers
- **Chat apps** (Slack, Discord, Teams): relaxed `@` matching for mentions,
  no `/` command conversion (Slack has its own slash commands)
- **Prose apps** (Notes, Pages, Word, email): default behavior, no special variants
- **Terminals** (Terminal, iTerm2, Warp): autocomplete spacing cleanup on

## Implementation

### New file: `src-tauri/src/pre_format.rs`

Pure functions, no state, no IO. Well-suited for unit tests.

```rust
pub struct FormatContext {
    pub app_name: String,
    pub bundle_id: String,
    pub window_title: String,
}

pub struct FormatOptions {
    pub convert_punctuation: bool,
    pub convert_literal: bool,
    pub punctuation_prefix: String,  // default: "type"
}

pub fn pre_format(
    text: &str,
    context: &FormatContext,
    options: &FormatOptions,
) -> String {
    let mut result = text.to_string();
    if options.convert_punctuation {
        result = apply_spoken_punctuation(&result, &options.punctuation_prefix, context);
    }
    if options.convert_literal {
        result = apply_literal_formatting(&result, context);
    }
    result
}
```

### Integration in `cleanup.rs`

Call `pre_format::pre_format()` just before the Chaperone LLM call:

```rust
pub fn run_pipeline(transcript: &str, context: &FormatContext) -> String {
    let formatted = if settings.pre_formatter_enabled {
        pre_format(transcript, context, &settings.format_options)
    } else {
        transcript.to_string()
    };
    // existing Chaperone pipeline
    classsify_and_clean(&formatted)
}
```

### Data structures

```rust
#[derive(Clone)]
enum PunctuationSpacing {
    RightAttached,
    LeftAttached,
    NoSpaceAround,
    SpaceAround,
    ToggleDoubleQuote,
    ToggleSingleQuote,
}

#[derive(Clone)]
struct PunctuationRule {
    phrase: Vec<&'static str>,        // ["exclamation", "mark"]
    symbol: &'static str,             // "!"
    spacing: PunctuationSpacing,
    context_guard: Option<ContextGuard>,
}

enum ContextGuard {
    DotContext,        // domain-like tokens nearby
    SymbolContext,     // coding context
    SlashPathContext,  // path-like tokens nearby
    AtSignContext,     // coding/terminal/chat app
}

struct RulesTable {
    by_first_word: HashMap<&'static str, Vec<(usize, PunctuationRule)>>,
    // (phrase_len, rule), sorted longest-first
}
```

### Settings additions (`settings.rs`)

```rust
pub struct PreFormatSettings {
    pub enabled: bool,                          // master toggle, default: true
    pub convert_punctuation: bool,              // default: true
    pub convert_literal: bool,                  // default: true
    pub punctuation_prefix: String,             // default: "type"
    pub terminal_apps: Vec<String>,             // default: ["com.apple.Terminal", ...]
    pub chat_apps: Vec<String>,                 // default: ["com.tinyspeck.slackmacgap", ...]
    pub coding_apps: Vec<String>,               // default: ["com.microsoft.VSCode", ...]
}
```

## Out of scope

- No changes to the Chaperone LLM call itself (it still runs on the pre-formatted text)
- No machine learning or model loading
- No network calls
- No clipboard or paste changes
- No changes to the audio capture or transcription pipeline

## Success signal

- Input "I need coffee type period and a nap" → output "I need coffee. and a nap"
- Input "slash open home dot slash src" (in coding context) → output "/open home./src"
- Input "run slash deploy" → output "run /deploy"
- Input "at sign bob" (in Slack) → output "@bob"
- Input "at the office" (any app) → unchanged
- Input "type open paren hello world close paren" → output "(hello world)"
- All formatting functions are pure (no IO, no state)
- `cargo test` covers all punctuation rules
- `cargo check && cargo clippy` pass
