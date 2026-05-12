# TASK-51: UTM Windows 11 ARM VM setup and x64 emulation verification

## Goal
A Windows 11 ARM64 VM is running in UTM on the host Mac, x64 app emulation is confirmed working, and the TurboTalk NSIS installer (.exe from TASK-50) has been copied into the VM and is ready to install.

## Context
UTM (open-source QEMU frontend for macOS) can run a Windows 11 ARM64 VM natively on Apple Silicon with near-native performance. Windows 11 ARM64 includes built-in x64 app emulation, so the TurboTalk x64 NSIS installer (built for `x86_64-pc-windows-msvc`) will run — though rdev's `SetWindowsHookEx` global keyboard hook and enigo's `SendInput` may behave differently under emulation. The goal of this task is to get the VM to a state where the installer can run and the app can be launched, which is the prerequisite for TASK-52 and TASK-53.

This task is primarily human-executed. The agent's role is to check local prerequisites and produce the step-by-step guide. The human must perform the actual VM creation.

## In scope
- Checking whether UTM is already installed on the host Mac
- Producing the step-by-step setup guide as a permanent doc at `docs/WINDOWS-UTM-TESTING.md`
- Confirming the installer from TASK-50 is accessible to copy into the VM

## Out of scope
- Any changes to TurboTalk source or configuration
- Installing Ollama, testing the app — those are TASK-52
- Windows ARM64 native builds of TurboTalk (not needed for this sprint; x64 emulation is sufficient for proof-of-concept testing)

## Steps

### Agent steps
1. Check if UTM is installed: `ls /Applications/UTM.app` or `mdfind -name UTM.app | head -3`
2. Check if a TurboTalk Windows installer exists from TASK-50: `ls dist-artifacts/windows-x64-tmp/*.exe`
3. Write `docs/WINDOWS-UTM-TESTING.md` with the complete setup guide (see template below). Use real paths and exact commands.

### Guide content for `docs/WINDOWS-UTM-TESTING.md`

The guide must cover:

**Prerequisites**
- UTM 4.x from https://mac.getutm.app (free, no App Store required)
- Windows 11 ARM64 ISO — use the UUP dump method: https://uupdump.net → select ARM64 → Windows 11 → latest → create ISO
  - Alternative: Microsoft VLSC if available, or the official Windows 11 on ARM ISO if Microsoft publishes one directly
  - Minimum 8 GB ISO download; allow 30–60 min
- Minimum 8 GB RAM allocated to VM, 80 GB virtual disk

**VM creation in UTM**
- New VM → Virtualize → Windows → enable "Import VHDX Image" if using a pre-built image, or boot from ISO
- Enable "Use Apple Virtualization" (required for Windows ARM on Apple Silicon)
- Allocate 8 GB RAM, 4 CPU cores, 80 GB disk
- Attach the Windows 11 ARM64 ISO as the boot drive

**Windows 11 setup**
- Complete initial setup (region, keyboard, Microsoft account or local account)
- After desktop appears: confirm x64 emulation works by opening Settings → System → About → "Device specifications" → confirm "System type" shows "64-bit operating system, ARM-based processor"
- Install Windows updates before testing (important: some x64 emulation fixes are in cumulative updates)

**Transferring the TurboTalk installer**
- In UTM: use the "Shared Directory" feature (VM Settings → Sharing → Shared Directory → point to `dist-artifacts/windows-x64-tmp/` on the host)
- Or: use Clipboard sharing (UTM installs SPICE guest tools) — the shared folder approach is more reliable for large files
- Install SPICE guest tools first (the SPICE ISO is mounted automatically by UTM; run `spice-guest-tools-*.exe` from the virtual CD drive inside Windows)

**Verification step**
- Open Windows Notepad (x64 app) — confirms basic x64 emulation works
- Open Task Manager → Details → confirm `notepad.exe` Architecture column shows "x64" or "x86_64 (emulated)"

4. After writing the doc, print the path and confirm it was created.

### Human steps (must be executed by the user, not the agent)
- Download UTM if not installed
- Download Windows 11 ARM64 ISO using UUP dump
- Create and configure the VM following `docs/WINDOWS-UTM-TESTING.md`
- Complete Windows 11 first-run setup
- Install SPICE guest tools
- Enable the shared directory pointing at `dist-artifacts/windows-x64-tmp/`
- Confirm x64 emulation works (Notepad → Task Manager check above)

## Success signal
`docs/WINDOWS-UTM-TESTING.md` exists. User reports: "Windows 11 ARM64 VM is running in UTM, x64 Notepad opens, SPICE guest tools installed, shared folder accessible."

## Notes
- Windows 11 ARM64 x64 emulation covers most Win32 API surface. Exceptions: kernel drivers, COM DLL injection, some DirectX. TurboTalk uses none of these.
- rdev uses `SetWindowsHookEx(WH_KEYBOARD_LL, ...)` — this is a user-mode global hook and should work under x64 emulation. If it silently fails to capture events, check if Windows Defender blocks the hook (common in VMs).
- enigo uses `SendInput` for keystroke injection — also user-mode, should work under emulation.
- If UTM performance is too slow for audio testing, note it but don't block on it — audio/transcription is explicitly out of scope for this sprint.
