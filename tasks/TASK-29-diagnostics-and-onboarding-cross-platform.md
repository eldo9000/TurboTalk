# TASK-29: Per-OS diagnostics + onboarding readiness probe

## Goal
The `run_diagnostics` Tauri command in `src-tauri/src/diagnostics.rs` returns fields meaningful on the current OS instead of mac-only fields, and the first-launch onboarding gate (commit `0eff544`) checks per-OS readiness instead of macOS Accessibility/TCC.

## Context
Two existing surfaces are mac-shaped today and need to honestly reflect what the runtime can determine on Win/Linux.

**1. `src-tauri/src/diagnostics.rs` — `run_diagnostics`** (added in TASK-1 of Block 3, commit `9a61f81`). Today it returns 7 health-check fields focused on the macOS environment:
- platform
- microphone availability (mac TCC)
- model file exists
- whisper-cli sidecar exists + executable
- cleanup mode + Ollama reachability
- accessibility-trusted flag (from `hotkey::accessibility_trusted()`)
- selected hotkey config

On Windows + Linux these mac concepts don't apply directly. Replacements:

| Mac field | Windows replacement | Linux replacement |
|---|---|---|
| Accessibility trusted (AXIsProcessTrusted) | WebView2 runtime present (registry probe) | Session type (X11 / Wayland) — unsupported if Wayland |
| Mic TCC permission | (Win has no permission gate at process start; report `cpal` device available) | (same — report `cpal` device available) |
| Whisper sidecar resolves | sidecar `.exe` resolves under `src-tauri/binaries/` | sidecar binary resolves |

Add an explicit per-OS field: `hotkey_backend_status` and `paste_backend_status`. Possible values: `"available"`, `"unsupported_wayland"`, `"unavailable_<reason>"`. Frontend's "Copy diagnostics" button (TASK-2 / commit `7fce12b`) already serializes everything — adding fields is safe.

**2. Onboarding gate (commit `0eff544 feat(onboarding): first-launch readiness gate + permission flow`)** — assumes mac TCC + Accessibility flow. On a Win/Linux first launch the gate should:
- Skip mic-permission prompt (no equivalent on Win/Linux out of the box).
- On Win: confirm WebView2 is present; if missing, deep-link the user to the Microsoft installer URL.
- On Linux: confirm `XDG_SESSION_TYPE` is not Wayland; if Wayland, show a one-screen "TurboTalk does not support Wayland in this beta — switch to an X11 session" message and continue (don't hard-block, just inform).
- Confirm hotkey backend can register (rdev listen succeeds) — best-effort; if it fails, show the existing `ui-error` banner kind from TASK-25.

The onboarding code lives in the frontend Svelte components plus a backend readiness command. Find the entry point by grepping for the commit's added files: `git show --stat 0eff544`.

`tauri-specta` typed bindings are in use (TASK-8, commit `1a1ebed`). Any new diagnostic field added to a `serde::Serialize + specta::Type` struct will regenerate `src/bindings.ts` automatically when tests are run. Don't write the TS types by hand.

## In scope
- `src-tauri/src/diagnostics.rs` — add per-OS conditional fields
- The frontend "Copy diagnostics" presentation — extend to show the new fields if it's templating from a static list rather than iterating
- The onboarding readiness backend command + its frontend gate logic — add per-OS branches
- `src/bindings.ts` — regenerate via the existing test harness, do not hand-edit

## Out of scope
- Hotkey impl (TASK-25)
- Paste impl (TASK-26)
- Sidecar binaries (TASK-27)
- Anything outside `diagnostics.rs` and the onboarding flow
- Cross-platform mic permission research — Win/Linux don't have equivalents at process start; just report capability
- Adding new permissions screens to onboarding

## Steps
1. Run `git show --stat 0eff544` to identify which files the onboarding commit touched. Read each one.
2. Read `src-tauri/src/diagnostics.rs` end to end. Identify which fields are mac-only and need `cfg`-branched values.
3. Define a minimal per-OS extension to the diagnostics struct. Add fields without removing existing ones (preserve mac contract). Use `Option<T>` and let mac populate the mac-relevant fields, Win the Win-relevant fields, etc. Or use a single `platform_specific: HashMap<String, String>` map — pick whichever fits the existing struct style.
4. Implement the per-OS probes:
   - Windows: WebView2 detection via reading `HKLM\Software\Wow6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}` registry key, or shelling to PowerShell `Get-ItemProperty`. Pick whichever has zero new dependencies.
   - Linux: `std::env::var("XDG_SESSION_TYPE")`, normalize to lowercase.
   - Mac: keep existing AXIsProcessTrusted call.
   - All: probe `cpal::default_host().default_input_device()` for mic presence (already partially done — preserve).
   - All: probe hotkey backend status — for mac, return based on `accessibility_trusted()`; for non-mac, return based on whether the rdev listener thread spawned successfully (introduce a static AtomicBool set by `hotkey::imp::spawn` after listener registration).
5. In the onboarding gate, branch the readiness check by `cfg(target_os)`:
   - mac: existing flow.
   - Windows: skip mic permission, check WebView2, check hotkey backend.
   - Linux: skip mic permission, check Wayland status (warn if Wayland), check hotkey backend.
6. Adjust the onboarding frontend copy to be platform-aware. Use `@tauri-apps/api/os::platform()` to fetch the runtime OS string and switch the displayed instructions. Keep all three flavors in the same component — no separate routes.
7. Run `cargo test --manifest-path src-tauri/Cargo.toml`. The export-bindings test should regenerate `src/bindings.ts` to include any new types.
8. Run `npm run build` and confirm the frontend type-checks against the regenerated bindings.

## Success signal
- `cargo test export_bindings` green.
- `npm run build` green.
- On macOS dev build, the "Copy diagnostics" output is unchanged in mac fields and now includes hotkey/paste backend status lines.
- On a Windows dev build (when available), running `run_diagnostics` returns a payload with `webview2_present: true/false` (or equivalent).
- On a Linux dev build, the payload contains `session_type: "x11"` or `"wayland"`.
- First-launch onboarding on Win shows "no mic permission needed" copy, not the macOS Accessibility instructions.
- First-launch onboarding on Linux/Wayland shows the unsupported-session warning.

## Notes
- WebView2 detection via registry is brittle — Microsoft has changed the GUID and key path before. Cross-reference against `https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution`. If the registry probe is too fragile, fall back to "attempt to create a webview and report success" — but that's heavier.
- Don't add a new top-level dependency for this. Use `winreg` only if it's already transitively present; otherwise PowerShell shellout via `std::process::Command` is fine.
- The onboarding gate must not block app startup if any check fails. It informs and lets the user proceed.

→ verify: copy-diagnostics on Win shows WebView2 + hotkey backend status; on Linux/X11 shows session_type=x11; on Linux/Wayland shows session_type=wayland and the warning banner is visible at first launch.
