# TASK-39: Main-window zoom layout stabilization

## Goal
Make TurboTalk's main window layout stable at every supported in-app zoom level, using 125% as the visual anchor.

Today 125% looks correct across pages. At other zoom levels, several pages are slightly off: content lands a few pixels short or tall, and scrollbars appear intermittently. Fix the window sizing and measurement model so History, Models, Modes, Settings, and Advanced Modes remain visually aligned at 100%, 125%, 150%, 175%, and 200%.

This is a UI polish task, but the expected fix is in sizing math, not a page-by-page padding tweak.

## Current user-visible symptom
- At 125%, every page looks good.
- Zooming down or up makes the UI slightly misfit on different pages.
- Scrollbars appear at various times when the page should fit.
- 125% should be treated as the correct anchor point. Do not redesign the 125% look.

The user currently has the in-app browser open at:

`http://localhost:1428/`

The real app is a Tauri window, so browser-only inspection may not fully exercise `getCurrentWindow().setSize(...)`, but it is useful for frontend DOM/CSS checking once Tauri internals are available or mocked.

## Investigation findings so far
Primary file: `src/App.svelte`.

The main issue is likely the coupling between CSS zoom and Tauri logical window resizing:

- `document.documentElement.style.zoom = ...` is applied at `src/App.svelte` around the zoom effect.
- The window is also resized with `getCurrentWindow().setSize(new LogicalSize(Math.ceil(w * zoom), Math.ceil(h * zoom)))`.
- Natural tab heights are measured once on mount, then reused for every later zoom level.
- The measurement baseline is not explicitly normalized to the visually correct 125% anchor.

Relevant anchors:

- `src/App.svelte:229-236` — stored per-tab measured heights:
  - `settingsH`
  - `modesH`
  - `modelsTabH`
  - `settingsTabH`
- `src/App.svelte:238-245` — measurement refs and hardcoded `SETTINGS_CHROME_H = 68`
- `src/App.svelte:255-270` — sizing effect that multiplies width/height by zoom and calls `setSize`
- `src/App.svelte:274-285` — supported zoom levels and CSS `style.zoom`
- `src/App.svelte:607-654` — one-time natural-height measurement sequence
- `src/App.svelte:789` — root container switches to `h-full overflow-hidden`
- `src/App.svelte:839-878` — titlebar/tabs chrome
- `src/App.svelte:909` — History intended inner scroll area
- `src/App.svelte:1025` — Models intended inner scroll area
- `src/App.svelte:1143` — Modes intended inner scroll/flex area
- `src/App.svelte:1223` — Advanced Modes right-column intended inner scroll area
- `src/App.svelte:1352` — Settings intended inner scroll area
- `src/App.svelte:1550-1572` — bottom zoom bar

Secondary observation:

`SETTINGS_CHROME_H = 68` is fragile. The comment says this is `h-10 titlebar + h-7 bottom bar`, but that is 40 + 28 = 68 only for those two bars. The rendered page also includes tab/titlebar behavior, scroll containers, browser/Tauri rounding, and zoom-scaled fractional pixels. A 1-3px undercount is enough to create unwanted scrollbars.

## Classification
This looks like a layout measurement/root sizing bug, not an individual page content bug.

Do not start by shaving random padding from Models/Settings/Modes. That may make one zoom level look better while breaking 125%, which the user has explicitly identified as the anchor.

The robust fix should:

- Preserve the 125% appearance.
- Normalize sizing math around 125%.
- Measure actual DOM chrome instead of relying on hardcoded constants.
- Separate "outer window size" from "inner scroll area available height."
- Keep intended inner scroll areas where content genuinely exceeds available space.

## In scope
- `src/App.svelte`
- Small helper functions inside `src/App.svelte` for measuring tab content/chrome
- Optional tiny CSS adjustments in `src/app.css` only if needed for scroll containment or scrollbar gutters
- `SESSION-STATUS.md` update after landing

## Out of scope
- Redesigning the UI
- Changing the zoom levels
- Changing the visual appearance at 125% except for removing unintended scrollbar artifacts
- Backend/Rust changes, unless a Tauri sizing API behavior proves impossible to handle from the frontend
- Onboarding layout, unless it is clearly affected by the same root sizing bug
- Overlay window layout

## Recommended implementation approach

### 1. Treat 125% as the anchor
Introduce an explicit anchor constant:

```js
const ZOOM_ANCHOR = 1.25;
```

The current measurements should be interpreted relative to the anchor, not blindly as 100% CSS pixels.

If keeping CSS `style.zoom`, prefer sizing the Tauri logical window by:

```js
const zoom = ZOOM_LEVELS[zoomIdx] / 100;
const anchorScale = zoom / ZOOM_ANCHOR;
```

Then use the anchor-normalized window size rather than assuming measured CSS pixels map directly to unscaled logical pixels at every zoom.

The exact formula must be verified in the real Tauri window because WebKit/Tauri logical size behavior matters. The success signal is visual and measurable, not theoretical.

### 2. Replace hardcoded Settings chrome measurement
Remove or stop relying on:

```js
const SETTINGS_CHROME_H = 68;
```

Measure actual non-content chrome from the DOM instead. Good options:

- Bind refs to the titlebar and bottom bar, then sum their `getBoundingClientRect().height`.
- Or measure `outerEl.scrollHeight - activeScrollableContent.scrollHeight` during the unconstrained measurement phase.

Prefer explicit refs; they are easier for the next agent to reason about.

Suggested refs:

```js
let titlebarEl = $state(null);
let bottomBarEl = $state(null);
```

