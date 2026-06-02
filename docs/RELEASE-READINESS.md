# TurboTalk Release Readiness

The pre-share checklist for cutting a TurboTalk version that goes out to **technical peers and business colleagues** — not the general public.

This document answers "is this fit to share with people who will look at the code?" It sits *above* `RELEASING.md` (which is the mechanical procedure) and `BUILD.md` (which is the build procedure). When a peer-share release is cut, this is the gate; once it passes, follow `RELEASING.md` to actually publish.

A different gate — heavier, with signing and supply-chain hygiene — applies before any public release. That gate lives in [§ Deferred until first public release](#deferred-until-first-public-release) below and is not in scope today.

---

## Audience and expectations

Peer-share readers will:
- Clone the repo and try to build it.
- Read the code, especially the IPC boundary, audio pipeline, and any unsafe blocks.
- Look at how secrets and user data are handled.
- Notice broken builds, committed binaries, leaked paths, and dead code.

They will *not*:
- File public bug reports.
- Be deterred by SmartScreen or Gatekeeper warnings (they know what those mean).
- Expect a marketing site or a signed installer.

Optimize for the first list. Defer the second.

---

## Portfolio stack standards (scoped to TurboTalk)

The portfolio-wide defaults live in [`Business-OS/standards/STACK-CHOICES.md`](https://github.com/eldo9000/Business-OS/blob/main/standards/STACK-CHOICES.md). Three apply; here is how they land for TurboTalk:

| Standard | Status | Notes |
|---|---|---|
| **CodeMirror 6** as embedded editor | N/A | TurboTalk's editable surfaces (vocabulary list, classifier prompt) are small enough that a plain `<textarea>` is the right tool. Adopt CodeMirror only if either surface grows into structured config. |
| **`keyring` crate** for all secrets | N/A | TurboTalk has no secrets to store — by design, no remote SDKs in the dependency graph. The rule becomes binding the moment any optional cloud feature is proposed: API keys never go to disk. |
| **Local-model fallback** for AI features | **Exemplar** | TurboTalk doesn't merely support local models — it is local-only. Whisper transcription and Ollama cleanup both run on the user's machine with no hosted alternative. This is the reference implementation of the principle for the rest of the portfolio. |

If a future change adds a cloud feature, surfaces structured-text editing, or removes the local-only architectural property, update this table and the corresponding portfolio-wide standard.

---

## Pre-share checklist

Each item is a hard gate for cutting a peer-share release. Items map back to "things a programmer reading the repo will notice."

### 1. Repo hygiene

- [ ] `git status` clean on `main`; no stray WIP files.
- [ ] `dist-artifacts/` is in `.gitignore` (it currently is *not* — add it). Tracked count is zero today, but a casual `git add .` would commit packaged binaries.
- [ ] No `.env`, `.env.local`, or other dotfiles in the working tree that aren't ignored.
- [ ] No fingerprintable absolute paths committed in code or docs (search for `/Users/`, `C:\Users\`, your username, your machine name).
- [ ] No vendored binaries that aren't sidecars (the bundled Whisper binaries are intentional; anything else is suspect).

### 2. Build hygiene

- [ ] Fresh-clone build works end to end on at least the primary platform: `git clone <url> && cd turbotalk && npm install && npm run package`. A peer who can't build it on the first try will close the tab.
- [ ] `npm run preflight` passes.
- [ ] Version values match across `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, artifact names, tag name, and release notes.
- [ ] Bundle preflight covers the real shipping surface: host sidecars, companion libraries, executable bits where required, `src-tauri/resources/silero_vad.onnx`, app icons, output artifact, and matching `.sha256`.
- [ ] `cargo clippy -- -D warnings` clean from `src-tauri/`.
- [ ] `cargo test` passes from `src-tauri/`.
- [ ] `npm run typecheck` passes.
- [ ] `npm run check` (svelte-check) passes if it exists in `package.json`.
- [ ] Beta packaging is intentionally unsigned/ad-hoc: no `APPLE_*` signing environment was used, macOS `signingIdentity` remains `"-"`, and the artifact is not notarized.
- [ ] Manual-update policy matches the code. `RELEASING.md` says the updater is intentionally not enabled for this beta; if updater dependencies/config/plugin registration remain present, document whether they are inert or remove/disable them before release.

### 3. Runtime hygiene

- [ ] One full pass of `docs/SMOKE-TEST.md` Test 3 (push-to-talk → paste into focused editor) on a packaged build, not just a `tauri dev` build.
- [ ] One full pass of the installed-artifact smoke test on a clean macOS user account or VM: install from DMG, complete first-run permission flow, dictate into TextEdit, quit/relaunch, verify persisted settings/history, then uninstall/delete data.
- [ ] App starts cleanly with no first-run crashes when no Whisper model is downloaded yet.
- [ ] Error paths surface as banners or toasts, not silent hangs (TurboTalk's recorder state machine already enforces this — verify it still holds).
- [ ] Quit from the tray fully exits — no orphaned `whisper-server` processes survive (this regression has bit before; verify with `pgrep whisper-server` after quitting).

### 4. Privacy claims testable

The README claims local-only with no telemetry. A reader may want to verify this themselves. The repo should make verification cheap:

- [ ] `docs/PRIVACY.md` is current and reflects the actual code paths.
- [ ] No new dependencies added since the last release that ship telemetry by default. Check the `package.json` and `Cargo.toml` diffs.
- [ ] A grep for HTTP client usage in `src-tauri/` returns only the local Ollama and local whisper-server endpoints — no external hosts.
- [ ] A repo-wide URL/network scan has an explained allowlist. Expected external URLs are release/download links, model-download links, documentation links, and validated browser-open links such as Ollama install pages; transcript and cleanup runtime traffic must remain localhost-only.
- [ ] Tauri IPC commands and capability files do not expose broad filesystem, shell/process, URL-opening, or network behavior beyond the documented app flows.

### 5. Documentation

- [ ] README installation and usage instructions match the current shipping behavior (model list, hotkey list, cleanup modes).
- [ ] `docs/BUILD.md`, `docs/RELEASING.md`, `docs/SMOKE-TEST.md`, and `docs/PRIVACY.md` reflect the current procedure and actual app behavior.
- [ ] A `CHANGELOG.md` exists at the repo root with an entry for this version. Even one bullet per release. Peer reviewers use it as a reading order.
- [ ] The git tag for this version (`v0.X.Y`) has a corresponding GitHub release with the artifacts attached and `.sha256` companion files. Peer-share should be a downloadable artifact, not "build it yourself."

### 6. Code under scrutiny

A reader will spend most of their time in three files: the recorder state machine, the IPC command surface, and the cleanup pipeline. Before sharing:

- [ ] Skim each for leftover `dbg!`, `println!`, and commented-out experimental code.
- [ ] Confirm no `.unwrap()` on user-controlled input paths (file picks, paste targets, Ollama endpoint URL).
- [ ] Confirm any `unsafe` block is annotated with a `// SAFETY:` comment explaining the invariant.

### 7. Beta release scan pack

For the next beta, run a coordinated scan pass before tagging. Treat each item as either **pass**, **fixed**, or **documented known issue**:

- [ ] Version consistency scan: all version-bearing files, tag names, artifact names, and release notes agree.
- [ ] Updater/manual-update contradiction scan: the documented manual-update policy matches `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, and `src-tauri/src/lib.rs`.
- [ ] Local-only/privacy scan: runtime transcript and cleanup paths call only loopback services; any external URL is user-initiated, documented, and non-telemetry.
- [ ] Transcript persistence scan: full transcript text is written only through intentional user-facing history or an explicitly enabled debug/beta path; diagnostic exports and bug-report uploads exclude transcript debug logs.
- [ ] Runtime model download scan: every app-initiated model download has an allowlisted host/path, a bounded destination under the models directory, cancellation cleanup, and size/hash validation appropriate to the catalog entry.
- [ ] Browser-open scan: user-facing links are opened through narrow backend commands or fixed allowlisted URLs; there is no generic arbitrary URL opener in the frontend fallback path.
- [ ] IPC/permissions scan: Tauri capabilities and commands expose only the minimum app surface needed for settings, model selection/download, diagnostics, paste, tray, and local cleanup.
- [ ] Rust risk scan: all non-test `unwrap()`, `expect()`, `unsafe`, `dbg!`, `println!`, `TODO`, and `FIXME` hits are reviewed.
- [ ] Dependency advisory scan: `npm audit --omit=dev` and `cargo audit` have been run. Any warnings accepted for the release are listed below with their scope.
- [ ] Bundle asset scan: sidecars, libraries, model resources, app icons, artifact outputs, and checksums are present and valid for the release host.
- [ ] Unsigned-beta scan: artifacts are intentionally unsigned/not notarized and release notes warn users exactly how to launch them.
- [ ] Installed-artifact scan: the packaged app passes the clean-account smoke path, including permission prompts and uninstall/data cleanup.
- [ ] Orphan-process scan: quitting the app leaves no `whisper-server` or related sidecar process behind.
- [ ] Docs-reality scan: README, privacy, build, releasing, and smoke-test docs match the exact app behavior being shipped.

If any item fails, fix or document the failure before tagging the version. A documented known issue is acceptable; an undocumented broken path is not.

### 8. Security notes for the current release

Record the outcome of each release security sweep here before tagging:

- Transcript privacy: diagnostic reports must include readiness, sanitized config, UI events, and recent non-transcript session logs only. Full transcript debug logs, if enabled for beta diagnosis, must remain local-only and must not be uploaded.
- Runtime network allowlist: expected runtime external hosts are GitHub releases for updates and Hugging Face for explicit model downloads. Cleanup/classifier traffic must remain loopback-only through the configured Ollama URL validator.
- Runtime model downloads: catalog downloads should be constrained by host/path, destination canonicalization, cancellation cleanup, and file size or SHA-256 checks. Build-time sidecar/VAD downloads are already SHA-256 pinned in scripts.
- macOS entitlements: `audio-input`, Apple Events automation, WebView JIT, unsigned executable memory, and disabled library validation are accepted for the current architecture because the app captures microphone audio, pastes through System Events, and bundles ML/WebView sidecars. Do not add new entitlements without a release-note justification.
- Updater posture: Tauri updater metadata must remain signed with the configured public key. Manual release-page links should be fixed/allowlisted, not arbitrary URL opens.
- Dependency audit: `npm audit --omit=dev` should be clean. `cargo audit` warnings from Tauri's Linux GTK/WebKit stack, `paste`, or Unicode helper crates may be accepted for a macOS-first beta only when no direct vulnerable code path is introduced and the warning is documented in the release notes/checklist.

---

## Deferred until first public release

These matter for a public release. They do **not** matter for sharing v0.X with peers and business colleagues, and adding them now would burn time without a payoff that the audience cares about. Captured here so the next maintainer doesn't have to rediscover them.

### Code signing and notarization

- macOS: Developer ID + notarization. Scaffolding already exists per `BUILD.md`; not executed.
- Windows: Authenticode signing certificate. SmartScreen warning on first launch is the visible cost of skipping this.
- Decision needed before public release. For peer-share, Gatekeeper / SmartScreen warnings are acceptable and documented in `RELEASING.md`.

### Supply-chain hygiene

- Pinned dependency versions across `Cargo.toml` and `package.json`.
- Reproducible-build verification (a second build host produces byte-identical artifacts).
- Sigstore / cosign signatures on release artifacts so a downloader can verify provenance independently of GitHub.
- See `Business-OS/SECURITY.md` for the portfolio-wide posture this rolls into.

### Brand resolution

- TurboTalk is currently logged in `Business-OS/CLAUDE.md` as a personal utility, Tier 1, *not* a Libre product unless promoted.
- Public release implies promotion. Decision needed: does it ship under the Libre brand (and pick up the Libre HIG, certified-badge story, and release-page template), or as an Eldo personal utility that happens to be open source?
- Both are valid; they imply different release pages, different press posture, and different ongoing maintenance commitments.

### LibreWin Certified badge

- Only relevant if TurboTalk releases under the Libre brand.
- Certification criteria are in `Business-OS/standards/LIBRE-SOFTWARE-STANDARDS.md`.
- Reference apps (Shelf, Stack, Prism, Fade) carry more weight than the written HIG; TurboTalk would join that tier if certified.

### Public-facing surface

- Marketing site or product page beyond the GitHub README.
- Press / launch posture.
- Issue templates and contribution guide for a public audience (the current `.github/` is fine for peer-share).
- Telemetry-free crash reporting path (today the answer is "user files an issue with the panic message" — fine for peers, insufficient at scale).

---

## When to revisit this document

- When TurboTalk is promoted from peer-share to public release.
- When a new portfolio-wide standard lands in `Business-OS/standards/STACK-CHOICES.md` that has scope to TurboTalk.
- When any item in the [Deferred](#deferred-until-first-public-release) section is actually executed (move it out of the deferred section into the live checklist with the version that landed it).
