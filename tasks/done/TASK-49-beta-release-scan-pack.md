# TASK-49: Beta release scan pack orchestration

## Goal

Run a comprehensive beta-release scan for TurboTalk before the next beta tag. The lead agent should dispatch focused subagents, collect their findings, resolve contradictions, and synthesize a single release-readiness report with clear pass/fail status, blockers, recommended fixes, and documented known issues.

This task is intentionally orchestration-heavy. The lead agent should not try to personally inspect every surface serially. It should split the work into bounded subagent investigations, let those run in parallel where possible, then verify and merge their conclusions.

## Context

TurboTalk is a Svelte + Tauri desktop dictation app. Current beta release procedure lives in:

- `docs/RELEASING.md` — mechanical release process.
- `docs/RELEASE-READINESS.md` — peer-share release gate.
- `docs/SMOKE-TEST.md` — manual runtime and packaged-artifact smoke tests.
- `docs/PRIVACY.md` — local-only and data-handling claims.
- `docs/BUILD.md` — build/package procedure.

The existing release gate already requires a clean tree, Rust tests, clippy, typecheck/build checks, packaged smoke testing, artifact checksums, and no orphaned sidecars. The new scan pack adds explicit checks for beta-specific release risks:

- Version consistency across files, tags, artifacts, and notes.
- Manual-update policy vs updater code/config/dependencies.
- Local-only/privacy claims vs actual network surface.
- Tauri IPC/capability exposure.
- Rust risk patterns in user-controlled paths.
- Bundle asset completeness and executable/resource validity.
- Unsigned/ad-hoc beta packaging state.
- Installed-artifact behavior on a clean macOS account/VM.
- Orphaned sidecar process cleanup.
- Documentation matching the app that is actually shipping.

Important current observation: `docs/RELEASING.md` says TurboTalk beta uses manual updates only and the updater plugin is intentionally not enabled. The codebase currently has updater dependencies/config and registers `tauri_plugin_updater` in `src-tauri/src/lib.rs`. This may be inert if no UI/runtime path invokes it, but it is a release-readiness contradiction until it is explained, removed, or documented.

## Lead agent requirements

The lead agent must dispatch subagents and synthesize their results. Do not treat subagent output as automatically final; resolve conflicts, spot-check high-risk claims, and produce a final report with evidence.

Use at least these subagents:

1. **Release procedure and versioning subagent**
   - Owns `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, release docs, artifact naming, and version/tag consistency.
   - Checks that release procedure, build procedure, and readiness gate agree.
   - Specifically investigates updater/manual-update consistency.

2. **Privacy and network surface subagent**
   - Owns URL/network search across frontend, Rust backend, configs, and docs.
   - Builds an allowlist of expected external hosts and loopback calls.
   - Confirms transcript/cleanup runtime traffic stays localhost-only.
   - Flags any telemetry-like dependency or external runtime call.

3. **IPC, permissions, and Rust risk subagent**
   - Owns Tauri capabilities, exported commands, URL opening, file/model path handling, paste injection, diagnostics, cleanup, and unsafe/panic-prone patterns.
   - Reviews non-test `unwrap()`, `expect()`, `unsafe`, `dbg!`, `println!`, `TODO`, and `FIXME` hits.
   - Distinguishes acceptable invariant unwraps from user-controlled crash paths.

4. **Packaging and smoke-test subagent**
   - Owns sidecars, dynamic libraries, VAD resource, app icons, artifact outputs, checksums, unsigned/ad-hoc packaging checks, installed-artifact smoke requirements, and orphan-process verification.
   - Reviews `scripts/preflight.mjs`, `scripts/rename-artifact.mjs`, Tauri bundle config, and `docs/SMOKE-TEST.md`.
   - Recommends concrete preflight extensions if needed.

5. **Docs-reality subagent**
   - Owns README, privacy, build, releasing, smoke-test, changelog/session status, and any known-limitation docs.
   - Compares documented platform support, hotkeys, model setup, cleanup modes, manual update behavior, signing warnings, and privacy claims against code/config findings from the other subagents.

If the environment supports more parallel work, split the Rust risk scan from IPC/permissions and split installed-artifact smoke from packaging. If subagents cannot be dispatched, the lead agent must still follow this same division of labor and clearly say that no actual subagents were available.

## In scope

- Documentation updates to release/readiness/smoke/build/privacy docs if contradictions are found.
- Small release-safety script updates, especially `scripts/preflight.mjs`, if they are low-risk and directly support the scan pack.
- Adding a release-readiness report artifact under `docs/` or `tasks/` if useful.
- Updating `SESSION-STATUS.md` only if a concrete release-readiness milestone is completed.
- Filing or drafting follow-up task docs for blockers that are too large to fix inside this scan task.

## Out of scope

- Implementing major product features.
- Signing or notarizing the beta.
- Enabling auto-update.
- Replacing the release process with CI unless the user explicitly asks.
- Changing platform support commitments without documenting the decision.
- Publishing the release, tagging, pushing, or creating GitHub releases unless separately requested.

## Suggested commands and searches

Run the project-native gates first:

```bash
npm run preflight
npm run typecheck
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

