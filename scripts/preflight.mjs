#!/usr/bin/env node
// Preflight: fail fast if a sidecar/companion lib is missing before tauri bundles a broken installer.
// Host-OS-aware: each host builds for its own OS (cross-compile is out of scope for v1 beta).
// Wired via package.json "package" script (npm run package = preflight && tauri build && rename-artifact)
// to keep the diff minimal and leave tauri.conf.json untouched.
//
// Sidecars are produced/installed by TASK-27 (per-host whisper.cpp build).
// If a check fails on a host where TASK-27 has not been run, that's the expected error.

import { execFileSync } from 'node:child_process';
import { statSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { assertNoCoreMLLinkage } from './lib/dylib-guard.mjs';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

// Per-host required asset list. Each entry is a repo-relative path under src-tauri/binaries/.
// Keep this list narrow: only files that, if missing, will produce a broken bundle.
let required;
switch (process.platform) {
  case 'darwin':
    required = [
      'src-tauri/binaries/whisper-cli-aarch64-apple-darwin',
      'src-tauri/binaries/whisper-server-aarch64-apple-darwin',
      'src-tauri/binaries/libwhisper.1.dylib',
      'src-tauri/binaries/libggml.0.dylib',
      'src-tauri/binaries/libggml-base.0.dylib',
      'src-tauri/binaries/libggml-blas.so',
      'src-tauri/binaries/libggml-metal.so',
      'src-tauri/binaries/ggml-silero-v5.1.2.bin',
    ];
    break;
  case 'win32':
    // Windows: upstream whisper-bin-x64 ships a shared build. Companion DLLs
    // sit alongside the .exe and are bundled via tauri.conf.json
    // bundle.windows.resources. Run `npm run fetch-sidecars` to populate.
    required = [
      'src-tauri/binaries/whisper-cli-x86_64-pc-windows-msvc.exe',
      'src-tauri/binaries/whisper-server-x86_64-pc-windows-msvc.exe',
      'src-tauri/binaries/whisper.dll',
      'src-tauri/binaries/ggml.dll',
      'src-tauri/binaries/ggml-base.dll',
      'src-tauri/binaries/ggml-cpu.dll',
    ];
    break;
  case 'linux':
    // Linux: whisper.cpp built statically per TASK-27 ships a single binary.
    // If a future TASK-27 variant ships .so files alongside the binary, add them here.
    required = [
      'src-tauri/binaries/whisper-cli-x86_64-unknown-linux-gnu',
    ];
    break;
  default:
    console.error(`[preflight] unsupported host platform: ${process.platform}`);
    console.error('[preflight] supported hosts: darwin, win32, linux');
    process.exit(1);
}

let missingCount = 0;
for (const rel of required) {
  const abs = resolve(repoRoot, rel);
  let st;
  try {
    st = statSync(abs);
  } catch {
    console.error(`[preflight] missing required bundle asset: ${rel}`);
    console.error(`[preflight]   expected at: ${abs}`);
    console.error('[preflight]   run `npm run fetch-sidecars` on this host to populate it');
    missingCount += 1;
    continue;
  }
  if (!st.isFile() || st.size === 0) {
    console.error(`[preflight] required bundle asset is empty or not a file: ${rel}`);
    console.error(`[preflight]   path: ${abs}`);
    console.error('[preflight]   re-run `npm run fetch-sidecars` to refresh it');
    missingCount += 1;
  }
}

if (missingCount > 0) {
  console.error(`[preflight] ${missingCount} required asset(s) missing for host ${process.platform}`);
  process.exit(1);
}

if (process.platform === 'darwin') {
  const server = resolve(repoRoot, 'src-tauri/binaries/whisper-server-aarch64-apple-darwin');
  const libWhisper = resolve(repoRoot, 'src-tauri/binaries/libwhisper.1.dylib');
  const links = execFileSync('otool', ['-L', server], { encoding: 'utf8' });
  const leakedLinks = links
    .split('\n')
    .map((line) => line.trim())
    .filter((line) =>
      (line.startsWith('/opt/homebrew/') || line.startsWith('/usr/local/')) &&
      !line.startsWith('/usr/lib/')
    );
  if (leakedLinks.length > 0) {
    console.error('[preflight] whisper-server is not self-contained; Homebrew links remain:');
    for (const line of leakedLinks) console.error(`[preflight]   ${line}`);
    console.error('[preflight]   run `npm run refresh-whisper-server` before packaging');
    process.exit(1);
  }

  try {
    assertNoCoreMLLinkage(server, 'whisper-server');
    assertNoCoreMLLinkage(libWhisper, 'libwhisper.1.dylib');
  } catch (err) {
    console.error(err.message);
    console.error('[preflight]   run `npm run refresh-whisper-server` to restore Metal-only sidecar');
    process.exit(1);
  }

  const vad = resolve(repoRoot, 'src-tauri/binaries/ggml-silero-v5.1.2.bin');
  const vadSize = statSync(vad).size;
  if (vadSize < 10_000) {
    console.error('[preflight] ggml-silero-v5.1.2.bin is missing or a placeholder');
    console.error('[preflight]   run `npm run fetch-vad-model`');
    process.exit(1);
  }
}

console.log(`[preflight] all required bundle assets present for host ${process.platform}`);
