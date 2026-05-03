#!/usr/bin/env node
// Rename the Tauri DMG output to TurboTalk's canonical artifact name.
// Convention: TurboTalk-<version>-<os>-<arch>.dmg (see BUILD.md).
// Wired via package.json "package" script (preflight && tauri build && this).
// macOS-only today: TurboTalk's first beta is mac arm64 (see BETA-AUDIT-ROADMAP.md).

import { readFileSync, mkdirSync, copyFileSync, writeFileSync, statSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

if (process.platform !== 'darwin') {
  console.log('[rename-artifact] non-macOS host: skipping (Win/Linux beta artifacts not yet defined)');
  process.exit(0);
}

const pkg = JSON.parse(readFileSync(resolve(repoRoot, 'package.json'), 'utf8'));
const version = pkg.version;

// Tauri 2 names macOS DMGs as `<productName>_<version>_<archAlias>.dmg`.
// productName comes from src-tauri/tauri.conf.json ("Turbo Talk").
// archAlias is "aarch64" for arm64, "x64" for Intel.
const archAlias = process.arch === 'arm64' ? 'aarch64' : process.arch === 'x64' ? 'x64' : process.arch;
const sourceDmg = resolve(
  repoRoot,
  'src-tauri/target/release/bundle/dmg',
  `Turbo Talk_${version}_${archAlias}.dmg`,
);

try {
  const st = statSync(sourceDmg);
  if (!st.isFile() || st.size === 0) {
    console.error(`[rename-artifact] expected DMG is empty or not a file: ${sourceDmg}`);
    process.exit(1);
  }
} catch {
  console.error(`[rename-artifact] expected DMG not found: ${sourceDmg}`);
  console.error('[rename-artifact] did `tauri build` succeed? Check src-tauri/target/release/bundle/dmg/.');
  process.exit(1);
}

// Canonical name uses "arm64" (user-facing), not "aarch64" (toolchain alias).
const archLabel = process.arch === 'arm64' ? 'arm64' : process.arch === 'x64' ? 'x64' : process.arch;
const outDir = resolve(repoRoot, 'dist-artifacts');
const outName = `TurboTalk-${version}-macos-${archLabel}.dmg`;
const outPath = resolve(outDir, outName);

mkdirSync(outDir, { recursive: true });
copyFileSync(sourceDmg, outPath);

console.log(`[rename-artifact] copied DMG to dist-artifacts/${outName}`);

// SHA-256 checksum: format is `<hex><two spaces><filename><newline>`,
// the canonical layout `shasum -a 256 -c` accepts.
const dmgBytes = readFileSync(outPath);
const sha256 = createHash('sha256').update(dmgBytes).digest('hex');
const checksumName = `${outName}.sha256`;
const checksumPath = resolve(outDir, checksumName);
writeFileSync(checksumPath, `${sha256}  ${outName}\n`);

console.log(`[rename-artifact] sha256 written: ${checksumName}`);
