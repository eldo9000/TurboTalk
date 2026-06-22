fn main() {
    // TASK-24: expose the build target triple to the crate so
    // `transcribe::find_whisper` can resolve the correct sidecar name without
    // hardcoding `aarch64-apple-darwin`. Cargo sets `TARGET` for build scripts
    // but not for the crate itself, so we forward it via `cargo:rustc-env`.
    let target = std::env::var("TARGET").unwrap_or_default();
    println!("cargo:rustc-env=TARGET_TRIPLE={}", target);

    // Media key toggle helper — compiled inline so it runs in the same
    // process and inherits the app's Accessibility permissions.
    #[cfg(target_os = "macos")]
    {
        cc::Build::new()
            .file("media_toggle.c")
            .flag("-x")
            .flag("objective-c")
            .compile("media_toggle");
    }

    tauri_build::build()
}
