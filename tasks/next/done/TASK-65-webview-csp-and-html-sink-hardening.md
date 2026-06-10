# TASK-65: Tighten webview CSP and remove latent HTML icon sinks

## Goal
Reduce the blast radius of any future webview injection bug by narrowing the Content Security Policy and removing shared-component HTML injection surfaces that are not necessary for user-controlled data.

## Context
The audit found:

| Audit item | Severity | Surface | Current gap |
|------------|----------|---------|-------------|
| #6 | Medium | `src-tauri/tauri.conf.json` | CSP `connect-src` allows every localhost/127.0.0.1 port even though TurboTalk only needs its own IPC endpoint and the Ollama loopback port. |
| #7 | Medium | `common-js` components | `SegmentedControl.svelte` and `Menu.svelte` render icons with `{@html ...}`. Current callers pass static SVG strings, but the shared components make future dynamic metadata an instant XSS sink. |
| #13 | Info | IPC surface | Tauri commands are powerful if the webview is compromised. No current injection path was found, so this task focuses on reducing injection and network reach. |

This is a hardening task, not a response to an active XSS bug in transcript/LLM/error rendering. The audit confirmed transcript, LLM, and error text are not currently rendered with `{@html}` in app windows.

## In scope
- Narrow CSP `connect-src` to the minimum needed endpoints.
- Replace or constrain `{@html opt.icon}` in `common-js/src/components/SegmentedControl.svelte`.
- Replace or constrain `{@html item.icon}` in `common-js/src/components/Menu.svelte`.
- Update current icon call sites to the chosen safer API.
- Add a small documentation note or component comment explaining the icon safety contract.
- Verify the app still builds and the affected controls still render icons.

## Out of scope
- Full IPC redesign.
- Rewriting all shared components.
- Introducing a large icon framework migration unless the app already depends on one and the change is small.
- Changing Ollama URL validation in Rust. That is already handled well by `validate_ollama_url()`.
- Removing all `{@html}` everywhere without evidence.

## Files to inspect first
- `src-tauri/tauri.conf.json`
- `src-tauri/src/ollama.rs`
- `src-tauri/capabilities/*.json`
- `common-js/src/components/SegmentedControl.svelte`
- `common-js/src/components/Menu.svelte`
- Call sites for `SegmentedControl` and `Menu`
- `common-js/COMPONENTS.md`
- `src/App.svelte`

Useful searches:

```bash
rg -n "\\{@html|icon:|<SegmentedControl|<Menu|connect-src|localhost:\\*|127\\.0\\.0\\.1:\\*" src common-js src-tauri
```

## Steps

### 1. Confirm required network endpoints
Build the actual runtime network allowlist:

- Tauri IPC endpoint:
  - `ipc:`
  - `http://ipc.localhost`
- App local assets:
  - `'self'`
- Ollama:
  - default endpoint is `http://localhost:11434`
  - check whether `http://127.0.0.1:11434` is needed for configured URLs

Inspect `ollama.rs` and settings UI to decide whether users can set custom loopback ports. If custom ports are supported and intentional, do not silently break them. In that case choose one of:

1. Keep CSP broad but document that custom Ollama ports require it. This is less ideal.
2. Restrict UI/backend to supported Ollama ports before tightening CSP.
3. Add a Tauri-side proxy command so the webview never connects directly to arbitrary localhost ports.

Recommendation for this task:
- If the frontend only talks to Rust commands and Rust talks to Ollama, `connect-src` does not need broad localhost at all.
- If the frontend directly fetches Ollama, restrict to `http://localhost:11434 http://127.0.0.1:11434`.

### 2. Tighten CSP
In `src-tauri/tauri.conf.json`, change:

```text
connect-src 'self' http://localhost:* http://127.0.0.1:* ipc: http://ipc.localhost
```

to the narrowest accurate version. Likely:

```text
connect-src 'self' ipc: http://ipc.localhost http://localhost:11434 http://127.0.0.1:11434
```

or, if no frontend direct Ollama calls exist:

```text
connect-src 'self' ipc: http://ipc.localhost
```

Run the app enough to confirm update checks, IPC calls, and Ollama settings still work.

### 3. Replace raw HTML icon API
Current risk:

- `SegmentedControl.svelte`: `{@html opt.icon}`
- `Menu.svelte`: `{@html item.icon}`

Preferred fix:
- Change the shared API so icons are rendered as Svelte snippets/components or as a closed enum of locally known icon names.
- If the codebase already has a local icon pattern, follow it.
- If current icons are only static SVG strings, move those SVGs into component-local safe render branches keyed by an icon name.

Acceptable minimal fix if a component API migration is too wide:
- Rename the prop field to something explicit like `trustedIconHtml`.
- Add a runtime guard that only permits a known local set of icon strings or rejects any value not imported from a local constant map.
- Update all call sites to make the trust boundary obvious.

Do not leave a generic prop called `icon` that can receive arbitrary strings and then feed it to `{@html}`.

### 4. Update call sites
Find every `SegmentedControl` and `Menu` caller that passes icons.

For each:
- Convert to the new safe icon API.
- Keep visual layout stable.
- Do not change labels, keyboard behavior, or selection behavior.

If a caller has no icon, leave it alone.

### 5. Document the safety contract
Update `common-js/COMPONENTS.md` or add a short component comment:

- Icons in shared components must not be arbitrary HTML strings.
- User/model/server/error text must be rendered through normal Svelte interpolation.
- If raw HTML is ever needed, it must be local static trusted content with a specific prop name and rationale.

### 6. Verify
Run:

```bash
npm run typecheck
npm run build
npm run preflight
```

If the app can be run locally:
- Open the main window.
- Check segmented controls and menus visually.
- Exercise the settings/cleanup/backend controls that use those components.
- Confirm no console CSP violations for normal app actions.

## Success signal
- CSP no longer allows `http://localhost:*` or `http://127.0.0.1:*` unless the final report explicitly justifies why a wildcard remains necessary.
- `SegmentedControl.svelte` and `Menu.svelte` no longer accept arbitrary `icon` strings and render them directly with `{@html}`.
- Current static icons still render correctly.
- Normal IPC and Ollama workflows still work under the tightened CSP.
- Typecheck/build pass.

## Notes
- This task is a good candidate for one focused frontend/security agent.
- Do not use this task to redesign the whole design system. The win is narrowing trust boundaries.
- If narrowing CSP exposes that a feature depends on direct frontend network calls, document the dependency and prefer routing through Rust in a follow-up.
