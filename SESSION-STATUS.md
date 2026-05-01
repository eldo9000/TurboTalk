# TurboTalk — Session Status

**Last updated:** 2026-04-30
**Current state:** Block-out. Architecture decided, Tauri scaffolding pending.

## Where We Are

Repo just created. Documentation and module layout are in place. No working build yet.

## Active Focus

M0 — landing the Tauri 2 + Svelte 5 scaffold and wiring the Libre foundation.

## Blockers

None.

## Next Session Should

1. Run `npm create tauri-app@latest` in a temp dir; copy generated `tauri.conf.json`, `build.rs`, `capabilities/`, and frontend skeleton into this repo.
2. Reconcile with existing `Cargo.toml` and `package.json`.
3. Vendor `~/Downloads/Github/Libre-Apps/common-js/` into `./common-js/` for `@libre/ui`.
4. `npm install && npm run tauri dev` — confirm a window opens with Libre titlebar.
5. Land as a single clean commit: `scaffold: Tauri 2 + Svelte 5 + librewin foundation`.

## Recent Decisions

- **Reference, not fork.** Build from scratch using Handy / typr / sagascript as references. Reasoning in `ARCHITECTURE.md`.
- **Consume Libre foundation.** Pin `librewin-common` (tag v0.1.3), vendor `common-js/`. Same pattern as Shelf/Stack/Prism/Fade/Ghost.
- **Private repo.** Personal-use scope until it earns Libre-product promotion.
- **MIT license.** Matches Libre family; no friction if it gets promoted.
