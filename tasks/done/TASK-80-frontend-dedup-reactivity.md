# TASK-80: Frontend dedup + reactivity fixes + save debounce

## Goal
Extract duplicated TypeScript constants/helpers into shared modules, fix Svelte 5 reactivity issues (rebuilt prop objects, redundant IPC round-trips), and debounce hot-path frontend operations (window resize, readiness checks, tab-switch re-fetches).

## Context
This task addresses the frontend audit findings. It depends on TASK-70 (App.svelte split) having landed — if App.svelte is still monolithic, the reactivity fixes are harder to apply. If TASK-70 has NOT landed yet, this task should still be runnable by fixing the issues in-place in App.svelte, then they carry forward when the split happens.

The findings:

**Duplication:**
- `PROMPT_PRESETS` + `DEFAULT_CLASSIFIER_PROMPT` duplicated verbatim in `ModesTab.svelte:4-44` and `App.svelte:286-339`. Any prompt edit must be made in two places.
- `KNOWN_FILENAMES` duplicated in `ModelsTab.svelte:32` and `App.svelte:782-786`.
- `altModelVariant` / `altModelActive` helpers duplicated in `ModelsTab.svelte:36-42` and `App.svelte:491-497`.
- `seg` helper duplicated in `ModesTab.svelte:46-52`, `App.svelte:427-433`, `ModelsTab.svelte:48-54`.
- Model catalog hardcoded in both TS (`ModelsTab.svelte:7-30`) and Rust (`lib.rs:1148-1183`).

**Reactivity:**
- `App.svelte:1047-1150` — tab wrappers receive props via `historyState()` / `modelsState()` / etc. which construct fresh object literals on every render. Each `{ ...state }` is a new reference, so child components always re-render.
- `App.svelte:862, 1003, 814` — save handlers do `await commands.getConfig()` before every `saveConfig()`. The local `cfg*` state already mirrors the backend — the getConfig is redundant.
- `App.svelte:728, 848, 969, 1152-1157` — tab switches call `commands.getConfig()` on every switch. Redundant IPC.
- `App.svelte:674-680` — `trackWindowHeight` not debounced. Does 2 IPC calls + localStorage write on every resize tick (can fire at 60 Hz during drag).
- `App.svelte:116, 1396-1401` — `recheckReadiness` not debounced. Called on every `focus` event + onboarding poll. Each does `checkReadiness` + possibly `listModelsForFamily` (filesystem scan).
- `Overlay.svelte:304-316` — `cursorTimer` polls `cursorPosition()` at 10 Hz via IPC for hover detection. Should be a `mousemove` listener.
- `Overlay.svelte:537` — `levels` array rebuilt 20×/sec (`levels = [...levels.slice(1), v]`). Should be a ring buffer index.

**setTimeout cleanup:**
- `App.svelte:1222, 1253, 1261, 1269, 1278, 1318, 1326` — ~7 `setTimeout` calls in `applyBackendEvent` mutate `$state` after a delay. None tracked for cleanup.

## In scope
- `src/App.svelte` (or its split components if TASK-70 has landed)
- `src/ModesTab.svelte`
- `src/ModelsTab.svelte`
- `src/Overlay.svelte`
- `src/Onboarding.svelte`
- New shared modules: `src/lib/prompts.ts`, `src/lib/catalog.ts`, `src/lib/utils.ts` (or similar)
- `SESSION-STATUS.md`

## Out of scope
- The App.svelte split itself (TASK-70)
- Backend changes
- Visual redesign
- Changing what data the frontend needs (only how it gets it)
- The model-catalog-from-backend sync (Rust↔TS duplication) — that's a larger architectural change; this task extracts the TS-side duplication into a shared module but doesn't change the source-of-truth to be the backend

