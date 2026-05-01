// Whisper.cpp sidecar wrapper.
//
// Spawns the bundled whisper-cli binary (Tauri sidecar), feeds it the captured
// WAV, and parses the transcript from stdout. Model is configurable; defaults
// to ggml-small.en. Models are downloaded on first use into
// ~/.config/librewin/turbotalk/models/.
//
// Apple Silicon: ensure whisper.cpp is built with Metal support for the sidecar.
