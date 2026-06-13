# TurboTalk Roadmap

Personal-use scope. Milestones are checkpoints, not deadlines. This file should
show the current shape of the project, not preserve every old task as if it were
still open.

## Current Strategy

- **1.0:** macOS + Windows confidence release.
- **2.0:** Linux-ready release.
- **Not a public product yet:** promotion still depends on daily use,
  at least one non-Eldo proof, and the Chaperone Layer proving worth shipping.

## Validation Legend

- **Proven:** recorded end-to-end or release/CI proof exists in `TRUTH.md`,
  `SESSION-STATUS.md`, or a referenced smoke/release doc.
- **Implemented / needs runtime proof:** source is in tree and compile/typecheck
  proof exists, but the exact user-visible path still needs a manual runtime pass.
- **Open:** not implemented, not packaged, or deliberately deferred.

## Completed / Proven History

### M0-M5 — Core Product Built

- [x] Tauri 2 + Svelte 5 app scaffold
- [x] Shared UI/common foundation wired
- [x] Tray-resident behavior on macOS
- [x] macOS push-to-talk dictation loop
- [x] Mic capture, WAV finalization, VAD trim, normalization, and pre-roll path
- [x] Whisper backend with persistent worker/server path
- [x] Moonshine backend end-to-end
- [x] Parakeet backend end-to-end
- [x] Backend selector and model catalog/download path
- [x] Chaperone cleanup via local Ollama classifier-router
- [x] Deterministic cleanup modes and voice commands
- [x] Settings, history persistence, model selection, custom vocabulary
- [x] macOS onboarding/readiness flow
- [x] Baseline recording overlay and tray indicators
- [x] Launch at login on macOS
- [x] Hallucination/repetition detection
- [x] Streaming chunked transcription
- [x] Segment recovery no longer pollutes history
- [x] Privacy boundary for diagnostics/bug reports: transcript text excluded

Proof source: `TRUTH.md`, `SESSION-STATUS.md`, `docs/pre-release-scans/`, and
historical smoke notes.

### M6A — Packaging + Release Plumbing

- [x] macOS app/DMG packaging
- [x] Windows NSIS installer packaging
- [x] Windows sidecar fetch path
- [x] VAD model fetch/bundle path
- [x] Icon generation for macOS/Windows
- [x] Release CI produces v0.9.8 artifacts
- [x] Release updater artifacts fixed in CI
- [x] macOS bundle codesign gate added for release CI
- [x] Manual-update/updater posture documented

## Implemented / Needs Runtime Proof

These are in tree, but should not be treated as fully closed until the named
manual proof is recorded.

- [x] **Windows full dictation loop:** hotkey/paste works on real hardware.
- [x] **Cancel-after-release suppression:** cancel before paste suppresses
  transcript/paste.
- [ ] **Device-lost next-press recovery:** source fix is in tree; real
  unplug/switch-mic repro still pending.
- [x] **Main-window placement safeguards:** window remains visible/reachable on
  smaller displays and monitor changes.
- [x] **Overlay size indicators:** Small / Medium / Large overlay modes are
  complete.
- [x] **Bug-report button:** Settings → Developer bug-report flow is complete.
- [x] **Windows onboarding persistence:** onboarding completion persists on
  Windows.
- [ ] **Windows tray/icon/light-mode polish:** older smoke results reported
  icon/light-mode issues; do not mark fixed without a current smoke note.

## Active 1.0 Closure

The 1.0 bar is not “add major features.” It is “prove the thing we already
built, from installed artifacts, on macOS and Windows.”

### M6B — Runtime Proofs Remaining

- [x] **Windows final dictation proof:** from the Windows installer on real
  hardware, trigger the configured hotkey, dictate into a common text target,
  and confirm paste lands correctly.
- [x] **Cancel-after-release proof:** start a dictation, release to begin
  transcription/cleanup, trigger cancel before paste, and confirm no text is
  pasted.
- [x] **Window-placement proof:** with the smaller laptop display attached,
  drag/resize the main window across monitors and toward edges; confirm it
  remains visible and reachable at 420×420.
- [x] **Bug-report button proof:** Settings → Developer → Create Bug Report
  saves a useful report bundle and excludes transcript text.
- [ ] **Device-lost proof:** unplug/switch mic mid-recording, release, then
  start a new recording; confirm the next press is normal and not instantly
  cancelled.
