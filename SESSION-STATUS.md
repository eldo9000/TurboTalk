# TurboTalk — Session Status

**Last updated:** 2026-05-05
**Current state:** TASK-27 (Whisper sidecar bundling) landed for Windows.
`scripts/fetch-sidecars.mjs` downloads upstream whisper.cpp v1.8.4
`whisper-bin-x64.zip` (sha256 verified), extracts the 5 runtime files into
`src-tauri/binaries/`, and renames `whisper-cli.exe` to the Tauri target-triple
convention. `src-tauri/tauri.windows.conf.json` declares the four runtime DLLs
as `bundle.resources` so the NSIS installer ships them alongside the .exe.
`scripts/preflight.mjs` win32 list now requires those five files. The CI
release workflow swapped its from-source cmake step for `npm run fetch-sidecars`,
saving a multi-minute CI cmake build per push. macOS happy path preserved:
`npm run package` reproduced `dist-artifacts/TurboTalk-0.8.1-macos-arm64.dmg`
with no regression. The `.gitignore` excludes the fetched Windows files so
they only exist on a Windows host post-fetch.

Win sidecar fetch path verified by CI run 25378189425 (workflow_dispatch on
`0e9ad71`): both matrix legs (macos-arm64, windows-x64) green. The Windows
runner ran `npm run fetch-sidecars` → preflight → `tauri build` and uploaded
the artifact. Runtime proof on a real Windows box still pending — hotkey +
paste are still `Err("unsupported platform")` stubs (TASK-25 / TASK-26).

Previous state: Overlay peek-through fix verified by user in an Accessibility-
trusted build. Holding Right Alt opens the recording pill; cursor hover dims it
via direct background alpha + backdrop blur reduction without stealing focus.
Frontend fix in `src/Overlay.svelte` (commit `fa9d02e`) is now confirmed end-to-
end.

Previous state: Win/Linux beta sprint partial loop landed (TASK-24, 28, 30, 31).
Codebase is now structurally cross-platform-aware: target-triple sidecar lookup,
per-platform bundle resources, host-aware preflight + artifact-rename scripts,
unsigned-beta docs, and a 3-OS GH Actions release workflow. Mac happy path
preserved. Win/Linux runtime impls (hotkey, paste, sidecar binaries, per-OS
diagnostics) still deferred — TASK-25/26/27/29 remain in tasks/.

v0.8 macOS beta package proof is now understood. The
ad-hoc signed DMG exists at `dist-artifacts/TurboTalk-0.8.0-macos-arm64.dmg`
with a passing `.sha256`. Packaged diagnostics confirmed mic input works and
the bundled sidecar resolves from `/Applications/Turbo Talk.app/Contents/MacOS/whisper-cli`.
The packaged hotkey works after manually removing/re-adding the installed app
in System Settings → Privacy & Security → Accessibility. Temporary beta
diagnostic scaffolding has been removed from the production UI and command
surface; the core diagnostics command remains dev-only in Settings. Apple
Developer signing/notarization remains intentionally skipped for v0.8.

## Where We Are

Roadmap M0 and M1 are done. Core loop works:
Right Alt → record → whisper transcription → paste into focused app.

Commits this session:
- `5063e13` scaffold: Tauri 2 + Svelte 5 + librewin foundation
- `12726ba` feat: hotkey + mic capture (CGEventTap, cpal, hound)
- `0d5d5be` feat: whisper transcription (whisper-cli, ggml-base.en)
- `6557ed8` feat: paste into focused app (arboard + osascript)
- `0c00ece` fix: UI chrome + transcribing state reset

## Active Focus

Mac beta readiness. The audit concluded true Windows/Linux beta is deferred;
current beta scope is macOS arm64 unless/until non-mac hotkey/paste and
Whisper sidecars land.

Open carry-overs are TASK-18's still-deferred warm Whisper backend (gated
on whisper-rs-sys cmake fix or a maintained whisper-server crate; option 3
lifecycle wrapper landed in TASK-20).

## Blockers

None.

## Next action

