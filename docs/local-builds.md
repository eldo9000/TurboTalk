# Local Builds (Save GitHub Runner Costs)

Building locally instead of on GitHub runners saves money. Here's what you can
do on each machine.

## Windows Machine

Prerequisites:
- Node.js 22+
- Rust toolchain (`x86_64-pc-windows-msvc`)
- Git

### Dev Build (installer)

```powershell
git pull
npm ci
npm run package
```

Installer appears in `build/`. No GitHub runner minutes consumed.

For the full suite (Rust lint, frontend tests), run:
```powershell
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets
cargo fmt --manifest-path src-tauri/Cargo.toml --check
npm test
```

### Release Builds

Release builds still need GitHub (the signing certificate lives in GitHub
Secrets). Tag-push triggers on GitHub are cheap because releases are rare.

## Mac (Your Primary Machine)

Everything builds natively: `npm run tauri build`.

## What Still Needs GitHub

| Build type | Why it stays |
|------------|-------------|
| Release builds (tag push) | Signing keys are GitHub Secrets |
| ISO builds (LibreWin-OS) | Requires Linux Docker pipeline + 5+ GB downloads |
