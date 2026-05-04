#!/usr/bin/env node
// Rename the Tauri bundle output to TurboTalk's canonical artifact name.
// Convention: TurboTalk-<version>-<os>-<arch>.<ext> (see BUILD.md).
// Wired via package.json "package" script (preflight && tauri build && this).
// Host-OS-aware: each host builds for its own OS (cross-compile out of scope for v1 beta).
//   - darwin → .dmg
//   - win32  → -setup.exe (NSIS)
//   - linux  → .AppImage

import {
  readFileSync,
  mkdirSync,
  copyFileSync,
  writeFileSync,
  statSync,
  existsSync,
  readdirSync,
} from 'node:fs';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

const pkg = JSON.parse(readFileSync(resolve(repoRoot, 'package.json'), 'utf8'));
const version = pkg.version;

// Tauri may emit bundles under either repoRoot/target or repoRoot/src-tauri/target
// depending on workspace layout. Probe both.
const bundleRoots = [
  resolve(repoRoot, 'target/release/bundle'),
  resolve(repoRoot, 'src-tauri/target/release/bundle'),
];

// Pick the most-recently-modified file in `dir` matching `predicate`.
// Returns null if nothing matches.
function pickNewestMatch(dir, predicate) {
  if (!existsSync(dir)) return null;
  let entries;
  try {
    entries = readdirSync(dir);
  } catch {
    return null;
  }
  const candidates = [];
  for (const name of entries) {
    if (!predicate(name)) continue;
    const full = join(dir, name);
    let st;
    try {
      st = statSync(full);
    } catch {
      continue;
    }
    if (!st.isFile() || st.size === 0) continue;
    candidates.push({ path: full, name, mtimeMs: st.mtimeMs });
  }
  if (candidates.length === 0) return null;
  candidates.sort((a, b) => b.mtimeMs - a.mtimeMs);
  return candidates[0];
}

// Find the newest matching bundle file across all candidate bundle roots.
function findBundleFile(subdir, predicate) {
  const probed = [];
  let best = null;
  for (const root of bundleRoots) {
    const dir = join(root, subdir);
    probed.push(dir);
    const hit = pickNewestMatch(dir, predicate);
    if (hit && (best === null || hit.mtimeMs > best.mtimeMs)) {
      best = hit;
    }
  }
  return { best, probed };
}

// Per-host config:
//   subdir     — bundle subdirectory under target/release/bundle/
//   predicate  — filename predicate identifying the artifact in that subdir
//   outName    — canonical TurboTalk-<v>-... output filename
//   archLabel  — user-facing arch label baked into outName
let cfg;
switch (process.platform) {
  case 'darwin': {
    const archLabel = process.arch === 'arm64' ? 'arm64' : process.arch === 'x64' ? 'x64' : process.arch;
    cfg = {
      subdir: 'dmg',
      predicate: (name) => name.toLowerCase().endsWith('.dmg'),
      outName: `TurboTalk-${version}-macos-${archLabel}.dmg`,
      archLabel,
    };
    break;
  }
  case 'win32': {
    // Beta target = NSIS. Tauri 2 emits something like `Turbo Talk_0.8.0_x64-setup.exe`
    // under target/release/bundle/nsis/.
    cfg = {
      subdir: 'nsis',
      predicate: (name) => {
        const lower = name.toLowerCase();
        return lower.endsWith('.exe') && lower.includes('setup');
      },
      outName: `TurboTalk-${version}-windows-x64-setup.exe`,
      archLabel: 'x64',
    };
    break;
  }
  case 'linux': {
    // Beta target = AppImage. Tauri may emit a plain `.AppImage` or a `.AppImage.tar.gz`
    // depending on bundler config — match the actual output, prefer plain AppImage.
    cfg = {
      subdir: 'appimage',
      predicate: (name) => {
        const lower = name.toLowerCase();
        // Prefer the executable AppImage; ignore .tar.gz wrappers.
        return lower.endsWith('.appimage');
      },
      outName: `TurboTalk-${version}-linux-x64.AppImage`,
      archLabel: 'x64',
    };
    break;
  }
  default:
    console.error(`[rename-artifact] unsupported host platform: ${process.platform}`);
    console.error('[rename-artifact] supported hosts: darwin, win32, linux');
    process.exit(1);
}

const { best, probed } = findBundleFile(cfg.subdir, cfg.predicate);
if (!best) {
  console.error(`[rename-artifact] no matching artifact found for host ${process.platform}`);
  console.error(`[rename-artifact] expected a file matching the host predicate in:`);
  for (const dir of probed) {
    console.error(`[rename-artifact]   - ${dir}`);
  }
  console.error('[rename-artifact] did `tauri build` succeed?');
  process.exit(1);
}

const outDir = resolve(repoRoot, 'dist-artifacts');
const outPath = resolve(outDir, cfg.outName);

mkdirSync(outDir, { recursive: true });
copyFileSync(best.path, outPath);

console.log(`[rename-artifact] copied ${best.name} -> dist-artifacts/${cfg.outName}`);

// SHA-256 checksum: format is `<hex><two spaces><filename><newline>`,
// the canonical layout `shasum -a 256 -c` accepts.
const bytes = readFileSync(outPath);
const sha256 = createHash('sha256').update(bytes).digest('hex');
const checksumName = `${cfg.outName}.sha256`;
const checksumPath = resolve(outDir, checksumName);
writeFileSync(checksumPath, `${sha256}  ${cfg.outName}\n`);

console.log(`[rename-artifact] sha256 written: ${checksumName}`);
