#!/usr/bin/env node
// Preflight: fail fast if a sidecar/dylib is missing before tauri bundles a broken DMG.
// macOS-only today: TurboTalk's first beta is mac arm64 (see BETA-AUDIT-ROADMAP.md).
// Wired via package.json "package" script (npm run package = preflight && tauri build)
// to keep the diff minimal and leave tauri.conf.json untouched.

import { statSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

if (process.platform !== 'darwin') {
  console.log('[preflight] non-macOS host: skipping (Win/Linux beta sidecars not yet defined)');
  process.exit(0);
}

const required = [
  'src-tauri/binaries/whisper-cli-aarch64-apple-darwin',
  'src-tauri/binaries/libwhisper.1.dylib',
  'src-tauri/binaries/libggml.0.dylib',
  'src-tauri/binaries/libggml-base.0.dylib',
];

for (const rel of required) {
  const abs = resolve(repoRoot, rel);
  let st;
  try {
    st = statSync(abs);
  } catch {
    console.error(`[preflight] missing required bundle asset: ${rel}`);
    process.exit(1);
  }
  if (!st.isFile() || st.size === 0) {
    console.error(`[preflight] missing required bundle asset: ${rel}`);
    process.exit(1);
  }
}

console.log('[preflight] all required bundle assets present');
