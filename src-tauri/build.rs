fn main() {
    // TASK-24: expose the build target triple to the crate so
    // `transcribe::find_whisper` can resolve the correct sidecar name without
    // hardcoding `aarch64-apple-darwin`. Cargo sets `TARGET` for build scripts
    // but not for the crate itself, so we forward it via `cargo:rustc-env`.
    let target = std::env::var("TARGET").unwrap_or_default();
    println!("cargo:rustc-env=TARGET_TRIPLE={}", target);

    tauri_build::build()
}