- [x] **Windows onboarding/tray smoke:** install on Windows, complete
  onboarding, quit/relaunch, confirm onboarding stays complete and tray/icon/UI
  polish are acceptable.
- [x] **Installed-artifact smoke:** clean install on macOS and Windows, complete
  onboarding, dictate into a text target, quit/relaunch, verify settings/history.

### M6C — 1.0 Release Decision

- [x] 1.0 ships unsigned/ad-hoc; Developer ID / Authenticode signing is
  deferred.
- [x] Update `README.md`, `docs/BUILD.md`, `docs/RELEASING.md`, and
  `docs/SMOKE-TEST.md` so they match the actual 1.0 platform promise.
- [ ] Cut the 1.0 release only after the runtime proofs above are recorded in
  `TRUTH.md`.

## Out of Scope for 1.0

- Linux runtime support.
- Public product promotion.
- New transcription features that are not required to stabilize the current
  macOS/Windows loop.

## 2.0 Linux Track

Linux is not an afterthought for the product, but it is intentionally separate
from 1.0. The Linux problem is mostly global input + paste policy, especially
under Wayland.

### M7 — Linux Feasibility Map

- [ ] Write a Linux capability matrix covering GNOME/KDE, X11/Wayland, tray
  availability, global hotkey support, paste support, clipboard support, audio
  backend, and packaging target.
- [ ] Add runtime session detection: OS, desktop environment,
  `XDG_SESSION_TYPE`, compositor hints, helper availability, and tray support.
- [ ] Add Linux readiness diagnostics that classify the machine as `full-loop`,
  `copy-only`, or `unsupported`.
- [ ] Document the first Linux promise: X11 full loop first; Wayland starts as
  portal/copy-only unless proven otherwise.

Proof: on one X11 session and one Wayland session, diagnostics reports the
correct support class and explains missing pieces.

### M8 — Linux Packaging + Local Runtime

- [ ] Choose first packaging format for Linux testing.
- [ ] Add Linux whisper.cpp sidecar packaging or a documented local-backend path.
- [ ] Package/validate ONNX runtime libraries for Moonshine/Parakeet if those
  remain supported on Linux.
- [ ] Validate `cpal` mic capture on PipeWire/PulseAudio/ALSA-backed systems.
- [ ] Confirm config/model paths under `~/.config/turbotalk/`.
- [ ] Add Linux release matrix only after local Linux artifact smoke is real.

Proof: fresh Linux install can record mic audio, transcribe locally, and
display/copy transcript text inside TurboTalk.

### M9 — Linux X11 Full Loop

- [ ] Implement `hotkey_linux_x11` with a real global push-to-talk binding.
- [ ] Implement `paste_linux_x11`: clipboard write + synthetic Ctrl+V.
- [ ] Verify overlay/cursor-dot placement and click-through behavior under X11.
- [ ] Add X11 smoke proof across a text editor, browser field, and Electron
  editor.

Proof: from a packaged Linux artifact on X11, hold the configured hotkey, say
"hello world", release, and `hello world` appears in the focused text field.

### M10 — Wayland Strategy

- [ ] Evaluate GlobalShortcuts portal availability for GNOME/KDE.
- [ ] Evaluate paste options: clipboard-only, portal-mediated action,
  user-configured system shortcut, or documented helper bridge.
- [ ] Implement honest Wayland readiness states:
  - `full-loop` only when hotkey and paste are both proven
  - `copy-only` when transcription works but paste must be manual
  - `unsupported` when recording/transcription prerequisites are missing
- [ ] Add first-run guidance for copy-only mode.
- [ ] Avoid privileged background services unless there is a deliberate
  product decision.

Proof: on GNOME Wayland and KDE Wayland, TurboTalk either completes the full
loop through approved mechanisms or clearly lands in copy-only mode.

### M11 — Linux 2.0 Release Candidate

- [ ] Smoke matrix: macOS, Windows, Linux X11, and Linux Wayland fallback/full
  loop as applicable.
- [ ] Release artifacts built for all supported targets.
- [ ] Diagnostics and bug reports include Linux session type, desktop
  environment, backend validity, clipboard/paste mode, and hotkey mode.
- [ ] Documentation names exactly which Linux desktops are supported and which
  fallback mode each one gets.
- [ ] No regressions to the macOS/Windows 1.0 proof path.

Proof: one clean install per supported Linux mode produces the documented
behavior from a release artifact.
