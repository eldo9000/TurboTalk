#!/usr/bin/env node
import { existsSync, rmSync } from 'node:fs';
import { resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const repoRoot = resolve(new URL('..', import.meta.url).pathname);
const builtApp = resolve(repoRoot, 'target/release/bundle/macos/Turbo Talk.app');
const installedApp = '/Applications/Turbo Talk.app';

function run(cmd, args, options = {}) {
  const res = spawnSync(cmd, args, {
    cwd: repoRoot,
    stdio: options.quiet ? 'pipe' : 'inherit',
    encoding: 'utf8',
  });
  if (res.status !== 0 && !options.allowFailure) {
    const detail = res.stderr?.trim() || res.stdout?.trim() || `exit ${res.status}`;
    throw new Error(`${cmd} ${args.join(' ')} failed: ${detail}`);
  }
  return res;
}

if (process.platform !== 'darwin') {
  console.log('[install-macos-app] skipped: host is not macOS');
  process.exit(0);
}

if (!existsSync(builtApp)) {
  console.error(`[install-macos-app] missing built app: ${builtApp}`);
  console.error('[install-macos-app] run `npm run package:app` first');
  process.exit(1);
}

run('pkill', ['-f', '/Applications/Turbo Talk.app/Contents/MacOS/turbotalk'], {
  allowFailure: true,
  quiet: true,
});
run('pkill', ['-f', 'whisper-server|whisper-cli|parakeet'], {
  allowFailure: true,
  quiet: true,
});

rmSync(installedApp, { recursive: true, force: true });
run('ditto', [builtApp, installedApp]);
run('codesign', ['--verify', '--deep', '--strict', '--verbose=2', installedApp]);

if (!process.argv.includes('--no-launch')) {
  run('open', ['-a', 'Turbo Talk']);
}

console.log(`[install-macos-app] installed ${installedApp}`);
