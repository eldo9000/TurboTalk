#!/usr/bin/env node
// Copies the Homebrew whisper-server binary and dylibs into src-tauri/binaries/,
// then patches the binary to use @rpath-relative dylib references instead of
// absolute Homebrew paths. Produces a self-contained sidecar that is immune to
// Homebrew ggml upgrades — the same setup whisper-cli uses.
//
// Run this whenever Homebrew upgrades whisper-cpp or ggml:
//   npm run refresh-whisper-server
//
// After running, commit the updated files in src-tauri/binaries/.
// The script is macOS arm64 only (matches the darwin sidecar convention).

import { execSync } from 'node:child_process';
import { copyFileSync, existsSync, statSync, unlinkSync, chmodSync } from 'node:fs';
import { resolve, dirname, basename } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const binariesDir = resolve(repoRoot, 'src-tauri/binaries');

function run(cmd, opts = {}) {
  return execSync(cmd, { encoding: 'utf8', ...opts }).trim();
}

function brewPrefix(formula) {
  try {
    return run(`brew --prefix ${formula}`);
  } catch {
    return null;
  }
}

// ── 1. Locate Homebrew sources ─────────────────────────────────────────────

const whisperPrefix = brewPrefix('whisper-cpp');
const ggmlPrefix    = brewPrefix('ggml');

if (!whisperPrefix) {
  console.error('[refresh-whisper-server] whisper-cpp not found — run: brew install whisper-cpp');
  process.exit(1);
}
if (!ggmlPrefix) {
  console.error('[refresh-whisper-server] ggml not found — run: brew install ggml');
  process.exit(1);
}

const sources = {
  bin:         resolve(whisperPrefix, 'bin/whisper-server'),
  libWhisper:  resolve(whisperPrefix, 'lib/libwhisper.1.dylib'),
  libGgml:     resolve(ggmlPrefix,    'lib/libggml.0.dylib'),
  libGgmlBase: resolve(ggmlPrefix,    'lib/libggml-base.0.dylib'),
  libBlas:     resolve(ggmlPrefix,    'libexec/libggml-blas.so'),
  libMetal:    resolve(ggmlPrefix,    'libexec/libggml-metal.so'),
};

for (const [key, p] of Object.entries(sources)) {
  if (!existsSync(p)) {
    console.error(`[refresh-whisper-server] missing source (${key}): ${p}`);
    process.exit(1);
  }
}

// Resolve all symlinks so we copy the actual Mach-O binary.
const realBin = run(`readlink -f ${sources.bin}`);
console.log(`[refresh-whisper-server] source binary: ${realBin}`);

// ── 2. Copy files into binaries/ ───────────────────────────────────────────

const destinations = {
  bin:         resolve(binariesDir, 'whisper-server-aarch64-apple-darwin'),
  libWhisper:  resolve(binariesDir, 'libwhisper.1.dylib'),
  libGgml:     resolve(binariesDir, 'libggml.0.dylib'),
  libGgmlBase: resolve(binariesDir, 'libggml-base.0.dylib'),
  libBlas:     resolve(binariesDir, 'libggml-blas.so'),
  libMetal:    resolve(binariesDir, 'libggml-metal.so'),
};

// The destination binary may currently be a symlink — remove it first so
// copyFileSync writes a real file, not through the symlink.
if (existsSync(destinations.bin)) unlinkSync(destinations.bin);

copyFileSync(realBin, destinations.bin);
chmodSync(destinations.bin, 0o755);

for (const key of ['libWhisper', 'libGgml', 'libGgmlBase', 'libBlas', 'libMetal']) {
  if (existsSync(destinations[key])) unlinkSync(destinations[key]);
  copyFileSync(sources[key], destinations[key]);
  chmodSync(destinations[key], 0o644);
}

console.log('[refresh-whisper-server] files copied');

// ── 3. Patch absolute Homebrew paths → @rpath-relative ────────────────────

const rawLinks = run(`otool -L ${destinations.bin}`);
let patched = 0;

for (const line of rawLinks.split('\n')) {
  // Match any absolute path that ends in a dylib name like libfoo.N.dylib
  const m = line.trim().match(/^(\/[^\s]+\/(lib[^/\s]+\.dylib))\s/);
  if (!m) continue;
  const [, fullPath, libname] = m;
  // Skip system dylibs — only rewrite Homebrew-rooted paths.
  if (!fullPath.startsWith('/opt/homebrew/') && !fullPath.startsWith('/usr/local/')) continue;
  run(`install_name_tool -change "${fullPath}" "@rpath/${libname}" "${destinations.bin}"`);
  console.log(`[refresh-whisper-server] patched: ${fullPath} → @rpath/${libname}`);
  patched++;
}

if (patched === 0) {
  console.log('[refresh-whisper-server] no absolute Homebrew paths found — already clean');
}

// ── 4. Add missing rpaths to match whisper-cli ─────────────────────────────

const rpathsRaw = run(`otool -l ${destinations.bin} | grep -A2 LC_RPATH | grep "path "`) ;
const existingRpaths = new Set(
  rpathsRaw.split('\n').map(l => l.trim().replace(/^path\s+/, '').replace(/\s+\(offset.*/, ''))
);

const requiredRpaths = ['@loader_path', '@executable_path/../Resources'];
for (const rp of requiredRpaths) {
  if (!existingRpaths.has(rp)) {
    run(`install_name_tool -add_rpath "${rp}" "${destinations.bin}"`);
    console.log(`[refresh-whisper-server] added rpath: ${rp}`);
  }
}

// ── 5. Verify ──────────────────────────────────────────────────────────────

const finalLinks = run(`otool -L ${destinations.bin}`);
const absoluteLeaks = finalLinks
  .split('\n')
  .map(l => l.trim())
  .filter(l => (l.startsWith('/opt/homebrew/') || l.startsWith('/usr/local/')) && !l.startsWith('/usr/lib/'));

if (absoluteLeaks.length > 0) {
  console.error('[refresh-whisper-server] FAIL — binary still has absolute Homebrew paths:');
  for (const l of absoluteLeaks) console.error(' ', l);
  process.exit(1);
}

// Confirm all three expected @rpath entries are present.
const expectedLibs = ['libwhisper.1.dylib', 'libggml.0.dylib', 'libggml-base.0.dylib'];
for (const lib of expectedLibs) {
  if (!finalLinks.includes(`@rpath/${lib}`)) {
    console.error(`[refresh-whisper-server] FAIL — missing @rpath/${lib} in final binary`);
    process.exit(1);
  }
}

// Confirm the binary is a real file, not a symlink.
if (statSync(destinations.bin).isSymbolicLink?.() ?? false) {
  console.error('[refresh-whisper-server] FAIL — destination is still a symlink');
  process.exit(1);
}

// Re-apply ad-hoc signature. install_name_tool invalidates the original
// Homebrew signature; without this the binary may be blocked on first run.
run(`codesign -f -s - "${destinations.bin}"`);
console.log('[refresh-whisper-server] ad-hoc signature applied');

console.log('\n[refresh-whisper-server] verification passed');
console.log('[refresh-whisper-server] final links:');
console.log(finalLinks);
console.log('\nCommit src-tauri/binaries/ to lock this version in.');
