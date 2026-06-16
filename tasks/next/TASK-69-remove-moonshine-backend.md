# TASK-69: Remove Moonshine and keep Whisper + Parakeet

## Goal
Remove Moonshine from the supported backend set so TurboTalk ships with two transcription choices only: Whisper and Parakeet.

## Why
The current three-backend setup is more maintenance than the product needs. Two options are enough to cover the useful tradeoff:
- Whisper for multilingual / accuracy-first use
- Parakeet for speed-first use

This keeps the backend menu useful without forcing extra complexity or risking someone getting stuck on the wrong hardware with no fallback.

## In scope
- `src-tauri/src/transcribe.rs`
- `src-tauri/src/transcribe_backends/`
- `src-tauri/src/settings.rs`
- `src/App.svelte`
- any UI text, backend catalogs, feature flags, docs, or tests that directly reference Moonshine
- `SESSION-STATUS.md`
- `TRUTH.md` if the documented backend tradeoffs change

## Out of scope
- Reworking the transcription pipeline beyond the backend removal
- Changing the Parakeet or Whisper implementations themselves unless a small cleanup is required to remove Moonshine-specific branching
- Hotkey refactoring
- App-wide UI splitting

## Steps
1. Remove Moonshine from the user-visible backend list.
2. Remove Moonshine-specific feature flags, catalog entries, and settings branches.
3. Keep Whisper and Parakeet working as the only supported choices.
4. Clean up backend selection code so it reads like a two-way choice, not a generic plugin registry.
5. Update docs and status ledgers so the supported matrix is accurate.

## Success signal
- The app offers only Whisper and Parakeet.
- There are no lingering Moonshine-only UI branches or settings paths.
- End-to-end dictation still works with the remaining two backends.
