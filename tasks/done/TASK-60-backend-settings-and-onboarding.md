# TASK-60: Backend selector in Settings and onboarding

## Goal
A user can pick a transcription backend family (Whisper / Moonshine / Parakeet) from the Settings tab. The choice persists, the worker is rebuilt on change, and the new family's models appear in the model picker. The Chaperone guided-setup onboarding flow asks the user which family to use on first run, downloads the appropriate model, and lands the user in a working dictation state. The hardcoded `TT_BACKEND` env-var selector from TASK-58/59 is removed.

## Context
TurboTalk is a personal-use macOS push-to-talk dictation utility. Tier 1 product. Read `CLAUDE.md` at repo root and `../../Business-OS/standards/Engineering.md` before starting.

This task assumes TASK-57, TASK-58, and TASK-59 have all landed. Three backend impls exist behind the `TranscriptionBackend` trait, selectable via an env var. This task replaces the env-var hack with proper UX: a setting that persists, a settings UI to change it, and an onboarding step that walks a new user through the choice.

Key UX principle: each backend family has a different model catalog. Whisper uses ggml files; Moonshine and Parakeet use ONNX bundles. The model picker in Settings must filter to the current family's models, and the onboarding download must pick the right kind of model for the chosen family.

Settings persistence at `~/.config/librewin/turbotalk/config.toml`. Settings tab UI at `src/` (search for the Settings tab component). Onboarding flow at `src/Onboarding.svelte` (the file referenced in earlier conversation context — the existing Whisper model picker lives there around line 50).

## In scope
- `src-tauri/src/settings.rs` — add `backend: BackendFamily` enum field to config (variants: Whisper, Moonshine, Parakeet); default Whisper
- `src-tauri/src/transcribe.rs` — `build_backend` now reads `cfg.backend` instead of `TT_BACKEND`. Remove the env-var read.
- Worker invalidation on backend change — extend the existing `invalidate_worker` trigger in `settings::save` so a family swap rebuilds the worker, not just a model swap
- Model catalog awareness — each family knows which models it can use. Suggest `models_for_family(family) -> Vec<ModelEntry>` returning entries with `{ id, label, family, size_bytes, download_url, on_disk_path }`. Catalog can be a const table.
- Settings tab in `src/` — backend picker (radio or dropdown) + a model picker that filters to the chosen family
- `src/Onboarding.svelte` — extend the existing model-pick step (Onboarding.svelte:50–60) into a two-step flow: pick family, then pick model within that family. Pick sensible defaults so the user can click through quickly.
- Model download UI — extend the existing Whisper download UI to handle Moonshine and Parakeet downloads with the same progress-event pattern
- `SESSION-STATUS.md` + `TRUTH.md` — one-line each
- Remove `TT_BACKEND` env-var handling and any associated docs/code comments

## Out of scope
- New backend implementations
- Tuning, accuracy benchmarking, A/B comparison between backends — out of scope here, can come later
- Streaming/segment transcription generalization — leave Whisper-only until proven necessary
- Renaming any existing UI strings beyond what's needed for the family-aware picker
- Windows or Linux UX paths — focus on macOS arm64

