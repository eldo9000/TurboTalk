# Timing Protocol — TASK-21 Phase 2

You (the human) collect the evidence the agent uses to decide whether the
streaming audio finalizer (TASK-19) is worth implementing. Two recordings,
four log lines, ~5 minutes of work.

## Setup

1. From the repo root, launch the app in dev mode:
   ```
   npm run tauri dev
   ```
2. Keep that terminal **visible** — every `tracing::info!` line prints
   here. The lines you care about are:
   - `[audio] stage timings (ms): capture_clone=… downmix=… resample=… vad=… normalize=… wav_write=… total=…`
   - `[transcribe] whisper took N ms`

   Both fire **once per accepted recording**, in that order.
3. Open `tasks/timing-evidence.md` in another window — that's where the
   numbers go. The file already has the right shape; just fill in the
   `paste here:` placeholders.

## Recording 1 — Short

Hold the push-to-talk hotkey, say:

> the quick brown fox jumps over the lazy dog

Release. About 3 seconds of audio.

Copy the two log lines (`[audio] stage timings` and
`[transcribe] whisper took`) from the dev terminal into the
**`## Short recording`** section of `tasks/timing-evidence.md`.

## Recording 2 — Long with silence

Hold the push-to-talk hotkey. Wait **~2 seconds in silence**, then say:

> this is the long recording with silence at both ends

Then wait another **~3 seconds in silence** before releasing.
About 10 seconds of audio total, with leading + trailing silence so the
VAD trim stage has something real to do.

Copy the two log lines from the dev terminal into the
**`## Long recording`** section of `tasks/timing-evidence.md`.

## Host info

Run these two commands and paste their output into the
**`## Host info`** section:

```
uname -m
sw_vers
```

## When you're done

Save `tasks/timing-evidence.md` and re-dispatch TASK-21 (or run
`/triage tasks/TASK-21-streaming-finalizer-decision.md`). The agent will
parse the numbers, apply the decision rule, and write the verdict.

## Troubleshooting

- **The `[audio] stage timings` line never appeared.** The recording was
  rejected before reaching the timing log — most likely the
  `NoStream` path (`stop()` called with no active stream) or a panic
  earlier in the pipeline. Try again with a louder clearer utterance,
  and if it still doesn't appear, that's a phase-1 instrumentation bug.
  Stop and report.
- **Only `[audio] …` appears, no `[transcribe] whisper took …`.**
  Something in the recorder lifecycle bailed before the whisper spawn.
  Check the dev terminal for an `error!` line between the two and
  paste that into the evidence file under a `## Notes` heading.
