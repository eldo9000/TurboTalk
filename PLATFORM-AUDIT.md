# TurboTalk Platform Audit (TASK-1)

Read-only audit of TurboTalk's cross-platform readiness, run on
2026-05-03 from a macOS arm64 host. No source files were modified by this
task. The macOS happy path is therefore unchanged by definition — the audit
is pure inventory.

## Compile audit

`rustup` target inventory after the audit prep step:

```
aarch64-apple-darwin       (already installed, host)
x86_64-apple-darwin        (already installed)
x86_64-pc-windows-gnu      (added cleanly during this task)
x86_64-unknown-linux-gnu   (already installed)
```

Both non-mac targets installed without incident. `rustup target add` was
not a blocker on this host.

`cargo check` was run from `src-tauri/` against each non-mac target. Full
stderr captured at:

- `/tmp/turbotalk-cargo-check-windows.log`  (334 lines)
- `/tmp/turbotalk-cargo-check-linux.log`    (211 lines)

### Windows (`x86_64-pc-windows-gnu`) — fail

Compilation made it past most of the dependency tree (Tauri, wry, rustls,
hyper, tokio all checked) and then died in the `core-foundation` crate
itself. Two unique errors:

| # | error | crate / location | classification |
|---|-------|-------------------|----------------|
| W1 | `E0433: failed to resolve: could not find 'unix' in 'os'` (`use std::os::unix::io::{AsRawFd, RawFd}`) | `core-foundation-0.10.1/src/filedescriptor.rs:19` | (a) unsupported platform boundary missing — `core-foundation` is unconditionally pulled in by `src-tauri/Cargo.toml` as a top-level dep, then it tries to use Unix-only `std::os::unix` against a Windows target. |
| W2 | `E0432: unresolved import 'libc::PATH_MAX'` | `core-foundation-0.10.1/src/url.rs:23` | (a) same root cause — `libc::PATH_MAX` does not exist on Windows. |

Both errors originate inside `core-foundation` (not our code), but they are
caused by *us* listing `core-foundation = "0.10"` as an unconditional
dependency. Fix shape (out of scope for this task): move `core-foundation`
and `core-graphics` under `[target.'cfg(target_os = "macos")'.dependencies]`
in `src-tauri/Cargo.toml`, and gate `src-tauri/src/hotkey.rs` behind
`#[cfg(target_os = "macos")]` with a stub for other targets.

No further errors were reached because compilation aborted on
`core-foundation`. There may be additional Windows-only failures hiding
behind that wall (notably the unconditional `osascript` call in
`paste.rs`, which would compile on Windows but fail at runtime since
`osascript` does not exist).

### Linux (`x86_64-unknown-linux-gnu`) — fail

Different failure family. Compilation died at the GTK/GLib `*-sys` build
scripts because `pkg-config` is not configured for cross-compilation on
this macOS host. Five unique errors, all the same shape:

| # | crate | classification |
|---|-------|----------------|
| L1 | `glib-sys v0.18.1`   | (c) missing system dependency — pkg-config cross-compile sysroot not configured. |
| L2 | `gobject-sys v0.18.0`| (c) same. |
| L3 | `gio-sys v0.18.1`    | (c) same. |
| L4 | `gdk-sys v0.18.2`    | (c) same. |
| L5 | `pango-sys v0.18.0`  | (c) same. |

These are all transitive Tauri-on-Linux dependencies (WebKitGTK / GTK3
stack). Verbatim message from each: "pkg-config has not been configured to
support cross-compilation. Install a sysroot for the target platform and
configure it via PKG_CONFIG_SYSROOT_DIR and PKG_CONFIG_PATH..."

The Linux check therefore cannot tell us whether *our* code (hotkey.rs,
paste.rs, etc.) compiles on Linux — the build aborted on the toolchain
gap, well before our crate was reached. Per the task notes, this is
recorded as a finding rather than chased: cross-compiling Tauri to Linux
from macOS is its own infrastructure project. Honest answer: a Linux
build needs to be attempted from a Linux host (or a containerized cross
toolchain) to surface the real platform boundary failures in our code.

