# TurboTalk — Build Convention

This directory follows a uniform convention shared across all Libre repos. Every
repo writes its build artifacts directly into `build/` — no subdirectories.

## Artifact location

| Repo          | Artifact                                 |
|---------------|------------------------------------------|
| TurboTalk     | `build/TurboTalk-{version}-windows-x64-setup.exe` (+ `.sha256`) |
| LibreWin OS   | `build/librewin-os-0.8-{arch}.iso`       |

The `build/` folder is the single entry point for anyone on the team:

> **Clone → `git pull` → double-click `windows-force-sync-and-build.bat` →
> find the artifact in `build/`**

---

## Files in this directory

| File / Dir                   | Purpose                                          |
|------------------------------|--------------------------------------------------|
| `windows-install-prereqs.bat` | One-time prerequisite installer (winget)        |
| `windows-build-turbotalk.bat` | Legacy build script (clone + build from scratch) |
| `BUILD.md`                    | This file — build convention documentation       |

---

## How the build works

1. **Force-sync** (`git fetch origin && git reset --hard origin/main && git clean -fd`)
   — wipes local changes and matches GitHub exactly.
2. **Prerequisite check** — verifies Git, Node.js 22+, Rust, and NSIS are installed.
3. **Dependencies** — `npm install` → `fetch-sidecars` (whisper.cpp ~4 MB) →
   `fetch-onnxruntime` (~500 MB) → `fetch-vad-model` (silero VAD).
4. **preflight** — validates all required binaries are present before build.
5. **tauri build** — compiles the Rust backend, bundles the Svelte frontend, and
   produces an NSIS installer.
6. **rename-artifact** — copies and renames the installer to
   `build/TurboTalk-{version}-windows-x64-setup.exe` with a `.sha256`.

---

## The double-click contract

Every Libre repo provides `windows-force-sync-and-build.bat` at the repo root.
It guarantees:

- **Nuke local changes** — fresh sync from `origin/main`
- **Prerequisite check** — fails fast with red console if tools are missing
- **Build** — runs the full pipeline
- **Clear output** — green banner with artifact path on success,
  red banner on failure

No manual steps. No reading docs. Double-click and wait.
