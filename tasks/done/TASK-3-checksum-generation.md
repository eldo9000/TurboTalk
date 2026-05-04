# TASK-3: Emit SHA-256 checksum alongside the renamed DMG

## Goal
After `npm run package` finishes, `dist-artifacts/` contains both the renamed
DMG and a matching `.sha256` text file with the canonical
`<sha256-hex>  <filename>` format that `shasum -a 256 -c` accepts.

## Context

Block 5 of `BETA-AUDIT-ROADMAP.md` lists "Produce checksums for downloadable
artifacts" as a release checklist item. Beta users need a way to verify the
DMG they downloaded is the one we built. Standard Unix convention is a
`.sha256` file with the format:

```
<64-char-hex>  TurboTalk-0.1.0-macos-arm64.dmg
```

The double-space separator and bare filename (not full path) is what
`shasum -a 256 -c TurboTalk-0.1.0-macos-arm64.dmg.sha256` expects.

Today `scripts/rename-artifact.mjs` runs after `tauri build` and produces
`dist-artifacts/TurboTalk-<version>-macos-<arch>.dmg`. The cleanest extension
point is to either:

(a) extend `rename-artifact.mjs` to also write the `.sha256`, or
(b) add a separate `scripts/checksum-artifact.mjs` chained after it in the
    `package` script in `package.json`.

Pick (a) if it stays under ~60 lines total and the responsibility is still
clear. Pick (b) if extending starts to muddy the original script.

## In scope
- `scripts/rename-artifact.mjs` (extend) **OR** `scripts/checksum-artifact.mjs`
  (new).
- `package.json` `"package"` script — only if option (b) above is chosen.
- `BUILD.md` — add a one-line mention that `npm run package` produces both the
  DMG and a `.sha256` file, plus the verification command users should run.

## Out of scope
- Signing the checksum file (GPG / minisign). Beta is small-scale; SHA-256 of
  a signed+notarized DMG is sufficient integrity proof.
- Generating checksums for the dylibs or sidecar — they're inside the DMG.
- Any non-macOS artifact handling. The renamer is already a no-op on non-Darwin.

## Steps
1. Read `scripts/rename-artifact.mjs` to understand the existing flow.
2. Decide option (a) extend, or (b) new file. State the choice in the commit
   message.
3. After the existing `copyFileSync` writes the renamed DMG:
   - Read the DMG bytes (it's ~50–100 MB; `readFileSync` is fine for one-shot
     scripts).
   - Compute SHA-256 with `node:crypto` `createHash('sha256').update(buf).digest('hex')`.
   - Write `dist-artifacts/<dmg-name>.sha256` containing exactly:
     `<64-hex><two spaces><dmg-filename><newline>`.
   - Print: `[<script-name>] sha256 written: <dmg-filename>.sha256`.
4. Verify by hand:
   - Build is not required to test the logic. Create a fake DMG:
     `mkdir -p dist-artifacts && dd if=/dev/urandom of=dist-artifacts/test.dmg bs=1024 count=1024`
     (or just use any existing file). Run a one-liner that imports the script's
     hashing logic, OR run the script against a mocked DMG path.
   - Confirm `shasum -a 256 -c dist-artifacts/<dmg-name>.sha256` prints
     `<dmg-name>: OK` when the DMG is intact, and `FAILED` if a byte is changed.
5. Add a one-line mention to `BUILD.md` after the existing description of
   `npm run package`:
   > Output: `dist-artifacts/TurboTalk-<version>-macos-arm64.dmg` and a matching
   > `.sha256`. Verify with `shasum -a 256 -c <dmg>.sha256`.

## Success signal
- After `npm run package` succeeds (or after a manual end-to-end test against
  a real or mocked DMG), `dist-artifacts/` contains both the DMG and a
  `<dmg>.sha256` file.
- `shasum -a 256 -c dist-artifacts/<dmg>.sha256` exits 0 with `<dmg>: OK`.
- `BUILD.md` mentions the checksum file in the package output description.

## Notes
- `node:crypto` is built-in; do not add a dependency.
- If the DMG is large enough to make `readFileSync` uncomfortable in the
  future, switch to a streamed hash — but for a 100MB DMG on a release
  workstation it's fine and clearer.
- Don't write the checksum to a different directory; both files in
  `dist-artifacts/` keeps them together for upload.
