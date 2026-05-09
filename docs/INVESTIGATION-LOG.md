# TurboTalk — Investigation Log

| Date | Status | Entry |
|------|--------|-------|
| 2026-05-09 | RULED OUT | dispatch 1/3 — Standard cmake configure hangs at SVE check_cxx_source_runs on Apple M4 (no SVE support); same hang class as whisper-rs. HuggingFace .mlmodelc artifact confirmed. Fix: -DGGML_CPU_ARM_ARCH=armv8.6-a |
| 2026-05-09 | RULED OUT | dispatch 2/3 — GGML_CPU_ARM_ARCH does not exist in v1.8.4 cmake; flag was silently ignored; SVE probe still hung. Wrong cmake variable name. |
| 2026-05-09 | CONFIRMED | dispatch 3/3 — Source-patch check_cxx_source_runs→check_cxx_source_compiles for SVE in ggml cmake; build succeeded; CoreML chain: whisper-server→libwhisper.1.dylib→libwhisper.coreml.dylib→CoreML.framework; commit e81fb11 |
