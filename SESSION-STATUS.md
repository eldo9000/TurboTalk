# TurboTalk — Session Status

**Last updated:** 2026-05-01
**Current state:** Multi-agent code review hardening sprint complete. 8/8 tasks landed.
Full dictation loop intact; security and architecture findings closed.

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

Roadmap M2 — Configurable.

## Blockers

None.

## Next Session Should

1. Tray icon — hide window to menu bar, show/hide on click.
2. Basic text cleanup — capitalize first word, strip trailing whitespace.
3. Config persistence — `~/.config/librewin/turbotalk/config.toml` via `settings.rs`.
4. Settings window — surface model path and hotkey to user.

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

## Recent Decisions

- **rdev → CGEventTap** — macOS 26 broke rdev (TSM `dispatch_assert_queue` crash). Direct
  `CGEventTap` via `core-graphics 0.24`. Right Option detected by keycode 0x3D only, no TSM.
- **Homebrew whisper-cpp** — not bundled as Tauri sidecar yet; hardcoded path for now.
- **ggml-base.en** — 141MB, ~130ms on M4 via Metal. Adequate for M1.
- **Window: 380×280** — no custom titlebar, native macOS traffic lights only.
- **Reference, not fork** — built from scratch. Handy/typr/sagascript as references.
