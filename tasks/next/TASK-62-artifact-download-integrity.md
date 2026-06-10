# TASK-62: Close artifact and model download integrity gaps

## Goal
Every runtime artifact TurboTalk downloads or bundles has an explicit SHA-256 integrity check before it can be used, copied into the app bundle, or marked as installed. This task covers the build-time Windows ONNX Runtime DLL fetch, runtime Whisper `.bin` downloads, low-risk tokenizer/vocab files, and the committed macOS Whisper sidecar/dylib provenance gap.

## Context
The pre-release security audit found that TurboTalk is already careful in most artifact paths:

- `scripts/fetch-vad-model.mjs` verifies the Silero VAD model hash.
- `scripts/fetch-sidecars.mjs` verifies the Windows whisper.cpp zip hash before extracting sidecars.
- Moonshine and Parakeet model downloads use `RuntimeModelFileSpec.sha256` plus `verify_runtime_model_file()`.

But several paths still trust downloaded or committed native/model artifacts without a pinned digest:

| Audit item | Severity | Surface | Current gap |
|------------|----------|---------|-------------|
| #1 | High | `scripts/fetch-onnxruntime.mjs` | Downloads Microsoft.ML.OnnxRuntime NuGet package and extracts `onnxruntime.dll` without checking the nupkg hash or extracted DLL hash. |
| #2 | Medium | `download_model()` in `src-tauri/src/lib.rs` | Whisper `.bin` files from Hugging Face are downloaded without SHA-256 verification. |
| #3 | Medium | `src-tauri/binaries/` | macOS `whisper-cli`, `whisper-server`, and dylibs are committed with no provenance manifest or release-time hash check. |
| #11 | Low | Moonshine/Parakeet tokenizer and vocab files | `RuntimeModelFileSpec.sha256` is `None` for `tokenizer.json` and `vocab.txt`. |

This is the most security-important pre-release code task because failed integrity here can become native code or parser input on user machines.

## In scope
- Add SHA-256 verification to `scripts/fetch-onnxruntime.mjs`.
- Add SHA-256 verification to Whisper `.bin` runtime downloads in `src-tauri/src/lib.rs`.
- Pin hashes for Moonshine `tokenizer.json` and Parakeet `vocab.txt` where the files are downloaded by TurboTalk.
- Add a committed provenance/hash manifest for macOS sidecars and dylibs under `src-tauri/binaries/`.
- Add or extend a script/preflight check that verifies committed macOS binary hashes against the manifest.
- Update release/build docs only where they need to mention the new integrity checks.

## Out of scope
- Replacing whisper.cpp binaries.
- Upgrading ONNX Runtime, whisper.cpp, Moonshine, or Parakeet versions.
- Signing/notarizing macOS artifacts or Authenticode-signing Windows artifacts. That is TASK-63.
- Changing where models are stored on disk.
- Building a general package manager or auto-updater.

## Files to inspect first
- `scripts/fetch-onnxruntime.mjs`
- `scripts/fetch-vad-model.mjs`
- `scripts/fetch-sidecars.mjs`
- `scripts/preflight.mjs`
- `src-tauri/src/lib.rs`, especially:
  - `download_model()`
  - `RuntimeModelFileSpec`
  - `sha256_file_hex()`
  - `verify_runtime_model_file()`
  - `download_moonshine_model()`
  - `download_parakeet_model()`
- `src-tauri/binaries/`
- `package.json`
- `docs/BUILD.md`
- `docs/RELEASING.md`

## Steps

### 1. Add ONNX Runtime package verification
In `scripts/fetch-onnxruntime.mjs`:

1. Import `readFileSync` and `createHash`.
2. Add a pinned digest constant for the exact NuGet package currently downloaded:
   - URL: `https://www.nuget.org/api/v2/package/Microsoft.ML.OnnxRuntime/1.26.0`
   - Package: `Microsoft.ML.OnnxRuntime` version `1.26.0`
3. After downloading the `.nupkg`, compute SHA-256 over the downloaded package and compare it to the pinned value.
4. On mismatch:
   - Print expected and actual hashes.
   - Delete or leave unused the temp file.
   - Exit non-zero before extraction.
5. Optionally also pin the extracted `onnxruntime.dll` hash. The package hash is the minimum requirement; checking both is better because the script's final trust object is the DLL.
6. Keep the script idempotent and Windows-only.

Hash sourcing guidance:
- Derive the hash from the official NuGet package bytes once, then pin it in source.
- Do not trust a hash from an arbitrary search result.
- Document in a comment how to refresh the hash when `ORT_VERSION` changes.

Verification:
- On non-Windows hosts, `npm run fetch-onnxruntime` should still skip cleanly.
- On a Windows host or CI runner, the script should fail closed if the hash is wrong and should copy `onnxruntime.dll` only after verification.

### 2. Add Whisper `.bin` hash specs
In `src-tauri/src/lib.rs`, refactor `download_model()` so each Whisper model ID maps to a struct with at least:

