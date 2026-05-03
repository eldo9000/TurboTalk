#!/usr/bin/env node
// Bump TurboTalk's version across all three manifests in lockstep.
// A release requires package.json, src-tauri/Cargo.toml, and src-tauri/tauri.conf.json
// to match — drift produces DMGs named one version while the app reports another.
// See BETA-AUDIT-ROADMAP.md Block 5. Run by humans at release time, not CI.
// Invocation: npm run bump-version -- 0.1.0

import { readFileSync, writeFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

const newVersion = process.argv[2];
if (!newVersion) {
  console.error('[bump-version] missing version argument');
  console.error('[bump-version] usage: npm run bump-version -- 0.1.0');
  process.exit(1);
}

// Basic semver: MAJOR.MINOR.PATCH with optional prerelease tag (e.g. 0.1.0-beta.1).
const semverPattern = /^\d+\.\d+\.\d+(-[\w.]+)?$/;
if (!semverPattern.test(newVersion)) {
  console.error(`[bump-version] invalid semver: "${newVersion}"`);
  console.error('[bump-version] expected MAJOR.MINOR.PATCH or MAJOR.MINOR.PATCH-PRERELEASE');
  process.exit(1);
}

const pkgPath = resolve(repoRoot, 'package.json');
const tauriConfPath = resolve(repoRoot, 'src-tauri/tauri.conf.json');
const cargoTomlPath = resolve(repoRoot, 'src-tauri/Cargo.toml');

// JSON files: parse to validate they're well-formed and that the `version`
// field exists, then do a targeted text replacement on the top-level
// `"version": "..."` line. This preserves the original file's whitespace and
// inline-array formatting — a full JSON.stringify roundtrip would reflow the
// document and produce noisy diffs unrelated to the version bump.
function bumpJsonVersion(filePath, label) {
  let text;
  try {
    text = readFileSync(filePath, 'utf8');
  } catch (err) {
    console.error(`[bump-version] failed to read ${label}: ${err.message}`);
    process.exit(1);
  }
  let parsed;
  try {
    parsed = JSON.parse(text);
  } catch (err) {
    console.error(`[bump-version] failed to parse ${label}: ${err.message}`);
    process.exit(1);
  }
  if (typeof parsed.version !== 'string') {
    console.error(`[bump-version] ${label}: top-level "version" field missing or not a string`);
    process.exit(1);
  }
  // Match the first top-level "version": "..." occurrence. JSON has no
  // comments and no other valid syntax that would produce a false match.
  const re = /("version"\s*:\s*")([^"]*)(")/;
  const updated = text.replace(re, (_match, p1, _old, p3) => `${p1}${newVersion}${p3}`);
  if (updated === text && parsed.version !== newVersion) {
    console.error(`[bump-version] ${label}: failed to locate version line for replacement`);
    process.exit(1);
  }
  writeFileSync(filePath, updated);
}

bumpJsonVersion(pkgPath, 'package.json');
bumpJsonVersion(tauriConfPath, 'src-tauri/tauri.conf.json');

// --- Cargo.toml ---
// Section-aware text edit: only touch the version line inside [package],
// never inside [dependencies] or any other table. We split the file into
// blocks by section header and rewrite only the [package] block.
let cargoText;
try {
  cargoText = readFileSync(cargoTomlPath, 'utf8');
} catch (err) {
  console.error(`[bump-version] failed to read src-tauri/Cargo.toml: ${err.message}`);
  process.exit(1);
}

const lines = cargoText.split('\n');
let currentSection = null;
let packageVersionReplaced = false;
const sectionHeader = /^\s*\[([^\]]+)\]\s*$/;
// Match `version = "..."` with optional leading whitespace. We do NOT match
// `something.version = "..."` (e.g. `tauri-build = { version = "2", ... }`)
// because those are inline tables on the right-hand side, not bare keys.
const versionLine = /^(\s*version\s*=\s*")([^"]*)(".*)$/;

const updatedLines = lines.map((line) => {
  const headerMatch = line.match(sectionHeader);
  if (headerMatch) {
    currentSection = headerMatch[1].trim();
    return line;
  }
  if (currentSection === 'package' && !packageVersionReplaced) {
    const m = line.match(versionLine);
    if (m) {
      packageVersionReplaced = true;
      return `${m[1]}${newVersion}${m[3]}`;
    }
  }
  return line;
});

if (!packageVersionReplaced) {
  console.error('[bump-version] could not find `version = "..."` in [package] section of Cargo.toml');
  process.exit(1);
}

writeFileSync(cargoTomlPath, updatedLines.join('\n'));

// --- Re-read and verify all three agree ---
const verifyPkg = JSON.parse(readFileSync(pkgPath, 'utf8')).version;
const verifyTauri = JSON.parse(readFileSync(tauriConfPath, 'utf8')).version;

const verifyCargoText = readFileSync(cargoTomlPath, 'utf8');
let verifySection = null;
let verifyCargo = null;
for (const line of verifyCargoText.split('\n')) {
  const headerMatch = line.match(sectionHeader);
  if (headerMatch) {
    verifySection = headerMatch[1].trim();
    continue;
  }
  if (verifySection === 'package' && verifyCargo === null) {
    const m = line.match(versionLine);
    if (m) {
      verifyCargo = m[2];
      break;
    }
  }
}

if (verifyPkg !== newVersion) {
  console.error(`[bump-version] verify failed: package.json is "${verifyPkg}", expected "${newVersion}"`);
  process.exit(1);
}
if (verifyTauri !== newVersion) {
  console.error(`[bump-version] verify failed: tauri.conf.json is "${verifyTauri}", expected "${newVersion}"`);
  process.exit(1);
}
if (verifyCargo !== newVersion) {
  console.error(`[bump-version] verify failed: Cargo.toml [package].version is "${verifyCargo}", expected "${newVersion}"`);
  process.exit(1);
}

console.log(`[bump-version] all three manifests at ${newVersion}`);
