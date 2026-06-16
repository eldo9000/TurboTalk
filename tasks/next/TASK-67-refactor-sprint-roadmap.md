# TASK-67: Refactor sprint roadmap - hotkey, backend trim, App split

## Goal
Carry out the refactor sprint in a controlled order:
1. separate hold and toggle hotkey logic,
2. remove Moonshine so only Whisper + Parakeet remain,
3. split `App.svelte` into focused UI modules.

## Why
TurboTalk is working well, but the current hotkey lifecycle is carrying too many shared edge cases, the backend lineup has more maintenance weight than needed, and `App.svelte` has become a monolith. This sprint keeps the product fast in use while reducing the amount of logic any one file has to own.

## Order
1. `TASK-68-hotkey-toggle-controller.md`
2. `TASK-69-remove-moonshine-backend.md`
3. `TASK-70-split-app-svelte.md`

## Shared rules
- Keep behavior stable unless the task explicitly asks to change it.
- Do not broaden the backend list beyond the two chosen options.
- Keep the dictation path fast and local-first.
- Update `SESSION-STATUS.md` as each task lands.

## Success signal
The three task files below can be executed independently in order, with clear ownership and no overlap in write scope.
