# TASK-34: Modes tab Advanced panel — guided Ollama setup UI

## Goal
The Advanced (Chaperone) panel of the Modes tab now contains a guided setup section at the top. The section reflects one of three live states: (1) Ollama not reachable — shows an "Install Ollama" button that opens https://ollama.com/download in the user's default browser; (2) Reachable but the configured classifier model is not pulled — shows a "Download classifier model" button that triggers a streaming pull with a percentage + progress bar styled like the existing whisper model rows; (3) Both gates pass — shows a single "Ready" pill in the section header. Detection re-runs on tab-switch into Modes, on window focus, and on a 5-second timer that runs only while the Advanced panel is mounted.

## Context
This is the user-visible payoff of the guided setup work. The two preceding tasks deliver the backend plumbing this UI consumes:

- `commands.pingOllama()` → `{ reachable: boolean, version: string | null }`
- `commands.checkOllamaModel(modelName)` → `boolean` (typed-error wrapped)
- `commands.openUrl(url)` → typed-error wrapped, allowlisted to `*.ollama.com`
- `commands.pullOllamaModel(modelName)` → typed-error wrapped, long-running; emits `ollama-pull-progress` events with `{ model: string, pct: number, status: string }` payloads

Both tasks must be merged before this one is started. If either is missing from `src/bindings.ts`, stop and ask the user to run TASK-32/TASK-33 first.

The Advanced panel lives in `src/App.svelte` inside the `{#if activeTab === 'modes'}` block, gated by `{#if isAdv}` (where `isAdv = cfgCleanupMode === 'chaperone'`). Search for `adv-panel-in` and `Ollama URL` to find the right anchor. The new setup section should sit at the **top** of the right (Advanced) column, above the current "Ollama URL" / "Classifier model" / "Classifier prompt" inputs — those remain editable for power users; the guided UI just makes the common case ("install + pull default model") one click each.

The whisper model row pattern is the visual reference. See the `modelRow` snippet and the `RECOMMENDED_MODEL` block in `src/App.svelte`. Reuse the same look: model name + size + description on the left, a single action button on the right (Download / Selected / Use), with a percentage and a disabled state during in-flight downloads. The Ollama setup row only ever has one action at a time, so the layout is simpler.

State-machine outline for the Ollama setup section:

| Reachable | Model present | Pull in flight | Render |
|-----------|---------------|----------------|--------|
| false     | —             | —              | "Ollama not detected" + "Install Ollama" button |
| true      | false         | false          | "Ollama reachable · classifier model missing" + "Download classifier model (~2GB)" button |
| true      | false         | true           | Same row, button disabled, percentage + progress bar |
| true      | true          | —              | "Ready" pill in section header, no row body |

The header should always show the section label ("Setup") plus the green "Ready" pill in state 4, nothing in the others.

Polling lifecycle:
- On `switchTab('modes')` and `cfgCleanupMode === 'chaperone'` becoming true, run a single detection pass immediately.
- On `window.focus`, re-run detection.
- While the Advanced panel is mounted (use `onMount` in a child component, or an `$effect` that watches `isAdv`), set up a 5s `setInterval` that runs detection. Tear it down when the panel unmounts or `isAdv` becomes false. **Do not poll while the panel is hidden.** The interval is a soft heartbeat, not a hard requirement — focus + tab-switch usually catch real state changes faster.
- During an in-flight pull, suspend polling for `check_ollama_model` (the answer is "false until pull finishes" and we already know that). Resume when the pull resolves.

The pull listener is an event subscription, not a poll. Mirror the existing whisper download pattern: `listen('ollama-pull-progress', (e) => { ... })` inside `onMount`, push the cleanup into the `unlisteners` array. Track in-flight pulls with a small reactive object `ollamaPullState` that holds `{ inFlight: bool, pct: number, status: string }`. On `commands.pullOllamaModel` resolution, flip `inFlight` off and re-run detection so the UI moves to the "Ready" state on success.

The configured model name comes from `cfgLlmModel` (already loaded by `openModes()`); pass it to `checkOllamaModel` and `pullOllamaModel`. Empty string → fall back to `'llama3.2:3b'` (the default in `src-tauri/src/settings.rs`).

The "Install Ollama" button calls `commands.openUrl('https://ollama.com/download')`. On error (allowlist or process spawn failure), surface a `transcriptError`-style banner or push to `uiErrors` — pick whichever the existing patterns prefer. Don't silently swallow.

The download button must reflect the typed-error contract: `commands.pullOllamaModel(model)` returns `{ status: "ok", data: null } | { status: "error", error: string }`. On error, push to `uiErrors` with a clear message; the user can then retry.

Polling work should NOT block paint. Each detection call is two HTTP requests with a 2s timeout — sequence them with `Promise.all([pingOllama(), checkOllamaModel(model)])` and only update reactive state once both resolve.

The auto-fit window-sizing logic from `settingsTabH`/`modelsTabH` already includes the Modes-Chaperone height (`settingsH`). The new section will make the Chaperone panel taller, so the existing measurement at mount needs to capture it. Re-running mount measurement is out of scope — just confirm the section renders inside the existing `flex-1 overflow-y-auto` container (line where `adv-panel-in` lives) so it scrolls if the user's window happens to be too short. The two-column layout's right column is already scrollable.

## In scope
- `src/App.svelte` only — the new section, the polling effect, the event listener, and the new reactive state vars
- Reusing existing UI components (`SectionLabel` etc.) and the whisper-model-row visual idiom