Win CI build proven green. Next M4 leverage points:
- TASK-25 / TASK-26 — real Windows hotkey + paste impls (today: stubs).
  Without these, the Windows `.exe` artifact installs but cannot dictate.
- Optional: download the `windows-x64` artifact from CI run 25378189425 and
  smoke-test the installer on a real Windows box to confirm the shipped
  whisper-cli.exe + DLLs load and respond to `--help`.
- Codesigning + notarization (gated on Apple Developer credentials).

Remaining v0.8 release note:
- Document/communicate first-run install caveat: if the packaged hotkey is
  dead, remove/re-add `/Applications/Turbo Talk.app` under Accessibility,
  enable it, then quit/reopen.
- Developer ID signing/notarization is deferred until credentials are available.

Cleanup update 2026-05-04:
- Removed temporary troubleshooting hooks: debug log command/writer, manual
  record commands, production-visible hold-to-record fallback controls, and
  expanded mic/accessibility probe fields from copied diagnostics.
- Kept production fixes discovered during packaging: macOS microphone
  `Info.plist`, packaged sidecar lookup for `Contents/MacOS/whisper-cli`, and
  user-facing Accessibility failure messaging.
- Proof after cleanup: `cargo test --manifest-path src-tauri/Cargo.toml
  export_bindings`, `npm run build`, full `cargo test --manifest-path
  src-tauri/Cargo.toml`, `npm run package`, and `shasum -a 256 -c
  TurboTalk-0.8.0-macos-arm64.dmg.sha256` all pass.

Block 1 of `BETA-AUDIT-ROADMAP.md` dispatched 2026-05-03 as a 3-task arc:
- TASK-1 (`2cee14f`) — `PLATFORM-AUDIT.md` written: cargo check classification
  on Win/Linux, platform touch-points, sidecar/config inventory, capability
  table.
- TASK-2 (`701ddf4`) — `hotkey.rs` split into `#[cfg(target_os = "macos")] mod
  imp` + non-mac unsupported stub; core-graphics/core-foundation moved under
  target-specific deps.
- TASK-3 (`fb2ed79`) — `paste.rs` `paste()` and `frontmost_app()` gated to
  macOS; non-mac branch returns an "unsupported platform" Err.

Block 2 dispatched 2026-05-03 as a 3-task arc, all landed:
- TASK-1 (`f604caa`) — `scripts/preflight.mjs` checks macOS bundle assets
  before `tauri build`; wired via `npm run package` (preflight && build).
- TASK-2 (`298ffbc`) — `BUILD.md` + `scripts/rename-artifact.mjs` produce
  `dist-artifacts/TurboTalk-<version>-macos-arm64.dmg` on `npm run package`.
- TASK-3 (`26c900a`) — README "Release matrix" section: supported
  platforms, install steps, permissions, local data paths, known limits.

Human verification update:
- Dictation/paste proof is confirmed by user for the v0.8 beta candidate.
- Packaged app microphone proof is currently not confirmed: user reports the
  volume bar does not register input in the built macOS app.
- `npm run package` reached `.app` + DMG creation; artifact rename needed the
  repo-level `target/release/bundle/dmg` path.

Block 3 dispatched 2026-05-03 as a 4-task arc, all landed:
- TASK-1 (`9a61f81`) — `diagnostics.rs` + `run_diagnostics` Tauri command; 7-field health check.
- TASK-2 (`7fce12b`) — "Copy diagnostics" button in settings panel; clipboard + "Copied" confirmation.
- TASK-3 (`8560387`) — Actionable error strings for mic denied, sidecar missing, model missing, whisper exit, Ollama unreachable.
- TASK-4 (`c76fa9d`) — `SMOKE-TEST.md`: 7-step manual beta test script.

Block 4 dispatched 2026-05-03 as a 3-task arc, all landed:
- TASK-5 (`3182617`) — `PRIVACY.md`: local-first data handling, exact paths, deletion instructions.
- TASK-6 (`2e7c3df`) — `save_history` toggle + `open_data_folder` button wired end-to-end.
- TASK-7 (`64763ae`) — Chaperone locality hint shown when Advanced cleanup mode selected.

