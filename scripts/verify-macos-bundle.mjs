#!/usr/bin/env node
// Verify the macOS .app bundle is self-contained enough for installed use.
// This runs after `tauri build`; preflight checks source assets, this checks
// the produced app that users actually launch.

import { execFileSync } from 'node:child_process';
import { existsSync, statSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { assertNoCoreMLLinkage } from './lib/dylib-guard.mjs';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const appPath = process.argv[2]
  ? resolve(process.argv[2])
  : resolve(repoRoot, 'target/release/bundle/macos/Turbo Talk.app');

if (process.platform !== 'darwin') {
  console.log('[verify-macos-bundle] skipped: host is not macOS');
  process.exit(0);
}

function requireFile(rel) {
  const abs = resolve(appPath, rel);
  if (!existsSync(abs)) {
    console.error(`[verify-macos-bundle] missing: ${rel}`);
    console.error(`[verify-macos-bundle] app: ${appPath}`);
    process.exit(1);
  }
  const st = statSync(abs);
  if (!st.isFile() || st.size === 0) {
    console.error(`[verify-macos-bundle] empty or not a file: ${rel}`);
    console.error(`[verify-macos-bundle] path: ${abs}`);
    process.exit(1);
  }
  return abs;
}

const server = requireFile('Contents/MacOS/whisper-server');
requireFile('Contents/MacOS/whisper-cli');
requireFile('Contents/MacOS/turbotalk');
requireFile('Contents/Resources/libggml.0.dylib');
requireFile('Contents/Resources/libggml-base.0.dylib');
requireFile('Contents/Resources/libggml-blas.so');
requireFile('Contents/Resources/libggml-metal.so');
requireFile('Contents/Resources/ggml-silero-v5.1.2.bin');

const libWhisper = requireFile('Contents/Resources/libwhisper.1.dylib');

const links = execFileSync('otool', ['-L', server], { encoding: 'utf8' });
const leakedLinks = links
  .split('\n')
  .map((line) => line.trim())
  .filter((line) =>
    (line.startsWith('/opt/homebrew/') || line.startsWith('/usr/local/')) &&
    !line.startsWith('/usr/lib/')
  );

if (leakedLinks.length > 0) {
  console.error('[verify-macos-bundle] whisper-server is not self-contained; Homebrew links remain:');
  for (const line of leakedLinks) console.error(`[verify-macos-bundle]   ${line}`);
  process.exit(1);
}

try {
  assertNoCoreMLLinkage(server, 'whisper-server');
  assertNoCoreMLLinkage(libWhisper, 'libwhisper.1.dylib');
} catch (err) {
  console.error(err.message);
  process.exit(1);
}

// Code signature must verify. Ad-hoc (`signingIdentity: "-"`) is fine — but a
// broken or missing signature makes Gatekeeper hard-block launch even after the
// documented `xattr -cr` workaround, so a bad signature is a release blocker.
// `--deep --strict` matches what the release workflow signs the bundle with.
try {
  execFileSync(
    'codesign',
    ['--verify', '--deep', '--strict', '--verbose=2', appPath],
    { stdio: 'pipe', encoding: 'utf8' },
  );
} catch (err) {
  console.error('[verify-macos-bundle] codesign verification failed:');
  console.error(String(err.stderr || err.message || '').trim());
  process.exit(1);
}
console.log('[verify-macos-bundle] codesign --verify passed');

console.log(`[verify-macos-bundle] verified ${appPath}`);
