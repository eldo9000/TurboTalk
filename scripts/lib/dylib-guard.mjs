// Shared dylib checks for whisper sidecar packaging scripts.

import { execFileSync } from 'node:child_process';

const COREML_MARKERS = [
  'CoreML.framework',
  'libwhisper.coreml.dylib',
  'libggml-coreml',
];

/**
 * Fail if `otool -L` output shows CoreML linked into the binary/dylib chain.
 * Prevents regressing to the TASK-48 dyld-init 60 s startup hang.
 */
export function assertNoCoreMLLinkage(absPath, label = absPath) {
  let links;
  try {
    links = execFileSync('otool', ['-L', absPath], { encoding: 'utf8' });
  } catch (err) {
    throw new Error(`[dylib-guard] otool -L failed for ${label}: ${err.message}`);
  }

  for (const line of links.split('\n')) {
    const trimmed = line.trim();
    for (const marker of COREML_MARKERS) {
      if (trimmed.includes(marker)) {
        throw new Error(
          `[dylib-guard] ${label} links ${marker} — CoreML must not ship in the default Metal sidecar (see docs/reference/COREML-BLOCKER.md)`
        );
      }
    }
  }
}
