# TASK-27: Build and bundle Whisper sidecar binaries for Windows + Linux

## Goal
`src-tauri/binaries/` contains working `whisper-cli` builds for `x86_64-pc-windows-msvc` and `x86_64-unknown-linux-gnu`, alongside the dynamic libraries each binary requires at runtime. Sidecars are named per Tauri's target-triple convention so `tauri build` picks them up automatically.

## Context
TurboTalk transcribes audio by spawning `whisper-cli` from `ggerganov/whisper.cpp`. Today `src-tauri/binaries/` holds:

```
libggml-base.0.dylib                 macOS dylib
libggml.0.dylib                      macOS dylib
libwhisper.1.dylib                   macOS dylib
whisper-cli-aarch64-apple-darwin     mac-arm64 executable
```

That's the v0.8 macOS beta payload. For Win/Linux beta the same artifact must exist for each target.

Tauri's `externalBin` convention (per `https://tauri.app/develop/sidecar/`) expects target-triple-suffixed filenames:

| Target | Sidecar filename | Companion libs |
|---|---|---|
| `x86_64-pc-windows-msvc` | `whisper-cli-x86_64-pc-windows-msvc.exe` | `whisper.dll`, `ggml.dll`, `ggml-base.dll` |
| `x86_64-unknown-linux-gnu` | `whisper-cli-x86_64-unknown-linux-gnu` | `libwhisper.so.1`, `libggml.so.0`, `libggml-base.so.0` |

These get placed alongside the binary in the same directory. Per `tauri.conf.json` they're declared under `bundle.<platform>.resources` (set up in TASK-24).

Build approach for each target:

**Windows (x86_64-pc-windows-msvc)**:
- On a Windows host (or via GitHub Actions `windows-latest` runner): clone `ggerganov/whisper.cpp`, run `cmake -B build -DBUILD_SHARED_LIBS=ON`, `cmake --build build --config Release`. Output: `build/bin/Release/whisper-cli.exe` plus DLLs in `build/bin/Release/`.
- CPU build only for v1 beta. No CUDA/Vulkan/OpenBLAS — keep the dependency surface minimal.
- Rename `whisper-cli.exe` to `whisper-cli-x86_64-pc-windows-msvc.exe`.
- Drop into `src-tauri/binaries/`.

**Linux (x86_64-unknown-linux-gnu)**:
- On an Ubuntu 22.04 host (or `ubuntu-latest` runner): same `cmake` flow with `-DBUILD_SHARED_LIBS=ON`.
- Output: `build/bin/whisper-cli` plus `libwhisper.so.1`, `libggml.so.0`, `libggml-base.so.0` in `build/`.
- Rename binary to `whisper-cli-x86_64-unknown-linux-gnu`. Strip with `strip --strip-unneeded` to shrink.
- Drop into `src-tauri/binaries/`.

These are large binary artifacts (~10–30 MB each). Decide commit strategy:
- **Commit directly** (simplest, biggest repo bloat). Acceptable for Tier 1 beta if the user accepts the LFS-or-bloat tradeoff.
- **Use Git LFS** (`.gitattributes` for `src-tauri/binaries/*.exe`, `*.dll`, `*.so*`). Cleaner long-term.
- **Download script** (e.g. `scripts/fetch-sidecars.mjs`) that pulls from a known-good GitHub release of whisper.cpp matching a pinned tag, and writes them to `src-tauri/binaries/`. Lightest repo footprint, requires network at build time.

Default to the **download script** approach — matches the Tier 1 "no extra weight" principle and keeps the repo clean. Pin to a specific whisper.cpp release tag (e.g. `v1.7.5` or whatever the macOS sidecar was built from — check by running the existing mac binary with `--version`).

The model file (`ggml-base.en.bin`, 141 MB) is **not** part of this task — it ships separately under user data, not as a sidecar.

## In scope
- `src-tauri/binaries/` — add Win and Linux sidecars + companion libs
- `scripts/fetch-sidecars.mjs` (new file, if download approach is chosen)
- `package.json` — add a `fetch-sidecars` script entry
- `.gitignore` — ignore the downloaded binaries if using the script approach
- `.gitattributes` — set up LFS rules if using LFS approach

