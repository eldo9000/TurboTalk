# TurboTalk — Build Convention

This directory (and its `build/artifacts/` subdirectory) follows a uniform convention
shared across all Libre repos. Every repo that produces an artifact writes its
output to `build/`.

## Output directory

| Repo          | Artifacts path       | Contents                              |
|---------------|----------------------|---------------------------------------|
| TurboTalk     | `build/artifacts/`   | Installer (`*.exe`), checksum (`.sha256`) |
| LibreWin OS   | `build/output/`      | ISO image (`*.iso`), build log        |

The `build/` folder is the single entry point for anyone on the team:

> **Clone → `git pull` → double-click `windows-force-sync-and-build.bat` → find the
> artifact in `build/...`**

---

## Files in this directory

| File / Dir        | Purpose                                          |
|-------------------|--------------------------------------------------|
| `artifacts/`      | Build output — the finished installer (`gitignored`) |
| `windows-install-prereqs.bat` | One-time prerequisite installer (winget)  |
| `windows-build-turbotalk.bat` | Legacy build script (clone + build from scratch) |

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
   `build/artifacts/TurboTalk-<version>-windows-x64-setup.exe` with a `.sha256`.

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