Block 5 dispatched 2026-05-03 as a 5-task arc, all landed:
- TASK-1 (`82ac00f`) — Developer ID signing config + entitlements.plist (hardened runtime, mic, AppleEvents, JIT, library-validation disable); BUILD.md "Release build (signed + notarized)" section with env vars and spctl verification.
- TASK-2 (`148fd58`) — `scripts/bump-version.mjs` keeps `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` in lockstep; `npm run bump-version -- <version>`.
- TASK-3 (`9f5a00e`) — `scripts/rename-artifact.mjs` extended to emit `<dmg>.sha256` in `shasum -a 256 -c` format.
- TASK-4 (`02be37e`) — `RELEASING.md`: pre-flight, version bump, signed build, tag + `gh release create`, manual-updates-only policy, copy-paste release notes template.
- TASK-5 (`2052e5f`) — `SMOKE-TEST.md` "Installed-artifact smoke test" section: 11-step procedure for verifying packaged DMG on a clean macOS account.

Pending human verification:
- Acquire Developer ID Application certificate and Apple Developer notary credentials.
- Run `APPLE_SIGNING_IDENTITY=... APPLE_ID=... APPLE_PASSWORD=... APPLE_TEAM_ID=... npm run package` and confirm `spctl -a -t open --context context:primary-signature -v dist-artifacts/TurboTalk-<v>-macos-arm64.dmg` returns "accepted".
- Run the SMOKE-TEST.md "Installed-artifact" section against the resulting DMG on a clean macOS account.

## Streaming-finalizer sprint (2026-05-03) — closed

TASK-22 shipped streaming audio finalizer. Concurrent resample + Silero
VAD off the cpal callback thread. Post-release finalization on the same
host as TASK-21:

- Short (1.41s after VAD): 192.49 ms → 49.60 ms (3.9× faster)
- Long  (8.13s after VAD): 781.94 ms → 61.94 ms (12.6× faster)

Both clear the < 250 ms gate and the < 100 ms aspirational target.
Quality preserved (no clipped first/last words).

- `3d13660` feat(audio): streaming audio finalizer — incremental resample + VAD off the callback

Post-landing evidence pasted into `tasks/done/TASK-22-implement-streaming-finalizer.md`.

## Hardening Sprint (2026-05-01) — closed

Multi-agent code review (security + architecture) → 8 tasks dispatched + landed.
All commits on main; tasks/done/ has the archived task files.

- `0b4d606` fix(security): CSP enabled in Tauri config (closes XSS class)
- `a8a75cd` fix(security): canonicalize subprocess paths (closes path traversal)
- `62243c7` docs(audio): SAFETY argument for unsafe Send/Sync on AudioCapture
- `d78abd4` refactor(cleanup): typed mode, URL allowlist, prompt isolation, 2s timeout
- `882ebdd` fix(recorder): type-enforce state transitions; paste-error/discarded events
- `4a0b654` fix(audio): RAII temp files, device-loss detection, recording-too-short
- `3526050` fix(history): backend-owned 50-entry limit, awaited save, ui-error channel
- `1a1ebed` chore(types): tauri-specta typed contract + multi-monitor overlay + SAFETY/TRUTH

Reports archived at `/tmp/static-analysis-main-20260501-1200.md` and
`/tmp/code-analysis-concern-based-main-20260501.md`.

## Dictation-quality sprint (2026-05-01) — closed

Research synthesized from cjpais/Handy reference impl, whisper.cpp issue
threads, and DSP literature → 4 tasks dispatched + landed via /triage-dispatch.
User-reported symptom: "mic not sensitive enough, output much worse than other
dictation apps." Root cause was zero audio preprocessing between cpal and
whisper.cpp — every comparable app does at least normalization + VAD.

- `2c08406` chore(tray): silence too_many_arguments on pixel helpers (prep)
- `9cbbffd` feat(audio): resample mic input to 16 kHz mono (rubato FftFixedIn)
- `6363ded` feat(audio): peak-normalize buffer to ~-1 dBFS (one-way boost only)
- `bbf5834` feat(audio): replace RMS trimmer with Silero VAD + hangover smoothing
- `55cfa21` feat(transcribe): tune whisper flags (--no-context, beam=5, temp=0,
            --suppress-blank) + wire cleanup.vocabulary into --prompt