## Out of scope
- Any backend changes (TASK-32 / TASK-33 own those)
- Modifying the existing Ollama URL / Classifier model / Classifier prompt input rows — leave them as-is below the new section
- Cancellation of an in-flight pull
- A "Test connection" button — the live polling makes a manual button redundant
- A separate "Other models" picker — the user can still type any model name into the existing Classifier model input; this guided flow optimizes only for the default
- The silent-fallback `ui-error` toast (TASK-35)
- Window-size remeasurement on Chaperone-section growth — out-of-scope; the panel is already scrollable

## Steps
1. Verify `commands.pingOllama`, `commands.checkOllamaModel`, `commands.openUrl`, and `commands.pullOllamaModel` all appear in `src/bindings.ts`. If any are missing, stop and tell the user to land TASK-32/TASK-33 first.
2. Read the existing `modelRow` snippet and the `RECOMMENDED_MODEL` block in `src/App.svelte` to internalize the visual idiom (label + size + description + right-aligned button + percentage + selected-pill).
3. Add reactive state near the other Modes-tab state (around the `cfgCleanupMode` declarations):
   - `let ollamaReachable = $state(null);` (null until first probe)
   - `let ollamaModelPresent = $state(null);`
   - `let ollamaPullState = $state({ inFlight: false, pct: 0, status: '' });`
4. Add an async helper `refreshOllamaSetup()` that does:
   ```
   const ping = await commands.pingOllama();
   ollamaReachable = ping.reachable;
   if (ping.reachable) {
     const model = cfgLlmModel || 'llama3.2:3b';
     ollamaModelPresent = (await commands.checkOllamaModel(model)).status === 'ok'
       ? (await commands.checkOllamaModel(model)).data
       : false;
   } else {
     ollamaModelPresent = null;
   }
   ```
   (Sketch only — implement the typed-error unwrap correctly; don't double-call `checkOllamaModel`.)
5. Add an `$effect` that activates when `activeTab === 'modes' && isAdv`:
   - Calls `refreshOllamaSetup()` immediately
   - Sets a 5s `setInterval` calling `refreshOllamaSetup()` (skip if `ollamaPullState.inFlight`)
   - Adds a `window.addEventListener('focus', refreshOllamaSetup)`
   - Returns a cleanup that clears the interval and removes the listener
6. In `onMount`, attach `listen('ollama-pull-progress', (e) => { ... })` updating `ollamaPullState`. Push the unlisten into `unlisteners`.
7. Add the UI block at the top of the right column inside `{#if isAdv}`. Structure:
   ```
   <div class="space-y-1">
     <div class="flex items-center justify-between">
       <SectionLabel>Setup</SectionLabel>
       {#if ollamaReachable && ollamaModelPresent}
         <span class="text-[10px] uppercase tracking-wider font-semibold text-green-400">Ready</span>
       {/if}
     </div>
     <!-- one of three rows depending on state -->
   </div>
   ```
   Render the not-reachable row, the model-missing row, and (during pull) the progress row using the same visual palette as the whisper recommended block — left side text + small description, right side button or pct.
8. Wire the Install Ollama button to `commands.openUrl('https://ollama.com/download')`. On `status === 'error'`, push to `uiErrors`.
9. Wire the Download button to a function that:
   - Sets `ollamaPullState = { inFlight: true, pct: 0, status: 'starting…' }`
   - `await commands.pullOllamaModel(cfgLlmModel || 'llama3.2:3b')`
   - On Ok: leave `inFlight=false`, call `refreshOllamaSetup()` to flip to Ready
   - On Err: push to `uiErrors`, set `inFlight=false`, leave the row showing the missing-model state
10. Run `npm run tauri dev` and walk through the three states by hand:
    - Stop Ollama → reload the app → switch to Modes → click Advanced. The "Install Ollama" button should be visible and clicking it should open the browser.
    - Start Ollama, wait for the 5s poll (or window focus) — the row should switch to "model missing".
    - Click Download — progress bar should advance from 0 → 100 with status changing through "pulling manifest", layer hashes, "verifying…", "writing manifest", "success". The section header should flip to "Ready".
11. Update `SESSION-STATUS.md` with one line.

## Success signal
- With Ollama not running, the Modes → Advanced panel shows "Ollama not detected" with an "Install Ollama" button. Clicking it opens https://ollama.com/download in the system browser.
- With Ollama running but the configured classifier model not pulled, the panel shows "Ollama reachable · classifier model missing" with a "Download classifier model" button. The button shows percentage and a progress bar during pull. On completion, the section header flips to a green "Ready" pill.
- The 5-second polling interval is observable in the Network tab as exactly two GET requests every 5 seconds while the Advanced panel is open, and zero requests while it's closed.
- Window-focus regains trigger one extra detection pass.

## Notes
- Don't add any business logic to the existing "Ollama URL" / "Classifier model" / "Classifier prompt" inputs. The guided UI sits above them and reads `cfgLlmModel` for "which model to pull" — if the user changes that field, the next 5s tick re-probes for the new model name.
- The IPC overhead for two `2s`-timeout HTTP calls every 5s on loopback is negligible. Don't over-engineer caching.
- Keep the wording compact and lowercase: "ollama not detected", "classifier model missing", "ready" — match the rest of the Modes tab voice.
- If `cfgLlmModel` is empty when the user opens the panel, fall back to `'llama3.2:3b'` for the probe — but don't write that fallback into the config, so empty stays empty until they explicitly set it.
- The download button should be `disabled` during `ollamaPullState.inFlight` to prevent double-clicks.
