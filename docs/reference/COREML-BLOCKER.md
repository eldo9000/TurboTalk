# CoreML / Neural Engine — dyld blocker and approved path

**Status:** Phase 2 deferred. Metal + adaptive `audio_ctx` + warm whisper-server is the
shipped default. CoreML is not active in production builds.

## What broke

Commit `e81fb11` replaced the bundled whisper sidecar with a CoreML-enabled source build.
The resulting dylib chain was:

```
whisper-server → libwhisper.1.dylib → libwhisper.coreml.dylib → CoreML.framework
```

`CoreML.framework` initializes during **dyld load**, before `main()`. Every cold start of
`whisper-server` blocked ~60 s — even with no `.mlmodelc` present and CoreML never requested.
`WHISPER_COREML_ALLOW_FALLBACK=1` does not help; the hang is in the framework loader, not
whisper's runtime flag handling.

Reverted in `be38b26` / `8e38cd8`. Current bundle is Metal-only via Homebrew refresh.

## What works today

| Path | Backend | Startup | Notes |
|------|---------|---------|-------|
| Default | Metal GPU | ~2–5 s prewarm | `refresh-whisper-server` copies Homebrew build |
| Fallback | CPU (ggml-cpu plugin) | Same | Metal plugin missing on some configs |
| **Not shipped** | CoreML / ANE | ~60 s dyld hang | Blocked |

## Approved mitigation (do not regress)

1. **Never** ship a default `libwhisper.1.dylib` that links `libwhisper.coreml.dylib`.
2. **Preflight guard** — `preflight.mjs` and `refresh-whisper-server.mjs` fail if CoreML
   appears in `otool -L` output for bundled whisper dylibs.
3. **Optional sidecar** — when phase 2 resumes, ship `whisper-server-coreml-aarch64-apple-darwin`
   as a **separate binary** selected only when:
   - User enables a future `whisper.coreml_enabled` setting, and
   - Matching `<model>-encoder.mlmodelc` exists beside the active `.bin`.
4. **Do not** replace the Metal default sidecar with a CoreML build.

The 60 s hang would then affect only users who explicitly opt in — acceptable for an
experimental path, unacceptable as the default.

## Phase 2 checklist (when resumed)

- [ ] Build CoreML sidecar without linking CoreML into the Metal default dylibs
- [ ] Download flow for precomputed `.mlmodelc` (HF hosts `ggml-large-v3-turbo-encoder.mlmodelc.zip`, ~1.17 GB)
- [ ] Bench: CoreML-active vs Metal-active median wall time, 5+ utterances
- [ ] Verify dyld init on opt-in sidecar ≤ acceptable threshold (or document one-time ANE compile cost separately from dyld hang)
- [ ] Packaged fallback: rename `.mlmodelc` → dictation still works via Metal

## References

- `tasks/deferred/TASK-48-coreml-neural-engine-backend.md`
- `docs/reference/KNOWN-BUG-CLASSES.md` → `CoreML-dyld-init-hang`
- `docs/INVESTIGATION-LOG.md` — 2026-05-09 CoreML build entries; 2026-05-26 dyld mitigation
- Commits: `e81fb11` (build), `be38b26` (revert hang), `8e38cd8` (cleanup dylibs)
