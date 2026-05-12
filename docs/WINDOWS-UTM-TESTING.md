# Windows UTM Testing Guide

How to run TurboTalk's Windows x64 installer in a UTM Windows 11 ARM64 VM on Apple Silicon for pre-release smoke testing.

---

## Prerequisites

| Item | Status |
|------|--------|
| UTM 4.x | Already installed at `/Applications/UTM.app` |
| Windows 11 ARM64 ISO | Must be obtained (see below) |
| TurboTalk installer | `dist-artifacts/windows-x64-tmp/TurboTalk-0.8.12-windows-x64-setup.exe` |

**Host requirements:** Apple Silicon Mac (M1/M2/M3/M4), 16 GB+ RAM recommended (8 GB minimum for VM + host).

---

## Step 1 — Obtain Windows 11 ARM64 ISO

Microsoft does not offer a direct ISO download for ARM64 from the regular download page. Use **UUP dump**:

1. Open https://uupdump.net in a browser
2. Search for: `Windows 11` → select the latest build → select **ARM64** architecture
3. Select language (English) → select **Windows 11 Home** or **Pro**
4. On the download page select **Download method: Download and convert the UUP files locally**
5. Download the `.zip` package, extract it, and run the included script:
   - macOS: the script requires `aria2c` and `cabextract` — install via `brew install aria2 cabextract`
   - Run `./uup_download_macos.sh` — this downloads ~4 GB and produces a `.iso` file
   - Allow 20–60 min depending on connection speed

The resulting ISO is typically named something like `22631.XXXXX.XXXXXXXX_ARM64FRE_EN-US.ISO`.

---

## Step 2 — Create the UTM VM

1. Open UTM → click **+** (new VM) → choose **Virtualize**
2. Select **Windows** → check **Import VHDX image** is NOT selected (we're booting from ISO)
3. Check **Use Apple Virtualization** — required for Windows ARM on Apple Silicon
4. Hardware settings:
   - CPU cores: **4**
   - RAM: **8192 MB** (8 GB minimum; 12 GB preferred)
   - Storage: **80 GB** (creates a new virtual disk)
5. Shared directory: point to `<repo>/dist-artifacts/windows-x64-tmp/` — this makes the installer accessible inside Windows without USB or network transfer
6. Boot ISO: attach the Windows 11 ARM64 ISO as the CD/DVD drive
7. Name the VM something like `Windows 11 ARM – TurboTalk Test`
8. Click **Save**

---

## Step 3 — Install Windows 11

1. Start the VM — it boots from the ISO
2. Complete Windows 11 setup:
   - Region: United States (or your locale)
   - Keyboard layout: US
   - Network: select **I don't have internet** if you want to skip Microsoft account requirement, or sign in
   - Create a local account: username `test`, no password (simplest for a throwaway test VM)
3. Wait for first-run setup to complete (~10–15 min)
4. When the desktop appears: open **Settings → System → About** and confirm:
   - **System type:** `64-bit operating system, ARM-based processor`
   - This confirms Windows 11 ARM64 is running and x64 emulation is available

---

## Step 4 — Install SPICE Guest Tools

UTM bundles a SPICE guest tools ISO. This enables clipboard sharing, resolution scaling, and shared folder access.

1. In UTM: right-click the VM → **Edit** → **Drives** → add a new CD/DVD drive and point it at UTM's SPICE tools ISO
   - Location: `/Applications/UTM.app/Contents/Resources/spice-guest-tools-latest.iso` (or check UTM's VM gallery for the path)
   - Alternatively: UTM may auto-mount SPICE tools on first boot — look for a virtual CD in Windows Explorer
2. Inside Windows: open File Explorer → find the SPICE CD → run `spice-guest-tools-*.exe`
3. Complete installation, allow reboot
4. After reboot: the shared directory from Step 2 should appear as a network drive (`\\Mac\<name>` or `Z:\`)

---

## Step 5 — Copy installer into VM

After SPICE tools are installed and the shared directory is mounted:

1. Inside Windows: open File Explorer → navigate to the shared folder (`\\Mac\<share-name>`)
2. You should see `TurboTalk-0.8.12-windows-x64-setup.exe`
3. Copy it to the Desktop (optional — just for convenience)

If the shared folder is not visible, use the UTM clipboard to paste the file path and open it directly, or use a USB drive as an alternative.

---

## Step 6 — x64 emulation verification

Before installing TurboTalk, confirm x64 app emulation works:

1. Open **Notepad** (Start → search "Notepad")
2. Open **Task Manager** → Details tab → find `notepad.exe`
3. Right-click the column header → Add Column → **Architecture**
4. Confirm `notepad.exe` shows `x64` or `x86 (emulated)` — either confirms x64 emulation is active

If Notepad doesn't appear in Details or Architecture shows `ARM64`, x64 emulation may not be enabled. Check Windows Update — some emulation improvements arrive via cumulative updates.

---

## Next steps

With the VM ready and x64 emulation confirmed, proceed to:

- **TASK-52** (`tasks/TASK-52-windows-smoke-test.md`) — install TurboTalk, run the 8-item non-dictation smoke test
- **TASK-53** (`tasks/TASK-53-hotkey-paste-validate-ledger.md`) — test hotkey capture and paste injection, update TRUTH.md

---

## Notes

- **rdev `WH_KEYBOARD_LL` under x64 emulation**: global keyboard hooks are user-mode Win32 and work under x64 emulation on Windows ARM. If the hook silently fails, check Windows Security → Virus & Threat Protection → Protection History for blocked events.
- **Ollama for Windows**: download from https://ollama.com/download/windows. Requires Windows 10 22H2+ — Windows 11 ARM satisfies this. Install inside the VM for TASK-52 Test 6.
- **Transcription is out of scope** for this testing phase. `whisper-server.exe` is bundled in the installer but the model file (`ggml-large-v3-turbo.bin`, 1.6 GB) must be downloaded separately. Skip model setup — the goal is hotkey + paste + settings + Ollama, not transcription quality.
- **VM snapshots**: before running tests, take a UTM snapshot (`VM → Snapshot → Save`) so you can roll back if anything goes wrong.
