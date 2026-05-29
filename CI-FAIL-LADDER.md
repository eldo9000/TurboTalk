# CI Fail Ladder — TurboTalk

## Fail #1 — 2026-05-28 — Windows build: E0505 borrow in hotkey.rs

- **Q1 in-last-commit:** yes — `src-tauri/src/hotkey.rs`
- **Q2 named-error:** yes — `error[E0505]: cannot move out of hotkey_state because it is borrowed` at hotkey.rs:1236
- **Q3 seen-before:** no — first entry this arc
- **Q4 broken-vs-missing:** broken
- **Verdict:** QUICK (budget: 1 attempt)
- **Hypothesis:** Startup log reads `hotkey_state` then moves it into the rdev closure in the same scope; drop the read guard before the move.
## Fail #2 — 2026-05-28 — Windows build: E0583 hotkey_win32 module not found

- **Q1 in-last-commit:** yes — `src-tauri/src/hotkey.rs` (+ `hotkey_win32.rs`)
- **Q2 named-error:** yes — `error[E0583]: file not found for module hotkey_win32` at hotkey.rs:1344
- **Q3 seen-before:** no — different error class from Fail #1 (module path vs borrow)
- **Q4 broken-vs-missing:** broken
- **Verdict:** QUICK (budget: 1 attempt)
- **Hypothesis:** `mod hotkey_win32` inside `hotkey.rs` resolves to `hotkey/hotkey_win32.rs`; file lives at `src/hotkey_win32.rs`. macOS CI skip hid this.
- **Next:** `hotkey.rs:1344` — add `#[path = "hotkey_win32.rs"]` on the mod declaration

## Fail arc closed — 2026-05-28 — 2 entries — green CI 26606830689

## Fail #3 — 2026-05-29 — Windows build: onnxruntime.dll missing at resource path

- **Q1 in-last-commit:** yes — `src-tauri/tauri.windows.conf.json`
- **Q2 named-error:** yes — `resource path 'binaries\onnxruntime.dll' doesn't exist`
- **Q3 seen-before:** no — new failure class (missing resource, not compile error)
- **Q4 broken-vs-missing:** **missing** — declared the resource but no CI step fetches `onnxruntime.dll`
- **Verdict:** **ARC**
- **Hypothesis:** The Windows CI runner has no `binaries/onnxruntime.dll`. `npm run fetch-sidecars` only grabs Whisper/ggml DLLs. Need a fetch step or an npm script that downloads the ONNX Runtime native DLL before `npm run package`.
- **Next:** created `scripts/fetch-onnxruntime.mjs` + `npm run fetch-onnxruntime` — downloads ONNX Runtime 1.26.0 DLL from pyke.io CDN, extracts via Python3 lzma, places in `src-tauri/binaries/`. Chained in `package` script before `tauri build`. Pushed as `2f61295`, triggering `Dev Build (All Platforms)` via `gh workflow run`.
- **Result:** Red — Python `lzma.open()` uses FORMAT_XZ, archive is raw LZMA2. Download worked (30 MB), extract failed: `Input format not supported by decoder`.

## Fail #4 — 2026-05-29 — Windows build: LZMA2 extraction failed in fetch-onnxruntime

- **Q1 in-last-commit:** no — failing script was in `2f61295`, last commit `e14a55f` touched `hotkey.rs` only
- **Q2 named-error:** yes — `extract failed: Input format not supported by decoder`
- **Q3 seen-before:** yes — same arc as Fail #3 (onnxruntime delivery to binaries/)
- **Q4 broken-vs-missing:** broken — extraction code uses wrong decompression method
- **Verdict:** **ARC**
- **Hypothesis:** Python built-in `lzma` module doesn't handle raw LZMA2 archives (no XZ container). 7z is available on `windows-latest` runners and handles this format natively.
- **Next:** rewritten `scripts/fetch-onnxruntime.mjs` to use Python for HTTPS download + 7z for extraction. Also handles nested directory finding in the archive (DLL may be in `bin/` or `lib/` subdir). Pushed with this ladder update.
- **Result:** Red — 7z doesn't recognize `.tar.lzma2` extension on Windows (`Cannot open the file as archive`).

## Fail #5 — 2026-05-29 — Windows build: 7z can't open .tar.lzma2 archive

- **Q1 in-last-commit:** yes — `scripts/fetch-onnxruntime.mjs`
- **Q2 named-error:** yes — `7-Zip: Cannot open the file as archive`
- **Q3 seen-before:** yes — same arc as Fail #3/#4 (onnxruntime delivery to binaries/)
- **Q4 broken-vs-missing:** broken — extraction uses format 7z can't detect
- **Verdict:** **ARC** (third attempt, same arc)
- **Hypothesis:** The `.tar.lzma2` extension is non-standard — 7z 26.00 on Windows can't autodetect the inner LZMA2 format from the filename. The pyke.io dist archives use raw LZMA2 compression (not XZ container, not legacy .lzma) which neither Python's `lzma` module nor 7z's autodetect handles well.
- **Next:** switched strategy — download `Microsoft.ML.OnnxRuntime` NuGet package (official Microsoft distribution, standard .zip format) and extract `runtimes/win-x64/native/onnxruntime.dll` via 7z. NuGet version pinned to 1.26.0 matching the ort-sys dist.txt. Pushed with this ladder update.
