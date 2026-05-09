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

## Audio Pipeline Contract

`audio.rs` follows a fixed, quality-first sequence after the push-to-talk
hotkey is released. Order is load-bearing and the constants in `audio.rs`
(`TARGET_SAMPLE_RATE`, `TARGET_CHANNELS`, `TARGET_BITS_PER_SAMPLE`,
`MIN_RECORDING_MS`) are the single source of truth.

```
native mic capture (cpal, device-native rate / channels / format)
        │  no work in the cpal callback beyond append + RMS update
        ▼
downmix to mono            (TARGET_CHANNELS = 1)
        ▼
resample to 16 kHz         (TARGET_SAMPLE_RATE, rubato FftFixedIn)
        ▼
Silero VAD trim            (silence trim happens *after* resample because
                            Silero v4 expects 16 kHz mono f32 frames)
        ▼
min-duration reject        (MIN_RECORDING_MS, drop with DiscardReason)
        ▼
peak normalize             (NORMALIZE_PEAK ≈ -1 dBFS, one-way boost only)
        ▼
write 16 kHz mono 16-bit PCM WAV  → handed to whisper-cli sidecar
```

**Disk handoff format.** The temporary file passed to `whisper-cli` is
*always* 16 kHz mono 16-bit PCM WAV. Compressed codecs (MP3, AAC, Opus,
FLAC) are intentionally not used: at 16 kHz mono 16-bit the file is
~32 KB/s, the WAV header is trivial, and whisper-cli's preferred input
format is uncompressed PCM. Adding a codec would add encode latency,
decode latency on whisper's side, and a quality loss for no real-world
size win.

**Why VAD runs after resample.** Silero v4 ships its weights for 16 kHz
mono f32 input and expects fixed-size frames at that rate. Running VAD
*before* resample would either (a) require a second Silero model variant
per device sample rate, or (b) silently degrade detection quality on
44.1 / 48 kHz Bluetooth devices. Doing one well-anti-aliased FFT
resample first keeps Silero on its happy path and keeps frame indexing
honest about timing.

**Stage timing evidence.** `stop()` instruments each post-release stage
(capture clone, downmix, resample, VAD, normalize, WAV write, total) and
emits a single compact `tracing::info!` line per finalization. This is
the evidence base for later persistent-Whisper / cached-VAD work; do
not regress the log line without replacing it with something at least
as detailed.

## Paste Target Policy

`paste.rs` injects text into whichever app is **frontmost at the moment Cmd+V
is sent**, not the app that was frontmost when recording started. For a
personal one-in-flight push-to-talk tool, "wherever I am now" matches user
expectation more reliably than "wherever I was 1–3 seconds ago" — the user
has, by then, often deliberately switched to where they want the text to
land.

**Observability.** `paste::frontmost_app()` is a best-effort macOS helper
that captures a coarse identifier (frontmost process name via osascript +
System Events) at two points:

1. Recording start — captured in `hotkey::ptt_down`, stored alongside the
   `job_id` in the `FOCUS_AT_START` mutex.
2. Immediately before paste — captured in `hotkey::ptt_up`'s pasting stage.

Both values are logged on a single `tracing::info!` line keyed by `job_id`:

```
[paste job_id=Some(7)] focus_at_start=Some("TextEdit") focus_at_paste=Some("Notes")
```

If they differ, an additive `focus-changed-before-paste` event is emitted
with `{ job_id, focus_at_start, focus_at_paste }`. The frontend renders a
short banner so the user knows the destination drifted. Either field may
be `None` if the macOS query failed at that capture site; missing data is
treated as "unknown" and never blocks paste.

**What this is not.** The current default is "paste anyway, surface the
change." We do *not* skip paste on focus mismatch, do *not* warn before
paste, and do *not* try to refocus the original app. These are deliberate
non-features for the personal-use scope.

**Future queueing must revisit this.** As soon as we allow more than one
in-flight dictation job (queue or pipeline), pasting into the current focus
becomes dangerous: a job that finishes minutes after recording could land
its output in a totally unrelated app. When that work begins, this section
must be rewritten before the queue is enabled — at minimum: per-job
target-app capture, opt-in "paste only into recorded focus" mode, or a
visible review-then-paste step.

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
