# TASK-2: Add diagnostics copy button to settings UI

## Goal
A "Copy diagnostics" button exists in the settings panel. Clicking it calls `run_diagnostics`, formats the result as plain text, copies it to the clipboard, and shows a brief confirmation. Beta users can paste the output directly into a bug report.

## Context
TurboTalk is a Tauri 2 + Svelte 5 dictation app on macOS. The frontend is `src/App.svelte`. Settings are shown in a settings panel or section of the main UI. The `run_diagnostics` Tauri command (added in a prior task) returns a `DiagnosticsResult` object with fields: platform, mic_available, model_exists, sidecar_exists, sidecar_executable, cleanup_mode, ollama_reachable (present if cleanup_mode is "advanced"), paste_capability.

The button should be placed in the settings section, not in the main dictation view. Placement: below the existing settings controls, labeled "Copy diagnostics".

On click:
1. Call `invoke("run_diagnostics")`.
2. Format the result as a readable multi-line string, e.g.:
   ```
   TurboTalk diagnostics
   platform: macos
   mic_available: true
   model_exists: true
   sidecar_exists: true
   sidecar_executable: true
   cleanup_mode: raw
   paste_capability: supported
   ```
3. Write the string to the clipboard using `navigator.clipboard.writeText()`.
4. Show inline confirmation text ("Copied") that disappears after 2 seconds.

Do not add a dedicated diagnostics "panel" or modal — a single button with inline confirmation is sufficient for beta.

## In scope
- `src/App.svelte` — add the button and handler in the settings section

## Out of scope
- Any changes to the `run_diagnostics` Rust implementation (TASK-1)
- Changing the overall layout or design of the settings section
- History, privacy settings, PRIVACY.md (TASK-5–7)
- Failure-mode error messages in the dictation flow (TASK-3)

## Steps
1. Import or reference the `run_diagnostics` binding from `src/bindings.ts`.
2. Add a reactive variable `copiedDiagnostics = false` in the script section.
3. Add `async function copyDiagnostics()` that: calls `run_diagnostics`, formats the result, writes to clipboard, sets `copiedDiagnostics = true`, then resets after 2 seconds via `setTimeout`.
4. Add the button in the settings section markup. Label: "Copy diagnostics". On click: `copyDiagnostics()`. When `copiedDiagnostics` is true, show "Copied" next to the button.
5. Style the button to match existing secondary buttons in the settings section (no new design tokens needed).

## Success signal
In `npm run tauri dev`, opening the settings section shows a "Copy diagnostics" button. Clicking it briefly shows "Copied" and the clipboard contains a formatted plain-text diagnostics block with correct field values for the current machine.
