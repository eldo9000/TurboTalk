# TurboTalk — Truth Ledger

What this project can honestly claim today. Updated when a claim changes.

**Operating model tier:** Tier 1 (small app, obvious behavior, personal-use scope). See `~/Downloads/Github/Business-OS/bin/SOFTWARE-DEVELOPMENT-OPERATING-MODEL.md` §15.

---

## What works end-to-end

Nothing. Repo is block-out only. No working build.

## What is partially proven

Nothing.

## What is explicitly not working

Everything. No code has been written. The architecture is decided; nothing is implemented.

## Support claims that are FORBIDDEN until stronger evidence exists

- "TurboTalk transcribes voice." (No transcription code exists.)
- "TurboTalk works on macOS." (Nothing runs.)
- "Push-to-talk works." (No hotkey code exists.)
- "It pastes into apps." (No paste code exists.)
- Any claim about latency, accuracy, or quality.

## Current strategic bet

**Reference, not fork.** Build from scratch using Handy / typr / sagascript as references. Bet: a small, fully-owned codebase with the Chaperone Layer baked in beats a forked codebase that has to be retrofitted. Reversal trigger: if the Tauri scaffold + first end-to-end loop takes more than three sessions, fork Handy instead.

## Promotion criteria

TurboTalk is a personal-use tool, not a Libre product. Promotion to Libre product happens only if:
- Eldo uses it daily for two consecutive weeks, AND
- It demonstrably works for at least one non-Eldo person, AND
- The Chaperone Layer pattern proves out and is worth shipping

Until then: private repo, no public claims, no marketing surface.
