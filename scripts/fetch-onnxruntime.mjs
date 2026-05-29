#!/usr/bin/env node
// Fetch ONNX Runtime native DLL into src-tauri/binaries/ for Windows builds.
//
// ort-sys 2.0.0-rc.12 bundles ONNX Runtime 1.26.0 (via pyke.io CDN).
// The crate's own build script downloads this archive during `cargo build`,
// but Tauri's bundle-resources check needs the DLL in src-tauri/binaries/
// *before* the build starts. This script provides it.
//
// Steps:
//   1. Download .tar.lzma2 archive via Python's urllib
//   2. Extract via 7z (pre-installed on windows-latest GitHub runners)
//   3. Find onnxruntime.dll in the extracted tree, copy to binariesDir
//
// Wired via package.json: `npm run fetch-onnxruntime`.
// Safe to run on any platform — skips silently on non-Windows.

import { mkdirSync, statSync, copyFileSync, readdirSync, rmSync, existsSync } from 'node:fs';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { tmpdir } from 'node:os';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const binariesDir = resolve(repoRoot, 'src-tauri', 'binaries');

const ARCHIVE_URL =
  'https://cdn.pyke.io/0/pyke:ort-rs/ms@1.26.0/x86_64-pc-windows-msvc.tar.lzma2';

if (process.platform !== 'win32') {
  console.log('[fetch-onnxruntime] skipping — only needed on Windows');
  process.exit(0);
}

console.log('[fetch-onnxruntime] ONNX Runtime 1.26.0 (Windows x64)');

// Check if already present (idempotent).
const dllPath = resolve(binariesDir, 'onnxruntime.dll');
try {
  statSync(dllPath);
  console.log('[fetch-onnxruntime] onnxruntime.dll already exists — skipping');
  process.exit(0);
} catch { /* not present, proceed */ }

mkdirSync(binariesDir, { recursive: true });

// Use Python to download (built-in, no deps).
// 7z does NOT handle HTTPS -> URL resolution on its own in all environments.
const archiveLocal = join(tmpdir(), 'onnxruntime.tar.lzma2');
execFileSync('python3', [
  '-c', `
import urllib.request, os, sys
url = ${JSON.stringify(ARCHIVE_URL)}
dest = ${JSON.stringify(archiveLocal)}
try:
    urllib.request.urlretrieve(url, dest)
except Exception as e:
    print(f'download failed: {e}', file=sys.stderr)
    sys.exit(1)
sz = os.path.getsize(dest)
print(f'downloaded {sz} bytes')
`,
], { stdio: 'inherit' });

if (!existsSync(archiveLocal)) {
  console.error('[fetch-onnxruntime] archive was not downloaded');
  process.exit(1);
}

// Extract using 7z — pre-installed on windows-latest GitHub runners.
// .tar.lzma2 is raw LZMA2; 7z handles the single-step extraction to a tar,
// then extracts the tar, all in one pass.
const extractDir = join(tmpdir(), 'turbotalk-onnxruntime-x');
rmSync(extractDir, { recursive: true, force: true });
mkdirSync(extractDir, { recursive: true });

console.log('[fetch-onnxruntime] extracting via 7z …');
execFileSync('7z', ['x', archiveLocal, `-o${extractDir}`, '-y'], { stdio: 'inherit' });

// Find onnxruntime.dll in the extracted tree — the archive contains a
// nested directory structure (e.g. lib/onnxruntime.dll or bin/onnxruntime.dll).
const files = findFiles(extractDir, 'onnxruntime.dll');
if (files.length === 0) {
  console.error('[fetch-onnxruntime] onnxruntime.dll not found in extracted archive');
  process.exit(1);
}

copyFileSync(files[0], dllPath);
console.log(`[fetch-onnxruntime] done — ${dllPath}`);

// Cleanup temp files
rmSync(extractDir, { recursive: true, force: true });
rmSync(archiveLocal, { force: true });

// --- helpers ---

function findFiles(dir, name) {
  const results = [];
  try {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = join(dir, entry.name);
      if (entry.isDirectory()) {
        results.push(...findFiles(full, name));
      } else if (entry.name === name) {
        results.push(full);
      }
    }
  } catch { /* permission or missing dir — skip */ }
  return results;
}