### Failure classification summary

| target | (a) platform boundary | (b) missing sidecar | (c) missing system dep | (d) real bug | uncategorized |
|---|---|---|---|---|---|
| windows-gnu | 2 (W1, W2) | 0 (didn't get there) | 0 | 0 | 0 |
| linux-gnu   | 0 (didn't get there) | 0 (didn't get there) | 5 (L1–L5) | 0 | 0 |

Both targets failed before reaching our own modules. The Windows wall is
in our direct dep `core-foundation`; the Linux wall is in cross-compile
toolchain plumbing.

## Platform-specific code touch points

Every `.rs` file under `src-tauri/src/` was grep'd for: `core_foundation`,
`core_graphics`, `objc`, `cocoa`, `osascript`, `target_os = "macos"`, and
`cfg(target_os`.

| file | macOS-specific imports / shell-outs | gated? |
|---|---|---|
| `src-tauri/src/hotkey.rs` | `use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop}` (line 4); `use core_graphics::event::{CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType, EventField}` (lines 5–8); whole module body uses `CGEventTap::new(...)`, `CFRunLoop::get_current()`, mac keycodes (`0x3E`, `0x36`, `0x3C`, `0x3D`). | **No.** Entire module compiles unconditionally. This is the proximate cause of the Windows failure (and would also fail on Linux once the toolchain gap is past). |
| `src-tauri/src/paste.rs` | `frontmost_app()` shells to `osascript` (line 33); `paste()` shells to `osascript` (line 67) to send `keystroke "v" using command down`. | **Partial.** `frontmost_app()` is split with `#[cfg(target_os = "macos")]` and a `None`-returning stub for non-mac (lines 31, 51). `paste()` itself is **not gated** — it would compile on Windows/Linux but fail at runtime ("osascript: command not found"). |
| `src-tauri/src/transcribe.rs` | Hard-coded sidecar name `"whisper-cli-aarch64-apple-darwin"` at line 65. Single string constant; no `cfg` selection. | **No.** The path-resolution logic is platform-neutral, but the binary it looks for is mac-arm64-only. On Windows/Linux this fails at runtime (sidecar not found). |
| `src-tauri/src/lib.rs` | `use arboard::Clipboard` at line 96 — `arboard` is cross-platform, just noting it. | n/a — cross-platform crate. |
| every other file (`audio.rs`, `audio_finalizer.rs`, `cleanup.rs`, `recorder.rs`, `settings.rs`, `tray.rs`, `vad.rs`, `theme.rs`, `main.rs`) | None matched the platform-specific grep. Whisper-related comments mention "whisper-cli" but no code paths fork on platform. | n/a |

Net: three files carry mac-only behavior — `hotkey.rs`, `paste.rs`,
`transcribe.rs`. `hotkey.rs` is the one currently breaking the Windows
build; `paste.rs` and `transcribe.rs` are runtime time-bombs that would
only surface after `hotkey.rs` is gated.

There is no `objc`, `cocoa`, or direct AppKit usage anywhere in the
codebase — the macOS surface is entirely CoreFoundation/CoreGraphics
crates plus `osascript` shell-outs.

## Tauri config + sidecar assets

`src-tauri/tauri.conf.json` (lines numbered against the file):

- **line 12** `"macOSPrivateApi": true` — required for the current
  CGEventTap-based hotkey. Mac-only flag; harmless on other platforms but
  signals macOS lock-in.
- **line 52** `"externalBin": ["binaries/whisper-cli"]` — single base
  name. Per Tauri sidecar docs, the actual file on disk must be suffixed
  with the target triple (e.g. `binaries/whisper-cli-aarch64-apple-darwin`).
  We have that file; we have no Windows or Linux equivalents.
- **lines 53–57** `"resources": { ... .dylib ... }` — three Mach-O
  dynamic libraries (`libwhisper.1.dylib`, `libggml.0.dylib`,
  `libggml-base.0.dylib`). Mac-only file format. On Windows the
  equivalents would be `.dll`; on Linux `.so`.
- **lines 58–60** `"macOS": { "signingIdentity": "-" }` — ad-hoc local
  signing. Mac-only key, harmless on other platforms.

`src-tauri/binaries/` listing:

```
libggml-base.0.dylib                 macOS dynamic library
libggml.0.dylib                      macOS dynamic library
libwhisper.1.dylib                   macOS dynamic library
whisper-cli-aarch64-apple-darwin     Apple Silicon executable, +x
```

Every artifact in this directory is macOS-arm64-only. Nothing in here
will be picked up by a Windows or Linux build.

For Tauri sidecar conventions to satisfy a multi-platform `externalBin`
declaration of `binaries/whisper-cli`, the following additional files
would need to exist (per `https://tauri.app/develop/sidecar/`):

| target triple | sidecar filename | shipping libs |
|---|---|---|
| `x86_64-apple-darwin`        | `binaries/whisper-cli-x86_64-apple-darwin`        | same `.dylib` set, Intel build |
| `x86_64-pc-windows-msvc`     | `binaries/whisper-cli-x86_64-pc-windows-msvc.exe` | `whisper.dll`, `ggml.dll`, `ggml-base.dll` |
| `aarch64-pc-windows-msvc`    | `binaries/whisper-cli-aarch64-pc-windows-msvc.exe`| same as above, ARM64 |
| `x86_64-unknown-linux-gnu`   | `binaries/whisper-cli-x86_64-unknown-linux-gnu`   | `libwhisper.so.1`, `libggml.so.0`, `libggml-base.so.0` |
| `aarch64-unknown-linux-gnu`  | `binaries/whisper-cli-aarch64-unknown-linux-gnu`  | same as above, ARM64 |

None of these exist today. The hard-coded `"whisper-cli-aarch64-apple-darwin"`
constant in `transcribe.rs:65` would also have to become target-aware
(or use `tauri::process::Command::new_sidecar` which Tauri auto-suffixes).

## Capability table

Sourced from the audit above. "proven" means observed working on this
host or in CI; "not proven" means actively known to fail; "unknown" means
not tested in this audit.

| Capability                | macOS         | Windows       | Linux         |
|---------------------------|---------------|---------------|---------------|
| Build app (cargo check)   | proven (host) | not proven (W1, W2 — `core-foundation` unconditional dep) | unknown (cargo check aborted on cross-compile pkg-config gap before reaching our code) |
| Global hotkey             | proven        | not proven (CGEventTap is mac-only; no Win impl exists) | not proven (CGEventTap is mac-only; no Linux impl exists) |
| Mic capture (cpal)        | proven        | unknown (cpal supports Win, but never built/run here) | unknown (cpal supports Linux, but never built/run here) |
| Whisper sidecar           | proven (arm64 only — no x86_64 mac sidecar shipped) | not proven (no Windows binary in `src-tauri/binaries/`) | not proven (no Linux binary in `src-tauri/binaries/`) |
| Paste into focused app    | proven (osascript Cmd+V) | not proven (paste.rs unconditionally calls osascript) | not proven (same) |
| Overlay window            | proven        | unknown (Tauri overlay window config is platform-neutral but never built/run on Win) | unknown (same) |

## Recommendations for follow-on tasks

The audit confirms what the roadmap already suspected: TurboTalk's
codebase is structurally macOS-only at the type level, not just at the
sidecar-asset level. The unconditional `core-foundation` /
`core-graphics` dependencies in `Cargo.toml` and the un-gated
`hotkey.rs` module mean `cargo check` cannot even *attempt* a Windows
build of our code. TASK-2 (move mac-only deps under
`[target.'cfg(target_os = "macos")']` and gate `hotkey.rs` /
`paste.rs::paste()` behind `#[cfg(target_os = "macos")]` with stub
implementations on other targets) should land first; TASK-3 (sidecar
matrix per target triple) is a natural follow-on once `cargo check`
reaches our modules cleanly. Linux build verification will additionally
require either a Linux host or a documented cross-toolchain setup —
note that as a separate prerequisite rather than something TASK-2 can
satisfy from this macOS host alone.