## Steps
1. Create `src/lib/prompts.ts` (or `src/shared/prompts.ts` — match the existing project structure) and move `PROMPT_PRESETS` + `DEFAULT_CLASSIFIER_PROMPT` there. Import in both `ModesTab.svelte` and `App.svelte` (or wherever they live post-TASK-70).
2. Create `src/lib/catalog.ts` and move `KNOWN_FILENAMES`, `altModelVariant`, `altModelActive`, and the model catalog constants there. Import in both `ModelsTab.svelte` and `App.svelte`.
3. Create `src/lib/utils.ts` (or add to an existing utils file) and move the `seg` helper there. Import in all three components.
4. Fix the reactivity issue: instead of `historyState()` / `modelsState()` / etc. constructing fresh objects, pass individual `$state`/`$derived` values as props directly. Svelte 5 tracks dependencies at the property level — passing `history` and `historyOpen` as separate props is better than passing `{ history, historyOpen }` as one object.
5. Fix the save handlers: remove the `await commands.getConfig()` call before `saveConfig()`. Construct the config object from the local `cfg*` state directly. The local state IS the source of truth for the frontend; the backend save just persists it.
6. Fix the tab-switch re-fetch: remove `commands.getConfig()` from `openModels` / `openModes` / `openSettings`. The local state is already current (it was just saved or loaded on mount). Only fetch on initial mount.
7. Debounce `trackWindowHeight` (`:674-680`): wrap in a 150ms debounce. Use a simple `setTimeout`-based debounce or a tiny utility. Cancel on unmount.
8. Debounce `recheckReadiness` (`:116, 1396-1401`): wrap in a 250ms debounce. Called on focus events which can fire rapidly.
9. Replace the Overlay `cursorPosition()` 10 Hz polling (`:304-316`) with a `mousemove` event listener on the window/document. Check if the cursor is inside the pill's hover zone from the event's `clientX`/`clientY` + `getBoundingClientRect()`. Remove the `setInterval`.
10. Replace the `levels` array rebuild (`:537`) with a ring buffer: `levels[head] = v; head = (head + 1) % levels.length;` and render by iterating from `head`. No allocation per frame.
11. Track the `setTimeout` calls in `applyBackendEvent` (`:1222` etc.): store IDs in a `Set<number>`, clear them all in the `onMount` cleanup return. This prevents orphaned state mutations after unmount (during hot-reload or future component splits).
12. Run `npm run typecheck` and `npm run build` (vite build).
13. Update `SESSION-STATUS.md`.

## Success signal
- `npm run typecheck` passes.
- `npm run build` (vite) passes with no errors.
- `grep -rn "PROMPT_PRESETS" src/` shows exactly one definition (in the shared module), imported by both `ModesTab.svelte` and `App.svelte`.
- `grep -rn "KNOWN_FILENAMES" src/` shows exactly one definition.
- `grep -rn "const seg" src/` shows exactly one definition.
- Tab components receive individual props (e.g. `history={history}` `historyOpen={historyOpen}`), not rebuilt object literals.
- `commands.getConfig()` is called on mount only, not on every tab switch or save.
- `trackWindowHeight` is debounced — during a window resize drag, localStorage write happens at most once per 150ms, not on every resize tick.
- Overlay hover detection uses `mousemove`, not `setInterval` + `cursorPosition()` IPC.
- `levels` array is a ring buffer (no `[...levels.slice(1), v]` allocation per frame).

## Notes
- This task is most effective if TASK-70 (App.svelte split) has already landed, because the reactivity fixes apply to the split components. If TASK-70 hasn't landed, apply the fixes to the monolithic App.svelte — they'll carry forward when the split happens.
- Svelte 5 runes (`$state`, `$derived`, `$effect`) are the correct reactivity model. The codebase already uses them (verified in the audit — `Status.svelte` and `HistoryTab.svelte` use them correctly). The issue is that `App.svelte` constructs intermediate objects that break fine-grained tracking.
- For the debounce utility: a simple inline implementation is fine — `let timeout: number; const debounced = (fn: () => void, ms: number) => { clearTimeout(timeout); timeout = setTimeout(fn, ms); };`. No need for a library.
- The `mousemove` listener on the Overlay should be passive (doesn't need `preventDefault`) and should check the pill's bounding rect. If the pill is small, the check is a simple `rect.contains(x, y)`.
- The ring buffer for `levels`: the array is fixed-size (e.g. 312 elements for the waveform). Writing `levels[head] = v` and advancing `head` is O(1) with zero allocation. Rendering iterates `levels[head..].chain(levels[..head])` or uses a computed view.
- Do NOT remove the `getConfig()` call on initial mount — the frontend needs to load the initial config from the backend. Only remove the redundant re-fetches on tab switch and before save.
