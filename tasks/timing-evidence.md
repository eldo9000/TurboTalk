# Timing Evidence — TASK-21 Phase 2

Fill in each section by pasting the log lines from the dev terminal.
The agent (Phase 3) parses this file, so keep the headings exactly as
written. Do not delete the `paste here:` markers — replace them.

## Short recording

Scenario: hotkey held, said "the quick brown fox jumps over the lazy dog",
released. ~3s of audio.

```
paste here: [audio] stage timings (ms): capture_clone=… downmix=… resample=… vad=… normalize=… wav_write=… total=…
paste here: [transcribe] whisper took … ms
```

## Long recording

Scenario: hotkey held, ~2s of leading silence, said
"this is the long recording with silence at both ends", ~3s of trailing
silence, released. ~10s of audio.

```
paste here: [audio] stage timings (ms): capture_clone=… downmix=… resample=… vad=… normalize=… wav_write=… total=…
paste here: [transcribe] whisper took … ms
```

## Host info

```
paste here: uname -m output
paste here: sw_vers output
```

## Notes

(Optional. Anything weird you noticed — repeated runs, retries, errors
between the two log lines, etc.)
