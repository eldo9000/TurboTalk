# TurboTalk — Investigation Log

| Date | Status | Entry |
|------|--------|-------|
| 2026-05-09 | RULED OUT | dispatch 1/3 — Standard cmake configure hangs at SVE check_cxx_source_runs on Apple M4 (no SVE support); same hang class as whisper-rs. HuggingFace .mlmodelc artifact confirmed. Fix: -DGGML_CPU_ARM_ARCH=armv8.6-a |
| 2026-05-09 | RULED OUT | dispatch 2/3 — GGML_CPU_ARM_ARCH does not exist in v1.8.4 cmake; flag was silently ignored; SVE probe still hung. Wrong cmake variable name. |
| 2026-05-09 | CONFIRMED | dispatch 3/3 — Source-patch check_cxx_source_runs→check_cxx_source_compiles for SVE in ggml cmake; build succeeded; CoreML chain: whisper-server→libwhisper.1.dylib→libwhisper.coreml.dylib→CoreML.framework; commit e81fb11 |
| 2026-05-11 | OPEN | dispatch 1/3 — Windows x64 CI leg can be restored by adding workflow_dispatch matrix entry to release.yml (ref: 0e9ad71); produces NSIS installer artifact |
| 2026-05-11 | RULED OUT | dispatch 1/3 — Windows x64 CI restored; build failed: tauri.windows.conf.json declares whisper-server resource that fetch-sidecars doesn't provide |
| 2026-05-11 | OPEN | dispatch 2/3 — Remove whisper-server from tauri.windows.conf.json resources; Windows build uses whisper-cli; whisper-server was added after TASK-47 macOS upgrade but never reconciled on Windows |
| 2026-05-11 | RULED OUT | dispatch 2/3 — fetch-sidecars missing whisper-server.exe fixed; build still fails: pre_exec/setsid() in transcribe.rs lacks #[cfg(unix)] guard |
| 2026-05-11 | OPEN | dispatch 3/3 — transcribe.rs pre_exec(setsid) block is ungated; wrap with #[cfg(unix)] to compile on Windows |
| 2026-05-11 | CONFIRMED | dispatch 3/3 — #[cfg(unix)] guard on pre_exec/setsid fixed Windows build; installer downloaded: TurboTalk-0.8.12-windows-x64-setup.exe |
| 2026-05-12 | OPEN | dispatch 1/1 — write docs/WINDOWS-UTM-TESTING.md; UTM confirmed installed, installer at dist-artifacts/windows-x64-tmp/ confirmed present |
| 2026-05-12 | CONFIRMED | dispatch 1/1 — docs/WINDOWS-UTM-TESTING.md written; UTM present, installer confirmed, 6-step guide covers ISO acquisition through x64 verification |
| 2026-05-26 | CONFIRMED | CoreML dyld-init hang — Metal-only default sidecar enforced; preflight + refresh-whisper-server reject CoreML.framework / libwhisper.coreml.dylib linkage; optional sidecar design documented in docs/reference/COREML-BLOCKER.md |
| 2026-05-26 | CONFIRMED | Silero VAD model bundled — ggml-silero-v5.1.2.bin fetched from ggml-org/whisper-vad (864 KB, sha256 29940d98…); npm run fetch-vad-model added |
| 2026-06-16 | MIGRATED | History comments stripped from source — ~55 TASK-XX references across 10+ source files migrated to docs/reference/KNOWN-BUG-CLASSES.md, TRUTH.md, and this log. Inline comments preserve invariants only. |
| 2026-06-24 | CONFIRMED | dispatch 1/3 — Replace Windows GetAsyncKeyState polling with WH_KEYBOARD_LL hook + dedicated message-pump thread; commit 63d79a0 |
| 2026-06-24 | CONFIRMED | dispatch 1/1 — Replace per-call reqwest::blocking::Client construction with a shared OnceLock client; commit a6a84f2 |
| 2026-06-24 | CONFIRMED | dispatch 1/1 — Gate worker invalidation on backend-affecting fields only in save_config; commit 1aeaacf |
| 2026-06-24 | CONFIRMED | dispatch 1/1 — Swap settings cache to Arc&lt;Config&gt; + narrow hot-path accessors; commit 2d149c7 |
| 2026-06-24 | CONFIRMED | dispatch 1/1 — Eliminate temp-file round-trip by building segment WAV bytes in memory; commit 10e18b7 |
| 2026-06-24 | CONFIRMED | dispatch 1/1 — Wire native NSPasteboard changeCount guard + Windows GetClipboardSequenceNumber guard; commit 3e4075d |