## Steps
1. Read `CLAUDE.md`, `../../Business-OS/standards/Engineering.md`, `SESSION-STATUS.md`, `TRUTH.md`, `src-tauri/src/settings.rs`, `src-tauri/src/transcribe.rs` (post TASK-57/58/59), `src/Onboarding.svelte`, and the Settings tab component in `src/`.
2. Add `BackendFamily` enum to `settings.rs` (Whisper / Moonshine / Parakeet). Add `backend: BackendFamily` to the config struct, default Whisper. Update default-construction tests.
3. Decide model catalog shape. Suggested: a `pub const MODEL_CATALOG: &[ModelEntry]` table in a new `src-tauri/src/model_catalog.rs` (or extend `whisper_models.rs` and rename it to `model_catalog.rs`). Entries carry the family tag so the frontend can filter.
4. Expose a tauri command `list_models(family) -> Vec<ModelEntry>` for the frontend to populate the model picker.
5. Update `build_backend` to read `cfg.backend` and dispatch on the family. Delete the env-var read.
6. Extend `settings::save` (or wherever model-change invalidation already lives) so a family change also triggers `invalidate_worker`. Confirm the worker rebuilds when either family or model changes.
7. Settings tab UI: add a family picker. When the family changes, repopulate the model dropdown via `list_models(new_family)`. Persist on change. Confirm the worker is rebuilt (the existing `dictation-ready` / `dictation-ready-failed` events from `src-tauri/src/transcribe.rs:693` already fire after a rebuild — surface them in the UI as you already do for model changes).
8. Onboarding (`src/Onboarding.svelte`): extend the existing "pick a model" step. Step A: pick a family (with a one-line explainer per family — e.g., "Whisper: multilingual, slower. Moonshine: English-only, fast, less hallucination on silence. Parakeet: English-only, fastest."). Step B: pick a model within the chosen family, download it, finish onboarding.
9. Run a full reset path:
   - `npm run package`, install the resulting DMG, OR
   - `cargo run -- --reset` (or whatever the existing reset flow is — see `src/Onboarding.svelte` and `src-tauri/src/permissions.rs`)
   - Walk through onboarding selecting **Moonshine**. Confirm the right model downloads (an ONNX bundle, not a ggml file). Finish onboarding. Hold PTT, say "hello world", confirm "hello world" pastes.
10. Repeat for Parakeet — reset, onboarding → Parakeet, dictate "hello world".
11. Repeat for Whisper — reset, onboarding → Whisper, dictate "hello world". (Regression: existing flow unaffected.)
12. From a populated install, switch backend in Settings from Whisper to Moonshine. Confirm the model picker changes its contents. Confirm dictation works on the new family without restarting the app.
13. `grep -r "TT_BACKEND" .` returns no hits in src-tauri/ or src/. Any references in code comments must also be removed.
14. `cargo test` exits 0.
15. Update `SESSION-STATUS.md` and `TRUTH.md`.
16. Commit with `feat(settings): backend family selector in Settings and onboarding`.

## Success signal
- Fresh install → onboarding → pick Moonshine → "hello world" PTT pastes "hello world". Works.
- Fresh install → onboarding → pick Parakeet → "hello world" PTT pastes "hello world". Works.
- Fresh install → onboarding → pick Whisper → "hello world" PTT pastes "hello world". Works.
- Mid-session backend swap in Settings: switching family rebuilds the worker, the model picker shows only the new family's models, dictation works on the new family without an app restart.
- `grep -r "TT_BACKEND" src-tauri/ src/` → no hits.
- `cargo test` exits 0.

## Notes
- The TASK-55 hallucination filter and TASK-56 VAD pre-filter only apply on the Whisper path. For Moonshine and Parakeet, the filter can still run (no harm — clean transcripts will pass the gate) but VAD is whisper-server-specific and should be skipped silently. Document this in code where the branching happens.
- The Chaperone Layer (`src-tauri/src/cleanup.rs`) runs on the output of all three backends — it's family-agnostic. Confirm by inspection that the cleanup pipeline doesn't accidentally hardcode anything Whisper-specific.
- Onboarding "explain the choice" text matters. Aim for one short sentence per family. Don't overwhelm a first-time user — most should just pick the recommended default and move on. The default in the picker should still be Whisper (most accurate, broadest), with Moonshine and Parakeet presented as alternatives.
- If users report buyer's-remorse switching ("I picked Moonshine but want Whisper now"), the Settings family picker is the answer — make sure that path is discoverable, not buried.
- This is the last Phase 3 task. After it lands, the env-var hack is gone, the three backends are first-class, and the original hallucination question raised by the user has been addressed at four levels: post-hoc detection (TASK-55), VAD pre-filter (TASK-56), trait abstraction (TASK-57), and two architecturally non-hallucinating alternative backends (TASK-58, TASK-59).