## Out of scope
- `transcribe.rs` lookup logic (TASK-24)
- `tauri.conf.json` per-platform resources (TASK-24)
- Build CI matrix (TASK-31)
- macOS sidecar — already in place, do not rebuild
- The Whisper model file (`ggml-base.en.bin`)
- CUDA, Vulkan, OpenBLAS, or any GPU-accelerated build

## Steps
1. Determine the existing mac sidecar's whisper.cpp version: run `src-tauri/binaries/whisper-cli-aarch64-apple-darwin --version` and note the commit/tag. Pin all platform builds to that same version for consistency.
2. Pick build approach — download script (default), LFS, or direct commit. Default to download script.
3. If using download script:
   - Either find an existing whisper.cpp GitHub release that ships pre-built `whisper-cli` binaries for Win/Linux (check `https://github.com/ggerganov/whisper.cpp/releases`), or set up a separate prebuilds repo under the user's GitHub.
   - Write `scripts/fetch-sidecars.mjs`: takes no args, reads a pinned version + URL list from a const at the top, downloads each binary + libs into `src-tauri/binaries/`, makes them executable (`chmod +x` on Unix-like targets only), verifies with sha256 hashes baked into the script.
   - Add `npm run fetch-sidecars` to `package.json`.
   - Update `.gitignore` to exclude the downloaded files: `src-tauri/binaries/whisper-cli-*` (but keep the existing mac arm64 file — either commit it explicitly with `!` rule, or fetch it the same way).
4. If pre-built whisper.cpp Win/Linux binaries are not available from upstream, document the build commands explicitly in `BUILD.md` under a new "Building sidecars from source" section. Provide exact `cmake` invocations and rename steps.
5. Run the fetch script (or build manually). Verify each binary runs:
   - Win: `wine src-tauri/binaries/whisper-cli-x86_64-pc-windows-msvc.exe --help` (or run on a real Win box).
   - Linux: `src-tauri/binaries/whisper-cli-x86_64-unknown-linux-gnu --help` on a Linux host.
6. Confirm dynamic linking: on Linux run `ldd src-tauri/binaries/whisper-cli-x86_64-unknown-linux-gnu` and confirm it finds `libwhisper.so.1`, `libggml.so.0`, `libggml-base.so.0` in the same directory (use `LD_LIBRARY_PATH=src-tauri/binaries` if needed). On Windows, `dumpbin /dependents` or check that the DLLs sit alongside the EXE.
7. Document in `BUILD.md` how to refresh / verify the sidecars.

## Success signal
- `ls src-tauri/binaries/` lists at minimum:
  - `whisper-cli-aarch64-apple-darwin` (existing)
  - `whisper-cli-x86_64-pc-windows-msvc.exe` (new)
  - `whisper-cli-x86_64-unknown-linux-gnu` (new)
  - The corresponding `.dylib`, `.dll`, and `.so` files.
- `whisper-cli-x86_64-pc-windows-msvc.exe --version` prints a whisper.cpp version on a Win host.
- `whisper-cli-x86_64-unknown-linux-gnu --version` prints a whisper.cpp version on a Linux host.
- Sha256 hashes (if using download script) match the values pinned in `fetch-sidecars.mjs`.
- macOS happy path unchanged: `npm run package` still produces a working DMG.

## Notes
- whisper.cpp build flags matter. `BUILD_SHARED_LIBS=ON` produces .dll / .so files; OFF makes a single static binary that doesn't need companion libs. A static build is **simpler to ship** but the resulting binary is bigger (~30 MB). Static may be the better Tier 1 choice — revisit the companion-libs requirement once you've checked binary size of a static Win build.
- If choosing static, drop the companion DLL/SO list from this task and from TASK-24's `tauri.conf.json` resources.
- Linux .so versioning is strict — `libwhisper.so.1` must match the SONAME the binary was linked against. Use `readelf -d` to confirm.
- Don't ship debug symbols. `strip --strip-unneeded` on Linux.

→ verify: on a real Win box, run the bundled `.exe` against a sample 16-kHz mono WAV and confirm the `.txt` output is non-empty. Same on Linux.
