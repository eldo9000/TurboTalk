#!/usr/bin/env node
// Fetch ONNX Runtime native DLL into src-tauri/binaries/ for Windows builds.
//
// Downloads the official Microsoft.ML.OnnxRuntime NuGet package and extracts
// onnxruntime.dll from it. The nupkg is a standard zip file; 7z handles it
// natively (pre-installed on windows-latest GitHub runners).
//
// ort-sys 2.0.0-rc.12 bundles ONNX Runtime 1.26.0. We pin to the matching
// NuGet package version so the DLL version matches what ort-sys expects.
//
// Wired via package.json: `npm run fetch-onnxruntime`.
// Safe to run on any platform — skips silently on non-Windows.

import { mkdirSync, statSync, copyFileSync } from 'node:fs';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { tmpdir } from 'node:os';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const binariesDir = resolve(repoRoot, 'src-tauri', 'binaries');

// Pin to the ONNX Runtime version that ort-sys 2.0.0-rc.12 uses (1.26.0).
// See ort-sys/build/download/dist.txt → ms@1.26.0 URLs.
const ORT_VERSION = '1.26.0';
const NUGET_URL =
  `https://www.nuget.org/api/v2/package/Microsoft.ML.OnnxRuntime/${ORT_VERSION}`;
const NUGET_DLL_PATH = 'runtimes/win-x64/native/onnxruntime.dll';

if (process.platform !== 'win32') {
  console.log('[fetch-onnxruntime] skipping — only needed on Windows');
  process.exit(0);
}

console.log(`[fetch-onnxruntime] ONNX Runtime ${ORT_VERSION} (Windows x64)`);

// Check if already present (idempotent).
const dllPath = resolve(binariesDir, 'onnxruntime.dll');
try {
  statSync(dllPath);
  console.log('[fetch-onnxruntime] onnxruntime.dll already exists — skipping');
  process.exit(0);
} catch { /* not present, proceed */ }

mkdirSync(binariesDir, { recursive: true });

// Download NuGet package via Python's built-in urllib (no deps needed).
const nupkgPath = join(tmpdir(), `onnxruntime-${ORT_VERSION}.nupkg`);
execFileSync('python3', [
  '-c', `
import urllib.request, os, sys
url = ${JSON.stringify(NUGET_URL)}
dest = ${JSON.stringify(nupkgPath)}
try:
    urllib.request.urlretrieve(url, dest)
except Exception as e:
    print(f'download failed: {e}', file=sys.stderr)
    sys.exit(1)
sz = os.path.getsize(dest)
print(f'downloaded {sz} bytes')
`,
], { stdio: 'inherit' });

try {
  statSync(nupkgPath);
} catch {
  console.error('[fetch-onnxruntime] nuget package was not downloaded');
  process.exit(1);
}

// Extract onnxruntime.dll from the nupkg (standard zip) using 7z.
// 7z is pre-installed on windows-latest GitHub runners.
const extractDir = join(tmpdir(), `onnxruntime-${ORT_VERSION}-dll`);
mkdirSync(extractDir, { recursive: true });
console.log('[fetch-onnxruntime] extracting via 7z …');
execFileSync('7z', [
  'e',                     // extract with directory flattening
  nupkgPath,
  `-o${extractDir}`,
  NUGET_DLL_PATH,
  '-y',
], { stdio: 'inherit' });

// Find the DLL in the flat output
const extractedDll = join(extractDir, 'onnxruntime.dll');
try {
  statSync(extractedDll);
} catch {
  console.error('[fetch-onnxruntime] onnxruntime.dll not found in extracted nupkg');
  process.exit(1);
}

copyFileSync(extractedDll, dllPath);
console.log(`[fetch-onnxruntime] done — ${dllPath}`);
