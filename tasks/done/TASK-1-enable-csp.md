# TASK-1: Enable Content Security Policy in Tauri config

## Goal
`tauri.conf.json` has a non-null CSP value that restricts script sources to `'self'` and connect sources to localhost only, and the app still launches and operates normally in dev and a release build.

## Context
TurboTalk is a personal-use macOS voice dictation app (Tauri 2 + Rust + Svelte 5). A multi-agent code review found that `src-tauri/tauri.conf.json:46` has `"csp": null`, which disables all Content Security Policy enforcement in the webview. Combined with stored XSS risks via the history JSON file (`~/.config/librewin/turbotalk/history.json` — entries are loaded into the UI without sanitization), this means a tampered history file could execute arbitrary JavaScript inside the Tauri webview.

This is the single highest-leverage one-line frontend hardening. Svelte's default escaping prevents the most obvious injection paths, but CSP is required as defense-in-depth.

The app uses:
- Local Vite dev server on `http://localhost:1428` (see `tauri.conf.json` `devUrl`)
- An optional Ollama HTTP call to a user-configurable URL (default `http://localhost:11434`) — the CSP must allow this connect-src
- Tailwind CSS via `@libre/ui` — needs `'unsafe-inline'` for `style-src` (Tailwind injects styles)
- No remote scripts, no remote fonts, no CDNs, no analytics

## In scope
- `src-tauri/tauri.conf.json` (the `app.security.csp` field only)

## Out of scope
- Any other field in `tauri.conf.json` (window config, bundle config, etc.)
- Any Rust source file
- Any frontend file
- History sanitization (separate task)
- Refactoring or unrelated cleanup

## Steps
1. Open `src-tauri/tauri.conf.json` and locate the `app.security.csp` field (currently `null`).
2. Replace `null` with a string CSP that allows: self for default/script/img/font, self + unsafe-inline for style (Tailwind requires this), self + http://localhost:* + http://127.0.0.1:* for connect-src (Ollama lives on localhost). Use a single concatenated string with directives separated by `;`.
3. Run `npm run tauri dev` from the repo root and verify the app launches without CSP violation errors in the webview console.
4. Open the app's main window and exercise: Settings tab, Modes tab, Models tab (which lists files from the models directory), Settings → theme switching. Verify nothing is visually broken.
5. If the app uses Tauri's IPC over a non-self origin, observe any `Refused to connect to ...` errors in DevTools and adjust `connect-src` minimally to allow them.
6. Stop dev. Run `npm run tauri build` and verify the release bundle compiles without errors.
7. Launch the bundled app once to confirm it starts.

## Success signal
- `src-tauri/tauri.conf.json` shows a non-null CSP string in `app.security.csp`.
- `npm run tauri dev` launches the app, all four tabs (history/models/modes/settings) render correctly, no CSP violation errors are printed in the webview console.
- `npm run tauri build` exits 0.
- Pasting (PTT hotkey → speak → release) still works end-to-end (tray icon transitions, overlay shows histogram, transcript appears in history tab, and text gets pasted into the focused app).

## Notes
- If Tailwind's runtime style injection breaks under strict `style-src`, add `'unsafe-inline'` only to `style-src`, never `script-src`.
- If the app loads any remote fonts or images you discover during testing, add the specific origin — do not use `*`.
- Tauri's IPC channel does not need a `connect-src` entry; it uses an internal protocol.
- Multi-agent review reference: finding C-2 / SEC-005 in `/tmp/code-analysis-concern-based-main-20260501.md`.