Then:

```js
function chromeHeight() {
  return Math.ceil((titlebarEl?.getBoundingClientRect().height ?? 0)
    + (bottomBarEl?.getBoundingClientRect().height ?? 0));
}
```

Use measured chrome when calculating Settings and other exact-fit tab heights.

### 3. Measure tab natural height in a single helper
The current mount code repeats a measure/tick/requestAnimationFrame sequence per tab. Keep the same flow, but centralize it so every tab is measured the same way.

Suggested helper:

```js
async function measureAfterPaint(fn) {
  fn?.();
  await tick();
  await new Promise(r => requestAnimationFrame(r));
  return outerEl ? Math.ceil(outerEl.scrollHeight) : 0;
}
```

For Settings, measure the inner settings content plus measured chrome.

For scrollable tabs, be careful not to measure a constrained `h-full overflow-hidden` state. The current code already delays committing `settingsH` until measurements are done; preserve that idea.

### 4. Add small rounding slack at the end, not as design padding
After computing the target logical size, add a tiny explicit guard band:

```js
const WINDOW_SIZE_SLACK = 2;
```

Apply it once to the computed height before `setSize`. Do not add arbitrary padding inside pages.

This is specifically for fractional WebKit/Tauri rounding. Keep it small.

### 5. Preserve intended inner scrolling
Do not remove `overflow-y-auto` from:

- History list with actual history entries
- Models tab
- Modes tab / Advanced panel
- Settings tab

The bug is unwanted scrollbars appearing when the page should fit, not all scrollbars everywhere.

### 6. Avoid resize loops
The sizing `$effect` currently runs whenever reactive state changes and calls `getCurrentWindow().setSize(...)`.

If the implementation starts measuring after size changes, guard against loops by caching the last requested size:

```js
let lastWindowSize = $state({ w: 0, h: 0 });
```

Only call `setSize` if width or height actually changed.

### 7. Be careful with startup ordering
The current startup flow hides the document with opacity 0, measures multiple tabs, then restores History. Preserve the no-flash behavior.

The measurement order currently is:

1. Modes non-chaperone
2. Modes chaperone wide
3. Models
4. Settings
5. Return to History

That order is acceptable. Refactor it only enough to fix the baseline/chrome issue.

## Verification plan

### Automated checks
Run:

```bash
npm run typecheck
npm run build
```

If frontend-only changes somehow touch generated bindings or backend contracts, also run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

This task should not require backend changes.

### Manual visual proof
Use the real Tauri window if possible:

```bash
npm run tauri dev
```

If port `1428` is already in use, use the existing dev server and launch Tauri without starting another Vite server:

```bash
npm run tauri -- dev --no-dev-server-wait -c '{"build":{"beforeDevCommand":""}}'
```

Then verify:

1. Open History at 100%, 125%, 150%, 175%, 200%.
2. Open Models at 100%, 125%, 150%, 175%, 200%.
3. Open Modes with Simple selected at 100%, 125%, 150%, 175%, 200%.
4. Open Modes with Advanced selected at 100%, 125%, 150%, 175%, 200%.
5. Open Settings at 100%, 125%, 150%, 175%, 200%.

For each page:

- 125% should look unchanged from the current good state.
- There should be no unintended outer/page scrollbar.
- Intended inner scroll areas should still scroll when content actually exceeds the viewport.
- Text should not overlap.
- Buttons and controls should not shift or clip.
- The Advanced Modes two-column width behavior should remain intact.

### Measurement proof
In addition to looking at the window, use a DOM-side proof while the app is running:

For each tab/zoom combination, inspect:

```js
document.documentElement.scrollHeight <= document.documentElement.clientHeight
document.body.scrollHeight <= document.body.clientHeight
document.querySelector('#app')?.scrollHeight <= document.querySelector('#app')?.clientHeight
```

Expected result:

- These should be true for the outer app shell when the tab is meant to fit.
- Inner scroll containers may intentionally have `scrollHeight > clientHeight`; that is fine.

If using the in-app browser without Tauri internals, note that this only validates CSS/DOM behavior. The final proof must include the real Tauri window because `setSize(LogicalSize)` is central to the bug.

## Success signal
- History, Models, Modes, Advanced Modes, and Settings show no unintended outer scrollbar at 100%, 125%, 150%, 175%, and 200%.
- 125% remains the visual anchor and does not regress.
- Any scrollbar that remains is an intentional inner content scrollbar, not the whole app/page drifting by a few pixels.
- `npm run typecheck` passes.
- `npm run build` passes.
- `SESSION-STATUS.md` is updated with the fix and proof.

## Suggested final status wording
After landing, update the top of `SESSION-STATUS.md` with something like:

`Main-window zoom layout drift fixed. 125% preserved as visual anchor; sizing now normalizes zoom against the anchor and measures actual chrome instead of relying on SETTINGS_CHROME_H. Proof: History, Models, Modes, Advanced Modes, and Settings inspected at 100/125/150/175/200 with no unintended outer scrollbars; npm run typecheck and npm run build passed.`

## Notes for the implementing agent
- Read `SESSION-STATUS.md` before starting. It already contains the current zoom investigation state.
- Do not move this file to `tasks/done/` until the proof checklist passes.
- If you discover that the true issue is different from the current hypothesis, update this file or `SESSION-STATUS.md` with the new finding before changing direction.
- The repo is Tier 1. Keep the fix focused and evidence concrete; no heavy investigation ledger is needed unless the failure becomes multi-session or opaque.
