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

import { mkdirSync, statSync, copyFileSync, readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { tmpdir } from 'node:os';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const binariesDir = resolve(repoRoot, 'src-tauri', 'binaries');

// Pin to the ONNX Runtime version that ort-sys 2.0.0-rc.12 uses (1.26.0).
// See ort-sys/build/download/dist.txt → ms@1.26.0 URLs.
const ORT_VERSION = '1.26.0';

// SHA-256 of the Microsoft.ML.OnnxRuntime 1.26.0 NuGet package
// (Microsoft.ML.OnnxRuntime.1.26.0.nupkg).
// To refresh when ORT_VERSION changes:
//   1. curl -sL -o /tmp/pkg.nupkg
//      "https://www.nuget.org/api/v2/package/Microsoft.ML.OnnxRuntime/${NEW_VERSION}"
//   2. shasum -a 256 /tmp/pkg.nupkg
//   3. Replace the constant below with the new hash.
const NUGET_SHA256 =
  '50cc3772668f04b8373ad65a36793f94699bc4e818f6e691fc68f1578c38ce42';
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

// Download NuGet package. Try python3 first, then python (Windows naming).
const nupkgPath = join(tmpdir(), `onnxruntime-${ORT_VERSION}.nupkg`);
const pythonCmds = process.platform === 'win32' ? ['python', 'python3'] : ['python3'];
let downloaded = false;
for (const py of pythonCmds) {
  try {
    execFileSync(py, [
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
    downloaded = true;
    break;
  } catch {
    console.log(`  ${py} not available, trying next...`);
  }
}

if (!downloaded) {
  // Fallback: try curl.exe (built into Windows 10/11)
  console.log('[fetch-onnxruntime] trying curl.exe fallback...');
  try {
    execFileSync('curl.exe', ['-fsSL', '-o', nupkgPath, NUGET_URL], { stdio: 'inherit' });
    downloaded = true;
  } catch {
    console.error('[fetch-onnxruntime] All download methods failed.');
    console.error('  Install Python 3 or ensure curl.exe is on PATH.');
    process.exit(1);
  }
}

try {
  statSync(nupkgPath);
} catch {
  console.error('[fetch-onnxruntime] nuget package was not downloaded');
  process.exit(1);
}

// Verify package SHA-256 before extraction.
const actualHash = createHash('sha256').update(readFileSync(nupkgPath)).digest('hex');
if (actualHash !== NUGET_SHA256) {
  console.error(`[fetch-onnxruntime] SHA-256 mismatch for NuGet package ${ORT_VERSION}`);
  console.error(`  expected: ${NUGET_SHA256}`);
  console.error(`  actual:   ${actualHash}`);
  process.exit(1);
}
console.log(`[fetch-onnxruntime] NuGet package SHA-256 verified (${actualHash})`);

// Extract onnxruntime.dll from the nupkg (standard zip) using 7z.
// 7z is pre-installed on windows-latest GitHub runners.
const extractDir = join(tmpdir(), `onnxruntime-${ORT_VERSION}-dll`);
mkdirSync(extractDir, { recursive: true });
console.log('[fetch-onnxruntime] extracting via 7z …');

// Try 7z first, then 7za, then PowerShell Expand-Archive (no deps).
let extracted = false;
for (const tool of ['7z', '7za']) {
  try {
    execFileSync(tool, [
      'e',
      nupkgPath,
      `-o${extractDir}`,
      NUGET_DLL_PATH,
      '-y',
    ], { stdio: 'inherit' });
    extracted = true;
    break;
  } catch {
    console.log(`  ${tool} not available, trying next...`);
  }
}

if (!extracted) {
  // PowerShell fallback — Expand-Archive is built into Windows
  console.log('[fetch-onnxruntime] trying PowerShell Expand-Archive...');
  execFileSync('powershell', [
    '-NoProfile',
    '-Command',
    `Expand-Archive -Path "${nupkgPath}" -DestinationPath "${extractDir}" -Force`,
  ], { stdio: 'inherit' });
}

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