Binary size +18.9 MB (statically-linked ort runtime + 1.8 MB Silero v4 ONNX
model). `cargo clippy -D warnings` clean across all five commits.

Tasks archived at `tasks/done/TASK-09..TASK-12.md`.

## Recent Decisions

- **rdev → CGEventTap** — macOS 26 broke rdev (TSM `dispatch_assert_queue` crash). Direct
  `CGEventTap` via `core-graphics 0.24`. Right Option detected by keycode 0x3D only, no TSM.
- **Homebrew whisper-cpp** — not bundled as Tauri sidecar yet; hardcoded path for now.
- **ggml-base.en** — 141MB, ~130ms on M4 via Metal. Adequate for M1.
- **Window: 380×280** — no custom titlebar, native macOS traffic lights only.
- **Reference, not fork** — built from scratch. Handy/typr/sagascript as references.

## TASK-21 (streaming-finalizer decision) — 2026-05-03

TASK-21: streaming-finalizer decision = implement (long ratio 35.7%, finalization 741.11 ms; resample-during-silence dominates). → TASK-22 created.

## TASK-19 (streaming audio finalizer) — deferred 2026-05-02

Deferred this sprint. Documentation-only completion; no source changes.

**Reason.** TASK-19 explicitly gates on Whisper-vs-finalization timing
evidence ("If Whisper still dominates and audio finalization is small, do
not implement this task"). Two facts make the gate fail-closed right now:

1. TASK-18 (persistent warm Whisper worker) was the dominant-latency
   target this sprint and was itself deferred — `whisper-rs` cmake build
   hangs on macOS 26.x. See `tasks/done/TASK-18-...`. So Whisper still
   dominates per-recording latency by definition.
2. No runtime data has been collected against the TASK-13 stage-timing
   instrumentation (no audio device available to the dispatcher/workers
   this sprint). The `[audio] stage timings (ms): capture_clone=… downmix=…
   resample=… vad=… normalize=… wav_write=… total=…` line in `audio.rs`
   `stop()` exists and is correctly wired, but has not been exercised
   end-to-end with a real microphone.

Optimizing audio finalization before either of those is doubly premature
and would add streaming-pipeline complexity with no measured payoff.

**Re-attempt condition.** Two gates, both required:
- Collect TASK-13 stage timings under realistic recording — at minimum
  one short push-to-talk dictation and one long recording with several
  seconds of leading + trailing silence.
- Only implement TASK-19 if `downmix + resample + vad + normalize +
  wav_write` exceeds Whisper transcription time on the long recording.
  Otherwise re-defer.

Task file moves to `tasks/done/TASK-19-…` with this deferral note as the
proof-of-completion.

## TASK-20 (warm Whisper backend retry) — 2026-05-02 → option 3 landed

Retried TASK-18 with a three-option ladder. Option 1 (`whisper-rs`) gate
**failed** again — same `cmTC_*` cmake hang as the first deferral, killed at
the 300-second budget. Option 2 (`whisper-server`) is **blocked** on a
packaging decision: the binary is not bundled in `src-tauri/binaries/` and
internet downloads were out of scope for this retry. Option 3 (serialized
worker around `whisper-cli`) **landed**: `TranscriptionWorker` in
`src-tauri/src/transcribe.rs` owns binary+model path validation, prompt
state, and a `Mutex` spawn lock; `lib.rs::save_config` invalidates the
cached worker on every save so model swaps and vocabulary edits are
picked up next dictation.

**Warmup is still pending.** Each transcribe call still spawns
`whisper-cli` and reloads the model — the lifecycle wrapper centralizes
the spawn path but does not amortize startup cost. Re-attempt unblocks
on either of (a) `whisper-rs-sys` upstream fix for the macOS 26.x cmake
hang, or (b) deciding to bundle `whisper-server` alongside `whisper-cli`
in `src-tauri/binaries/`.

`cargo build`, `cargo test` (66 passed), and
`cargo clippy -D warnings` all green for `src-tauri`.