```rust
struct WhisperModelSpec {
    url: &'static str,
    sha256: &'static str,
    max_bytes: u64,
}
```

Then:

1. Replace `catalog_url(model_id)` with `catalog_model(model_id) -> Option<WhisperModelSpec>` or equivalent.
2. Keep the existing URL validation:
   - `https`
   - host is `huggingface.co`
   - path starts with `/ggerganov/whisper.cpp/resolve/main/`
   - path ends with `.bin`
3. After streaming to the temp file and flushing/syncing it, call the existing `sha256_file_hex()` helper or reuse `verify_runtime_model_file()` with an adapted spec.
4. If the hash mismatches:
   - Drop/close the file handle.
   - Remove the temp file.
   - Return an error that says the model failed integrity verification.
5. Only persist/rename the temp file into place after the hash check passes.
6. When an existing destination file already exists, consider verifying it before treating it as installed. If this path currently does not skip existing Whisper files, do not add a skip unless you also verify first.

Model IDs currently in scope:
- `ggml-large-v3-turbo`
- `ggml-large-v3-turbo-q5_0`
- `ggml-large-v3`

Hash sourcing guidance:
- Use the exact bytes fetched from the current canonical Hugging Face URLs.
- Record the source URL next to each hash.
- Do not change model IDs, filenames, or UI labels.

### 3. Fill tokenizer/vocab hashes
In `download_moonshine_model()` and `download_parakeet_model()`:

1. Replace `sha256: None` for Moonshine `tokenizer.json` specs with pinned SHA-256 values.
2. Replace `sha256: None` for Parakeet `vocab.txt` specs with pinned SHA-256 values.
3. If the same remote file appears in multiple variants, use the same digest in each spec.
4. Keep `verify_runtime_model_file()` behavior unchanged unless a bug is discovered.

These are lower-risk data files, but they are parsed by local model code and should follow the same convention as the ONNX weights.

### 4. Add macOS binary provenance manifest
Add a manifest file for committed macOS sidecars/dylibs. Suggested path:

`src-tauri/binaries/MANIFEST.sha256`

The manifest should include one line per committed macOS native artifact, in standard `shasum -a 256` format:

```text
<sha256>  whisper-cli-aarch64-apple-darwin
<sha256>  whisper-server-aarch64-apple-darwin
<sha256>  libwhisper.1.dylib
<sha256>  libggml.0.dylib
...
```

Include all committed macOS Whisper/ggml/onnx native runtime files that are expected to ship from `src-tauri/binaries/`. Do not include generated Windows `.exe`/`.dll` files if they are fetched on Windows during build.

Add a short comment or adjacent `README.md` only if needed to explain:
- Upstream source/version, currently whisper.cpp `v1.8.4` for the Whisper sidecars.
- How to refresh the manifest when binaries are intentionally replaced.

### 5. Wire manifest verification into preflight
Extend `scripts/preflight.mjs` or add a small script called by preflight/package scripts so release agents cannot accidentally ship a modified committed binary.

Expected behavior:
- Read `src-tauri/binaries/MANIFEST.sha256`.
- Verify each listed file exists.
- Compute SHA-256 of each file.
- Fail non-zero on missing file or digest mismatch.
- Print a concise pass message on success.

Keep this local and dependency-free using Node's built-in `fs` and `crypto`.

### 6. Update docs
Update docs only where needed:

- `docs/BUILD.md`: mention that runtime artifact fetches and committed sidecars are SHA-256 verified.
- `docs/RELEASING.md`: mention that preflight verifies committed binary manifests and artifact checksums.

Do not overstate signing status; unsigned packages are still unsigned until TASK-63 is complete.

## Suggested commands

```bash
npm run fetch-onnxruntime
npm run preflight
npm run typecheck
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
```

If hash sourcing requires downloads and the environment blocks network access, stop and report exactly which hashes still need to be collected. Do not paste placeholder hashes into source.

## Success signal
- `scripts/fetch-onnxruntime.mjs` refuses to extract or copy an unverified NuGet package.
- Whisper `.bin` downloads fail closed on SHA-256 mismatch before moving into the models directory.
- Moonshine and Parakeet tokenizer/vocab downloads use pinned hashes.
- A committed macOS binary manifest exists and is verified by preflight or an equivalent release gate.
- Existing download cancellation and progress behavior still works.
- Project checks listed above pass, except for any network/platform-specific command that is explicitly documented as not run.

## Notes
- This task is intentionally security-biased: fail closed, prefer explicit hashes, and keep error messages understandable.
- Avoid broad refactors in `download_model()`. It is okay to introduce a small local spec struct, but do not rewrite the whole model catalog.
- If upstream model bytes have changed since the previous beta, call that out loudly in the final report. A hash mismatch against a newly sourced digest may indicate a real upstream artifact change, not just a local code gap.
