# TASK-63: Prepare signed and notarized distribution pipeline

## Goal
Move TurboTalk's release pipeline from documented unsigned beta packaging toward a repeatable signed distribution process for macOS and Windows. The agent should wire the repo so signing can be enabled when credentials exist, document exactly what secrets are required, and keep unsigned beta behavior explicit until those credentials are available.

## Context
The audit found two distribution issues:

| Audit item | Severity | Surface | Current state |
|------------|----------|---------|---------------|
| #4 | Medium | macOS bundle signing | `src-tauri/tauri.conf.json` uses ad-hoc signing with `"signingIdentity": "-"`; GitHub-downloaded apps will trigger Gatekeeper warnings and notarization is not complete for public distribution. |
| #9 | Low | Windows installer signing | Windows installer is unsigned; SmartScreen will show unknown publisher. |
| #12 | Low | macOS hardened runtime entitlements | `disable-library-validation` and `allow-unsigned-executable-memory` are broad. They may be needed while bundled dylibs are unsigned, but should be revisited after Developer ID signing. |

This task probably cannot complete real signing by itself because it depends on Apple Developer ID certificates, App Store Connect credentials, and a Windows code-signing certificate. It should still leave the repo ready for signed releases and preserve the current unsigned-beta path.

## In scope
- Audit and update macOS signing/notarization config and docs.
- Audit and update Windows signing config/docs.
- Add CI scaffolding that is conditional on secrets being present.
- Add a release checklist for signed artifacts.
- Document current unsigned beta behavior if signing cannot be enabled yet.
- Revisit macOS entitlements and document which ones are required today.

## Out of scope
- Purchasing certificates.
- Uploading private keys to GitHub.
- Running notarization against a real Apple account unless credentials already exist and the user explicitly approves.
- Removing entitlements without verifying the packaged app still launches, records, transcribes, and runs bundled native sidecars.
- Changing updater policy.

## Files to inspect first
- `src-tauri/tauri.conf.json`
- `src-tauri/tauri.macos.conf.json`
- `src-tauri/tauri.windows.conf.json`
- `src-tauri/entitlements.plist`
- `.github/workflows/release.yml`
- `docs/BUILD.md`
- `docs/RELEASING.md`
- `docs/RELEASE-READINESS.md`
- `docs/SMOKE-TEST.md`
- `scripts/verify-macos-bundle.mjs`
- `scripts/rename-artifact.mjs`

## Decision points for the user
The agent should not block on these immediately, but the final report must make them explicit:

1. Does the next public release remain an unsigned beta, or is Developer ID signing required before shipping?
2. Which Apple signing path should be used?
   - Local developer machine with certificate in Keychain.
   - GitHub Actions with base64 P12 certificate and App Store Connect API key secrets.
3. Which Windows signing path should be used?
   - No signing yet, documented SmartScreen warning.
   - Traditional Authenticode certificate.
   - Azure Trusted Signing or another cloud HSM-backed signing flow.

## Steps

### 1. Establish the current signing truth
Read the config and docs and produce a short matrix:

| Platform | Build artifact | Current signing | Current first-run UX | Docs accurate? |
|----------|----------------|-----------------|----------------------|----------------|
| macOS arm64 | `.dmg` / `.app` | ad-hoc or Developer ID | Gatekeeper warning or normal open | yes/no |
| Windows x64 | installer `.exe` | unsigned or Authenticode | SmartScreen warning or normal open | yes/no |
| Linux | AppImage | unsigned | distro-dependent | yes/no |

If docs and code disagree, fix docs or config so they match.

### 2. Prepare macOS Developer ID path
Update or document Tauri/macOS signing so there is a clean path for signed builds:

- Identify the config key that should replace `"signingIdentity": "-"` when signing is enabled.
- Document expected identity format, for example `Developer ID Application: <Name> (<Team ID>)`.
- Verify whether `hardenedRuntime` stays enabled.
- Verify whether notarization is configured through Tauri, GitHub workflow, or a separate script.
- Ensure `Info.plist` and entitlements are referenced correctly.

If adding CI scaffolding:
- Gate signing steps on required secrets, such as:
  - Apple certificate P12/base64
  - certificate password
  - Apple team ID
  - App Store Connect issuer ID
  - App Store Connect key ID
  - App Store Connect private key
- Make unsigned beta builds continue to work when secrets are absent.
- Do not print secrets or derived private material.

### 3. Review entitlements
Inspect `src-tauri/entitlements.plist` and determine why each entitlement exists.

Special attention:
- `com.apple.security.cs.disable-library-validation`
- `com.apple.security.cs.allow-unsigned-executable-memory`

Expected result:
- Either remove an entitlement because it is clearly unnecessary and the app still works, or document why it remains.
- If removal requires packaged smoke testing that is not available, leave the entitlement in place and file a follow-up note tied to signed-dylib validation.

Do not weaken hardened runtime.

### 4. Prepare Windows signing path
Inspect `.github/workflows/release.yml` and Windows Tauri config.

Add or document a conditional signing stage for the Windows installer:
- It should run after the installer is produced and before checksum generation/release upload.
- It should fail clearly if signing is required but credentials are absent.
- It should skip cleanly for unsigned beta builds if that is the selected policy.

Acceptable implementation options:
- Documentation-only, if no signing provider has been chosen.
- Workflow skeleton with placeholder secret names and disabled-by-default condition.
- Actual signing command if a provider/certificate is already configured.

### 5. Update release docs and gates
Update docs so release agents know the difference between unsigned beta and signed release:

- `docs/RELEASING.md`
  - Current beta path.
  - Future signed path or enabled signed path.
  - Required secrets/certificates.
  - How to verify signatures.
- `docs/BUILD.md`
  - Local signing prerequisites if applicable.
- `docs/RELEASE-READINESS.md`
  - Add a gate that explicitly marks signing status.
- `docs/SMOKE-TEST.md`
  - Add first-run expectations for signed vs unsigned artifacts.

Suggested verification commands to document:

```bash
codesign -dv --verbose=4 "Turbo Talk.app"
spctl --assess --type execute --verbose=4 "Turbo Talk.app"
xcrun stapler validate "Turbo Talk.app"
```

Windows verification depends on the signing provider, but likely includes:

```powershell
Get-AuthenticodeSignature .\TurboTalk-<version>-windows-x64-setup.exe
```

### 6. Do not hide unsigned status
If signing is not complete, keep the docs honest:
- macOS GitHub-downloaded artifacts are unsigned/not notarized or ad-hoc signed.
- Windows installers are unsigned and likely trigger SmartScreen.
- This is acceptable only if the release is intentionally labeled beta/internal.

## Suggested commands

```bash
npm run preflight
npm run build
npm run tauri build
node scripts/verify-macos-bundle.mjs
```

Only run full packaging if the environment has the needed platform tools. Do not request credentials through chat; ask the user to configure them outside the repo.

## Success signal
- The repo contains a clear signed-release path for macOS and Windows.
- Unsigned beta behavior remains explicit and accurate.
- Release docs tell an agent exactly what signing credentials are needed and how to verify the resulting artifacts.
- Entitlements are either reduced safely or justified with a concrete reason.
- No workflow step exposes secrets or makes unsigned builds accidentally look signed.

## Notes
- This task is partly engineering and partly release operations. A good final result may be docs plus gated CI scaffolding rather than a fully signed artifact.
- Coordinate with TASK-62 before making claims about tamper resistance. Hash verification and signing solve different parts of the chain.
