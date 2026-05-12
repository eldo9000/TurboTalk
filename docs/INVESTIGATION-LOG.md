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
