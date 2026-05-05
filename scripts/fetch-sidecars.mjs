#!/usr/bin/env node
// Fetch per-host Whisper sidecars into src-tauri/binaries/.
// Pinned to whisper.cpp v1.8.4 — the same version as the committed macOS sidecar.
//
// Acts on Windows only today. macOS arm64 is committed under src-tauri/binaries/.
// Linux is excluded from the release matrix (see .github/workflows/release.yml).
//
// Wired via package.json: `npm run fetch-sidecars`.
// Re-running is idempotent — files are overwritten in place.

import { writeFileSync, readFileSync, mkdirSync, statSync, copyFileSync, rmSync } from 'node:fs';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { tmpdir } from 'node:os';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const binariesDir = resolve(repoRoot, 'src-tauri', 'binaries');

const WHISPER_VERSION = 'v1.8.4';

// Per-host download manifest. Keep `files` list narrow: the .exe Tauri bundles
// as externalBin plus the runtime DLLs declared under bundle.windows.resources.
const TARGETS = {
  win32: {
    label: 'Windows x64 (whisper.cpp ' + WHISPER_VERSION + ')',
    url: `https://github.com/ggml-org/whisper.cpp/releases/download/${WHISPER_VERSION}/whisper-bin-x64.zip`,
    sha256: '74f973345cb52ef5ba3ec9e7e7af8e48cc8c71722d1528603b80588a11f82e3e',
    archiveRoot: 'Release',
    files: [
      { src: 'whisper-cli.exe', dst: 'whisper-cli-x86_64-pc-windows-msvc.exe' },
      { src: 'whisper.dll',     dst: 'whisper.dll' },
      { src: 'ggml.dll',        dst: 'ggml.dll' },
      { src: 'ggml-base.dll',   dst: 'ggml-base.dll' },
      { src: 'ggml-cpu.dll',    dst: 'ggml-cpu.dll' },
    ],
  },
};

const target = TARGETS[process.platform];
if (!target) {
  console.log(`[fetch-sidecars] no fetch needed on ${process.platform} — skipping`);
  process.exit(0);
}

console.log(`[fetch-sidecars] ${target.label}`);
mkdirSync(binariesDir, { recursive: true });

const zipPath = join(tmpdir(), `turbotalk-whisper-${WHISPER_VERSION}-${process.platform}.zip`);
const extractDir = join(tmpdir(), `turbotalk-whisper-${WHISPER_VERSION}-${process.platform}-x`);

const res = await fetch(target.url, { redirect: 'follow' });
if (!res.ok) {
  console.error(`[fetch-sidecars] HTTP ${res.status} fetching ${target.url}`);
  process.exit(1);
}
const buf = Buffer.from(await res.arrayBuffer());
writeFileSync(zipPath, buf);
console.log(`[fetch-sidecars] downloaded ${buf.length} bytes`);

const actualHash = createHash('sha256').update(readFileSync(zipPath)).digest('hex');
if (actualHash !== target.sha256) {
  console.error(`[fetch-sidecars] sha256 mismatch`);
  console.error(`  expected: ${target.sha256}`);
  console.error(`  actual:   ${actualHash}`);
  rmSync(zipPath, { force: true });
  process.exit(1);
}
console.log(`[fetch-sidecars] sha256 verified`);

rmSync(extractDir, { recursive: true, force: true });
mkdirSync(extractDir, { recursive: true });

if (process.platform === 'win32') {
  execFileSync(
    'powershell',
    ['-NoProfile', '-Command',
     `Expand-Archive -Force -Path '${zipPath}' -DestinationPath '${extractDir}'`],
    { stdio: 'inherit' }
  );
} else {
  execFileSync('unzip', ['-o', '-q', zipPath, '-d', extractDir], { stdio: 'inherit' });
}

for (const f of target.files) {
  const src = join(extractDir, target.archiveRoot, f.src);
  try { statSync(src); }
  catch {
    console.error(`[fetch-sidecars] missing ${f.src} in extracted archive`);
    process.exit(1);
  }
  const dst = join(binariesDir, f.dst);
  copyFileSync(src, dst);
  console.log(`[fetch-sidecars] -> src-tauri/binaries/${f.dst}`);
}

rmSync(extractDir, { recursive: true, force: true });
rmSync(zipPath, { force: true });
console.log(`[fetch-sidecars] done — ${target.files.length} file(s) placed`);
