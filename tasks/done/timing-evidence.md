# Timing Evidence — TASK-21 Phase 2

Fill in each section by pasting the log lines from the dev terminal.
The agent (Phase 3) parses this file, so keep the headings exactly as
written. Do not delete the `paste here:` markers — replace them.

## Short recording

Scenario: hotkey held, said "the quick brown fox jumps over the lazy dog",
released. ~3s of audio.

```
6:41:32 AM [vite-plugin-svelte] src/App.svelte:822:14 Buttons and links should either contain text or have an `aria-label`, `aria-labelledby` or `title` attribute
https://svelte.dev/e/a11y_consider_explicit_label
2026-05-03T13:42:19.879259Z  INFO turbotalk_lib::audio: [audio] opening stream: "MacBook Air Microphone" 48000 Hz 1 ch F32
2026-05-03T13:42:19.939973Z  INFO turbotalk_lib::audio: [audio] recording started
2026-05-03T13:42:19.940000Z  INFO turbotalk_lib::recorder: [recorder] Ready → Recording
2026-05-03T13:42:20.296043Z  INFO turbotalk_lib::hotkey: [hotkey job_id=1] recording started focus_at_start=Some("Terminal")
2026-05-03T13:42:23.751373Z  INFO turbotalk_lib::recorder: [recorder] Recording → FinalizingAudio
2026-05-03T13:42:23.792908Z  INFO turbotalk_lib::audio: [audio] 181760 samples captured (48000 Hz, 1 ch — pre-resample)
2026-05-03T13:42:23.912626Z  INFO turbotalk_lib::audio: [audio] 60586 samples after resample (16000 Hz, 1 ch)
2026-05-03T13:42:23.942459Z  INFO turbotalk_lib::vad: [vad] timings (ms): init=19.51 reset=0.01 compute=10.28 frames=127
2026-05-03T13:42:23.943961Z  INFO turbotalk_lib::audio: [audio] wrote 37440 samples (2.34s trimmed) → "/var/folders/4x/k1tmnw492f53d73dq0hnzy080000gn/T/turbotalk-suYByz.wav"
2026-05-03T13:42:23.943974Z  INFO turbotalk_lib::audio: [audio] stage timings (ms): capture_clone=11.94 downmix=0.13 resample=119.52 vad=29.85 normalize=0.32 wav_write=1.14 total=192.49
2026-05-03T13:42:23.945970Z  INFO turbotalk_lib::recorder: [recorder] FinalizingAudio → Transcribing
2026-05-03T13:42:23.946294Z  INFO turbotalk_lib::transcribe: [transcribe] worker built for model "/Users/eldo/.config/librewin/turbotalk/models/ggml-large-v3-turbo.bin"
2026-05-03T13:42:27.041686Z  INFO turbotalk_lib::transcribe: [transcribe] whisper took 3095 ms
2026-05-03T13:42:27.041875Z  INFO turbotalk_lib::recorder: [recorder] Transcribing → Cleaning
2026-05-03T13:42:27.041926Z  INFO turbotalk_lib::hotkey: [transcribe job_id=Some(1)] raw "Testing, testing, recording."
2026-05-03T13:42:27.042464Z  INFO turbotalk_lib::hotkey: [cleanup   job_id=Some(1)] final "Testing, testing, recording."
2026-05-03T13:42:27.042528Z  INFO turbotalk_lib::recorder: [recorder] Cleaning → Pasting
2026-05-03T13:42:27.167664Z  INFO turbotalk_lib::hotkey: [paste job_id=Some(1)] focus_at_start=Some("Terminal") focus_at_paste=Some("Terminal")
2026-05-03T13:42:27.481740Z  INFO turbotalk_lib::recorder: [recorder] Pasting → Ready (finish)
```

## Long recording

Scenario: hotkey held, ~2s of leading silence, said
"this is the long recording with silence at both ends", ~3s of trailing
silence, released. ~10s of audio.

```
2026-05-03T13:43:41.048581Z  INFO turbotalk_lib::audio: [audio] opening stream: "MacBook Air Microphone" 48000 Hz 1 ch F32
2026-05-03T13:43:41.099153Z  INFO turbotalk_lib::audio: [audio] recording started
2026-05-03T13:43:41.099179Z  INFO turbotalk_lib::recorder: [recorder] Ready → Recording
2026-05-03T13:43:41.217445Z  INFO turbotalk_lib::hotkey: [hotkey job_id=2] recording started focus_at_start=Some("turbotalk")
2026-05-03T13:44:08.677026Z  INFO turbotalk_lib::recorder: [recorder] Recording → FinalizingAudio
2026-05-03T13:44:08.717837Z  INFO turbotalk_lib::audio: [audio] 1323520 samples captured (48000 Hz, 1 ch — pre-resample)
2026-05-03T13:44:09.373072Z  INFO turbotalk_lib::audio: [audio] 441173 samples after resample (16000 Hz, 1 ch)
2026-05-03T13:44:09.446566Z  INFO turbotalk_lib::vad: [vad] timings (ms): init=0.00 reset=0.00 compute=73.46 frames=920
2026-05-03T13:44:09.459049Z  INFO turbotalk_lib::audio: [audio] wrote 366240 samples (22.89s trimmed) → "/var/folders/4x/k1tmnw492f53d73dq0hnzy080000gn/T/turbotalk-vE7byq.wav"
2026-05-03T13:44:09.459061Z  INFO turbotalk_lib::audio: [audio] stage timings (ms): capture_clone=12.31 downmix=0.74 resample=654.42 vad=73.51 normalize=3.12 wav_write=9.32 total=781.94
2026-05-03T13:44:09.461097Z  INFO turbotalk_lib::recorder: [recorder] FinalizingAudio → Transcribing
2026-05-03T13:44:11.536239Z  INFO turbotalk_lib::transcribe: [transcribe] whisper took 2074 ms
2026-05-03T13:44:11.536415Z  INFO turbotalk_lib::recorder: [recorder] Transcribing → Cleaning
2026-05-03T13:44:11.536451Z  INFO turbotalk_lib::hotkey: [transcribe job_id=Some(2)] raw "All right, we're gonna test a longer recording I guess how long is this one? I think it's 20 seconds 10 seconds Okay"
2026-05-03T13:44:11.536758Z  INFO turbotalk_lib::hotkey: [cleanup   job_id=Some(2)] final "All right, we're gonna test a longer recording I guess how long is this one? I think it's 20 seconds 10 seconds Okay"
2026-05-03T13:44:11.536795Z  INFO turbotalk_lib::recorder: [recorder] Cleaning → Pasting
2026-05-03T13:44:11.662343Z  INFO turbotalk_lib::hotkey: [paste job_id=Some(2)] focus_at_start=Some("turbotalk") focus_at_paste=Some("zed")
2026-05-03T13:44:11.662377Z  WARN turbotalk_lib::hotkey: [paste job_id=2] focus changed before paste: "turbotalk" → "zed"
2026-05-03T13:44:11.947721Z  INFO turbotalk_lib::recorder: [recorder] Pasting → Ready (finish)

```

## Host info

```
eldo@Elliotts-Air TurboTalk-App % uname -m
arm64
eldo@Elliotts-Air TurboTalk-App % sw_vers
ProductName:		macOS
ProductVersion:		26.4.1
BuildVersion:		25E253
eldo@Elliotts-Air TurboTalk-App % 

```

## Notes

(Optional. Anything weird you noticed — repeated runs, retries, errors
between the two log lines, etc.)
