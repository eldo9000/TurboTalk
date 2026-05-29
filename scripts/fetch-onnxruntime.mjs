#!/usr/bin/env node
// Fetch ONNX Runtime native DLL into src-tauri/binaries/ for Windows builds.
//
// ort-sys 2.0.0-rc.12 bundles ONNX Runtime 1.26.0 (via pyke.io CDN).
// The crate's own build script downloads this archive during `cargo build`,
// but Tauri's bundle-resources check needs the DLL in src-tauri/binaries/
// *before* the build starts. This script provides it.
//
// Uses Python's built-in lzma module for decompression so there are no
// Node.js dependency requirements. Python is available on all CI runners.
//
// Wired via package.json: `npm run fetch-onnxruntime`.
// Safe to run on any platform — skips silently on non-Windows.

import { mkdirSync, statSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
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

// Use Python to download + decompress lzma2 tar.
// Python's built-in lzma + tarfile modules handle this natively.
execFileSync('python3', [
  '-c', `
import urllib.request, tarfile, lzma, os, sys

url = ${JSON.stringify(ARCHIVE_URL)}
dest = os.path.join(${JSON.stringify(binariesDir)}, 'onnxruntime.tar.lzma2')

try:
    urllib.request.urlretrieve(url, dest)
except Exception as e:
    print(f'download failed: {e}', file=sys.stderr)
    sys.exit(1)

sz = os.path.getsize(dest)
print(f'downloaded {sz} bytes')

try:
    with lzma.open(dest) as f:
        with tarfile.open(fileobj=f, mode='r|') as tar:
            tar.extractall(path=${JSON.stringify(binariesDir)})
except Exception as e:
    print(f'extract failed: {e}', file=sys.stderr)
    # Clean up partial download
    try: os.remove(dest)
    except: pass
    sys.exit(1)

os.remove(dest)
print('onnxruntime.dll extracted to ' + ${JSON.stringify(binariesDir)})
`,
], { stdio: 'inherit' });

// Verify the DLL landed
try {
  statSync(dllPath);
  console.log(`[fetch-onnxruntime] done — ${dllPath}`);
} catch {
  console.error('[fetch-onnxruntime] onnxruntime.dll not found after extraction');
  process.exit(1);
}
