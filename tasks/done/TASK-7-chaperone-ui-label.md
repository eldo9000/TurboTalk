# TASK-7: Clarify Chaperone UI label for Advanced cleanup mode

## Goal
When "Advanced" cleanup mode is selected in settings, the UI shows a one-line explanation: "Sends transcript to your local Ollama server". A user who has never read the docs understands that text leaves the transcription engine but stays on their machine.

## Context
TurboTalk is a Tauri 2 + Svelte 5 dictation app on macOS. The frontend is `src/App.svelte`. There is a cleanup mode selector in the settings section with at least two options: a basic/raw mode and an "Advanced" (Chaperone/LLM) mode that sends the transcript to a local Ollama server for processing.

Currently the mode selector has no explanation copy. Beta users may not understand what "Advanced" does or that it involves any data movement, even though the movement is local-only.

The fix is minimal: add a one-line help text below or beside the cleanup mode selector that is only visible when "Advanced" is selected. No modal, no tooltip, no new component — inline conditional text is sufficient.

Copy: **"Sends transcript to your local Ollama server (localhost only — no internet)"**

Do not change the selector labels, the selector logic, or anything in the Rust backend. Frontend-only change.

## In scope
- `src/App.svelte` — add conditional help text below the cleanup mode selector

## Out of scope
- Any Rust backend changes
- Changing the selector labels or options
- Changing the Ollama URL or cleanup logic
- PRIVACY.md (TASK-5)
- History settings (TASK-6)

## Steps
1. Read `src/App.svelte`. Find the cleanup mode selector.
2. Identify the reactive variable or binding that holds the current cleanup mode value.
3. Add a `{#if cleanupMode === "advanced"}` block (or equivalent for however the value is named) immediately after the selector element.
4. Inside the block, render a `<p>` or `<span>` with the copy above. Style it as muted/secondary text using whatever class is already used for help copy in the settings section. If no such class exists, use `opacity: 0.6` inline.
5. No other changes.

## Success signal
In `npm run tauri dev`, opening settings and selecting "Advanced" cleanup mode shows the explanation text directly below the selector. Selecting any other mode hides the text. The text reads: "Sends transcript to your local Ollama server (localhost only — no internet)".
