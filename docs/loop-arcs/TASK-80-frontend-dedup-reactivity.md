# Arc Log — TASK-80: Frontend dedup + reactivity fixes + save debounce

## Gate
Extract duplicated TS constants into shared modules, fix Svelte 5 reactivity issues,
debounce hot-path frontend operations, and fix Overlay performance.

## Premise declarations

### Premise 1 (initial)
- **SYMPTOM:** PROMPT_PRESETS/KNOWN_FILENAMES/seg duplicated across 3+ components.
  save handlers fetch getConfig before save (redundant). Tab switches fetch getConfig again.
  trackWindowHeight not debounced. Overlay polls cursorPosition at 10Hz via IPC.
  levels array rebuilt 20x/sec. setTimeout callbacks never cleaned up.
- **PREMISE:** Extracting shared constants to src/lib/*.ts, removing redundant IPC calls,
  debouncing resize/recheck with simple setTimeout utilities, replacing cursor polling
  with mousemove listener, and using a ring buffer for levels will fix all findings.
- **DERIVATION:** Svelte 5 reactive state is already local — getConfig before save is
  always a no-op. Cursor polling via IPC at 10Hz is wasteful when mousemove gives
  free events. [...levels.slice(1), v] allocates per audio tick.
- **FALSIFICATION:** If `npm run typecheck` or `npm run build` fails, premise is false.
- **FALSIF-RESULT:** `npm run typecheck` + `npm run build` pass. 3 shared modules created. Tab props individual. save handlers build cfg from local state. trackWindowHeight/recheckReadiness debounced. Overlay mousemove + ring buffer. setTimeout cleanup tracked.
- **DISPOSITION:** CONFIRMED — dispatch 1 green. Commit 829c9a4.