Use targeted scans:

```bash
rg -n "updater|plugin_updater|createUpdaterArtifacts|latest.json|manual update|manual updates" package.json src-tauri docs src
rg -n "http://|https://|reqwest|TcpStream|UdpSocket|WebSocket|fetch\\(|EventSource|localhost|127\\.0\\.0\\.1" src src-tauri common-js package.json src-tauri/Cargo.toml docs
rg -n "dbg!|println!|eprintln!|TODO|FIXME|unwrap\\(|expect\\(|unsafe" src-tauri/src src -g '*.rs' -g '*.svelte' -g '*.js' -g '*.ts'
rg -n "invoke\\(|commands\\.|openUrl|open_url|dialog|fs|shell|process|clipboard|paste|diagnostics" src src-tauri/src src-tauri/capabilities
rg -n "/Users/|C:\\\\Users\\\\|eldo|\\.env|SECRET|TOKEN|API[_-]?KEY" . -g '!node_modules/**' -g '!target/**' -g '!dist/**'
```

When artifacts exist, verify:

```bash
shasum -a 256 -c dist-artifacts/*.sha256
codesign -dv dist-artifacts/TurboTalk-<version>-macos-arm64.dmg
pgrep -fl "whisper-server|whisper-cli"
```

Only run platform-specific installed-artifact smoke tests on a clean macOS user account or VM as described in `docs/SMOKE-TEST.md`.

## Steps

1. Read `docs/RELEASING.md`, `docs/RELEASE-READINESS.md`, `docs/SMOKE-TEST.md`, `docs/PRIVACY.md`, `docs/BUILD.md`, `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, and `src-tauri/src/lib.rs`.
2. Dispatch the five subagents listed above. Give each subagent concrete ownership, expected evidence, and an output format:
   - `status`: pass / warning / blocker / not-run.
   - `findings`: ordered by severity with file references.
   - `commands run`: include important pass/fail results.
   - `recommended fixes`: concrete and scoped.
   - `open questions`: only where a decision is genuinely needed.
3. While subagents run, the lead agent should run the top-level gates it can run locally without blocking on manual smoke tests.
4. Gather all subagent reports. Deduplicate findings and resolve conflicts. If two reports disagree, inspect the source files directly and decide.
5. Classify each scan-pack item as:
   - **Pass** — no action needed.
   - **Fixed** — issue found and corrected in this task.
   - **Documented known issue** — acceptable for beta, explicitly documented.
   - **Blocker** — should stop the beta until fixed.
   - **Not run** — requires a specific host/account/artifact not available in this session.
6. Fix small documentation or script gaps immediately if the fix is obvious and low-risk. For anything larger, write a follow-up task doc instead of silently leaving it vague.
7. Produce a final release-readiness report. Include:
   - Overall recommendation: ship / do not ship / ship only after listed fixes.
   - Blockers first.
   - Warnings and known issues second.
   - Passed checks third.
   - Not-run manual checks last.
   - Exact files changed.
8. If code/docs were edited, run the relevant verification again.

## Success signal

- The lead agent used subagents, synthesized their results, and produced one coherent release-readiness decision.
- Every scan-pack item in `docs/RELEASE-READINESS.md` has a status.
- Updater/manual-update consistency is either fixed or explicitly documented.
- Any privacy/network concern is backed by a concrete code reference and host allowlist.
- Any IPC/permission/Rust-risk concern is tied to a file/line and user-impact explanation.
- Packaging checks identify exactly what was verified locally and what still requires a clean macOS account/VM.
- Docs accurately describe the shipping beta state.
- No beta blocker remains undocumented.

## Notes

- A documented beta limitation is acceptable. An undocumented contradiction is not.
- Do not remove updater code merely because it looks suspicious. First determine whether it is reachable and whether release policy wants it removed, disabled, or documented.
- Do not broaden the privacy claim to fit the code. Prefer tightening the code or documenting the narrow exception.
- Do not hand-wave installed-artifact smoke. If no clean macOS account/VM is available, mark it **Not run** and keep it as a release gate.
- Keep follow-up tasks narrow. A good follow-up has one owner, one behavioral goal, and a clear success signal.
