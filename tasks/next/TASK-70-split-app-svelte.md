# TASK-70: Split the monolithic `App.svelte`

## Goal
Break `src/App.svelte` into smaller, focused UI modules without changing the user-visible behavior.

## Why
`App.svelte` has become a full dashboard. It is doing too much at once: onboarding, history, models, modes, settings, diagnostics, update handling, and event wiring. The goal is not new features; it is making the UI easier to understand and safer to modify.

## In scope
- `src/App.svelte`
- new Svelte components extracted from it
- any small shared UI helpers needed for the split
- `SESSION-STATUS.md`

## Out of scope
- Visual redesign
- Changing the app’s behavior or workflow
- Backend logic changes
- Moonshine removal
- Hotkey refactoring

## Suggested split
Start by separating the largest responsibilities into focused components:
- onboarding / readiness
- history
- models
- modes and settings
- diagnostics / updates / developer tools

## Steps
1. Identify the stable seams already present in the file.
2. Extract the largest self-contained sections first.
3. Keep the event handling and state ownership clear so the top-level file becomes orchestration rather than a wall of logic.
4. Preserve the current interactions and visual layout.
5. Update `SESSION-STATUS.md` once the split lands.

## Success signal
- `App.svelte` is materially smaller and easier to scan.
- The app still behaves the same from the user’s point of view.
- The extracted components have clear ownership and minimal cross-talk.
