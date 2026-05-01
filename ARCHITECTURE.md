# TurboTalk Architecture

## Goals

1. **Push-to-talk dictation** — press hotkey, speak, release, see text in active field.
2. **Fully local** — no network calls in default config.
3. **Fast** — sub-second from release-key to text-paste for short utterances.
4. **Smart cleanup** — LLM postprocessor handles punctuation and classifies output type (prose / code / command).
5. **Out of the way** — invisible most of the time. Tray icon + tiny floating overlay only while recording.

## Component Map

```
┌──────────────────────────────────────────────────────────┐
│                     Tauri 2 host                         │
│                                                          │
│  ┌────────────┐    ┌────────────┐    ┌──────────────┐   │
│  │  hotkey    │───▶│  recorder  │───▶│   audio      │   │
│  │  (global)  │    │  (3-state) │    │   (cpal mic) │   │
│  └────────────┘    └─────┬──────┘    └──────────────┘   │
│                          │                              │
│                          ▼                              │
│                   ┌────────────┐                        │
│                   │ transcribe │  whisper.cpp sidecar   │
│                   │  (local)   │  (ggml model)          │
│                   └─────┬──────┘                        │
│                         │                               │
│                         ▼                               │
│                   ┌────────────┐                        │
│                   │  cleanup   │  Ollama / llama.cpp    │
│                   │ (chaperone)│  classifier-router     │
│                   └─────┬──────┘                        │
│                         │                               │
│                         ▼                               │
│                   ┌────────────┐                        │
│                   │   paste    │  osascript / enigo     │
│                   │            │  → active app          │
│                   └────────────┘                        │
│                                                          │
│  ┌──────────────┐         ┌──────────────────┐          │
│  │  settings UI │         │ recording overlay│          │
│  │  (Svelte 5)  │         │  (Svelte 5)      │          │
│  └──────────────┘         └──────────────────┘          │
└──────────────────────────────────────────────────────────┘
```

## Module Responsibilities

| Module | Responsibility | Key Crate |
|---|---|---|
| `hotkey.rs` | Register global push-to-talk binding; emit press/release events | `tauri-apps/global-hotkey` |
| `audio.rs` | Open mic stream, buffer 16kHz mono PCM into a ring | `cpal`, `hound` |
| `recorder.rs` | State machine: Ready → Recording → Transcribing → Ready | (own) |
| `transcribe.rs` | Spawn whisper.cpp sidecar, feed WAV, parse stdout | `tokio`, sidecar |
| `cleanup.rs` | Pipe transcript through local LLM with system prompt; classify output type; apply formatting | `reqwest` (Ollama) |
| `paste.rs` | Inject text into focused app via macOS clipboard + Cmd+V (or enigo on Win/Linux) | `arboard`, `enigo` |
| `settings.rs` | Read/write `~/.config/librewin/turbotalk/config.toml` | `serde`, `toml` |

## State Machine (recorder.rs)

```
Ready ──hotkey-down──▶ Recording ──hotkey-up──▶ Transcribing ──done──▶ Ready
  ▲                                                             │
  └─────────────────────error/cancel────────────────────────────┘
```

Three states, no in-flight overlap. If user holds hotkey again while Transcribing, queue or drop (TBD — drop is simpler).

## Chaperone Layer (cleanup.rs)

The differentiator. Voice transcription output is messy: no punctuation, mixed-case, "uh", "um", false starts. A naive paste is unusable.

The Chaperone is a small local LLM (Llama 3.2 3B or similar) wired as a **classifier-router**:

1. **Classify** the utterance into one of N hand-defined modes:
   - `prose` — normal sentences (apply punctuation + capitalization)
   - `code` — technical content (preserve identifiers, no autocorrect, fenced)
   - `command` — meta-instruction to TurboTalk itself ("scratch that", "new paragraph", "send")
   - `raw` — paste verbatim, no edit
2. **Route** to a deterministic handler per mode. The LLM never freely rewrites — it picks a handler.
3. **Apply** the handler (regex / template / passthrough) to produce final text.

Closed action space, open input space. Reference: `Business-OS/memory/project_chaperone_layer.md`.

## Settings Storage

Following Libre convention: `~/.config/librewin/turbotalk/config.toml`. Includes:
- Hotkey binding (default: `F1` hold)
- Whisper model path / size
- Cleanup mode (off / regex-only / chaperone)
- Mic device override

## Build & Distribution

- Single binary, codesigned with the Libre signing infrastructure (see `Libre-Apps/docs/specs/SIGNING.md` when ready).
- Whisper.cpp + ggml model shipped as sidecar.
- No installer for personal use; just `cargo build --release` and run.

## Reference Implementations (read these, do not copy)

- **Handy** (`cjpais/Handy`) — closest to our target. Read its hotkey + audio + transcribe wiring.
- **typr** (`albertshiney/typr`) — clean 3-state recorder pattern in `recorder.rs`.
- **sagascript** (`Magnus-Gille/sagascript`) — minimal macOS-specific glue.
