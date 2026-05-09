## SVE-cmake-probe-hang
check_cxx_source_runs for ARM SVE hangs on macOS arm64 (Apple M4/M3/M2/M1 have no SVE).
Fix: patch check_cxx_source_runs → check_cxx_source_compiles in ggml/src/ggml-cpu/CMakeLists.txt.
Applies to: whisper.cpp cmake, any ggml-based cmake build on macOS arm64.
