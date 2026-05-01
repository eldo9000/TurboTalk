# @libre/ui — Component Reference

All components are exported from `@libre/ui`. Import what you need:

```js
import { WindowFrame, Titlebar, TabBar, ProgressBar, Button } from '@libre/ui';
```

---

## Layout

### `WindowFrame`

Root wrapper for all Libre app windows. Initialises the theme on mount.
Must be the outermost element — the window chrome (rounded corners, shadow)
is applied by `tokens.css` to `#app > div:first-child`.

```svelte
<WindowFrame>
  <Titlebar>...</Titlebar>
  <main class="flex-1 min-h-0">...</main>
</WindowFrame>
```

**Props:** none (accepts extra HTML attributes and forwards them to the div — useful for `ondragover` etc.)

**Reference:** [ghost/src/App.svelte](../ghost/src/App.svelte), [prism/src/App.svelte](../prism/src/App.svelte), [fade/src/App.svelte](../fade/src/App.svelte)

---

### `Titlebar`

Window chrome: top resize strip, drag region, app content slot, and
Windows-style min/max/close controls.

```svelte
<Titlebar>
  <!-- icon + title or nav controls here -->
  <div class="flex items-center gap-2 pl-3" data-tauri-drag-region>
    <span class="text-[13px] font-medium text-[var(--text-primary)]">My App</span>
  </div>
</Titlebar>

<!-- Taller bar for apps with nav controls (e.g. Ghost) -->
<Titlebar height="h-11">
  ...
</Titlebar>
```

**Props:**
- `height` — Tailwind height class. Default: `'h-8'`

**Reference:** [ghost/src/App.svelte](../ghost/src/App.svelte), [prism/src/App.svelte](../prism/src/App.svelte)

---

## Navigation

### `TabBar`

Horizontal content tabs with active underline indicator and keyboard navigation
(left/right arrow keys).

```svelte
<script>
  const tabs = [
    { id: 'image', label: 'Image' },
    { id: 'video', label: 'Video' },
    { id: 'audio', label: 'Audio' },
  ];
  let active = 'image';
</script>

<TabBar {tabs} {active} onSelect={(id) => active = id} />
```

**Props:**
- `tabs` — `{ id: string, label: string }[]`
- `active` — id of the active tab
- `onSelect` — `(id: string) => void`
- `class` — extra classes for the container

**Reference:** [fade/src/App.svelte](../fade/src/App.svelte)

---

## Feedback

### `ProgressBar`

Progress indicator with ARIA `progressbar` role.

```svelte
<!-- Basic -->
<ProgressBar value={42} />

<!-- Thin variant (per-item in a list) -->
<ProgressBar value={item.percent} height="h-0.5" />

<!-- Success state -->
<ProgressBar value={100} variant="success" />

<!-- Error state -->
<ProgressBar value={job.percent} variant="error" label="Conversion progress" />
```

**Props:**
- `value` — `number` 0–100
- `height` — Tailwind height class. Default: `'h-1.5'`
- `variant` — `'default' | 'success' | 'error'`. Default: `'default'`
- `label` — aria-label. Default: `'Progress'`
- `animated` — Whether to animate width changes. Default: `true`

**Reference:** [fade/src/App.svelte](../fade/src/App.svelte)

---

### `Toast`

Ephemeral notification card. Positioned bottom-right, animated in with fadeIn.
Use this when you need full control over the content.

```svelte
{#if toast}
  <Toast>
    <svg ...></svg>
    <div>
      <div class="font-medium">{toast.message}</div>
      <div class="text-[11px] text-[var(--text-secondary)]">{toast.detail}</div>
    </div>
  </Toast>
{/if}
```

**Props:**
- `class` — extra classes

**Reference:** [ghost/src/App.svelte](../ghost/src/App.svelte) (inline pattern, pre-Toaster)

---

### `Toaster`

Singleton toast manager. Place once in your `WindowFrame`. Call `show()` from
anywhere via a `bind:this` reference.

```svelte
<script>
  import { WindowFrame, Titlebar, Toaster } from '@libre/ui';
  let toaster;

  async function save() {
    await saveFile();
    toaster.show({ message: 'Saved', detail: 'my-file.md', variant: 'success' });
  }
</script>

<WindowFrame>
  <Toaster bind:this={toaster} />
  <Titlebar>...</Titlebar>
  ...
</WindowFrame>
```

**Methods:**
- `show(toast, duration?)` — Show a toast. `toast`: `{ message: string, detail?: string, variant?: 'default'|'success'|'error' }`. `duration` defaults to 4000ms.
- `dismiss()` — Dismiss immediately.

---

## Overlays & interaction

### `Dialog`

Modal dialog with backdrop, focus trap, and escape-to-close.

```svelte
<script>
  let showDialog = false;
</script>

<Button onclick={() => showDialog = true}>Open</Button>

<Dialog bind:open={showDialog} title="Confirm Delete" onclose={() => showDialog = false}>
  <p>Are you sure you want to delete this file?</p>
  {#snippet footer()}
    <Button variant="secondary" onclick={() => showDialog = false}>Cancel</Button>
    <Button onclick={handleDelete}>Delete</Button>
  {/snippet}
</Dialog>
```

**Props:**
- `open` — bindable boolean
- `title` — dialog heading string
- `size` — `'sm' | 'md' | 'lg'`. Default: `'md'`
- `onclose` — callback fired on close

**Slots:** `children` (body), `footer` (snippet — action buttons)

**Keyboard:** `Esc` closes; `Tab` traps focus inside the panel.

---

### `Menu`

Dropdown / context menu anchored to a trigger element.

```svelte
<script>
  let menuOpen = false;
  let btnEl;

  const items = [
    { label: 'Rename',  onclick: rename },
    { label: 'Duplicate', onclick: duplicate },
    { separator: true },
    { label: 'Delete',  onclick: deleteItem, disabled: !canDelete },
  ];
</script>

<button bind:this={btnEl} onclick={() => menuOpen = !menuOpen}>Options</button>
<Menu bind:open={menuOpen} anchor={btnEl} {items} />
```

**Props:**
- `open` — bindable boolean
- `anchor` — `HTMLElement | null`. Menu positions below this element.
- `x` / `y` — fixed position (px) when `anchor` is null
- `items` — `{ label, icon?, onclick, disabled?, separator? }[]`

**Keyboard:** `ArrowDown/Up` navigate; `Enter/Space` activate; `Esc` closes; `Home/End` jump to first/last.

---

### `Tabs`

Tabbed container — wraps `TabBar` with associated content panels.

```svelte
<script>
  let active = 'editor';
</script>

<Tabs bind:activeTab={active} tabs={[
  { id: 'editor',  label: 'Editor',  panel: editorPanel  },
  { id: 'preview', label: 'Preview', panel: previewPanel },
]} />

{#snippet editorPanel()}  <CodeMirrorEditor /> {/snippet}
{#snippet previewPanel()} <MarkdownPreview /> {/snippet}
```

**Props:**
- `tabs` — `{ id: string, label: string, panel: Snippet }[]`
- `activeTab` — bindable string (id of active tab)
- `orientation` — `'horizontal' | 'vertical'`. Default: `'horizontal'`
- `class` — extra classes on container

**Note:** Uses `TabBar` internally for the horizontal case. Vertical mode renders its own tab list with `ArrowUp/Down` navigation.

---

### `Tooltip`

Hover/focus tooltip wrapping any single child element.

```svelte
<Tooltip content="Save file (Ctrl+S)">
  <Button onclick={save}>Save</Button>
</Tooltip>

<Tooltip content="Open settings" placement="bottom" delay={200}>
  <IconButton label="Settings" onclick={openSettings}>
    <svg .../>
  </IconButton>
</Tooltip>
```

**Props:**
- `content` — tooltip text string
- `placement` — `'top' | 'bottom' | 'left' | 'right'`. Default: `'top'`
- `delay` — ms before appearing. Default: `500`

**Accessibility:** Injects `aria-describedby` on the trigger; tooltip uses `role="tooltip"`.

---

### `Input`

Labeled text input with error state and optional leading icon.

```svelte
<!-- Basic -->
<Input bind:value={name} label="Display name" placeholder="Jane Doe" />

<!-- With validation error -->
<Input bind:value={email} label="Email" type="email" error={emailError} />

<!-- With leading icon -->
<Input bind:value={query} label="Search" placeholder="Search files...">
  {#snippet icon()}
    <svg width="14" height="14" .../>
  {/snippet}
</Input>
```

**Props:**
- `value` — bindable string
- `label` — visible label (also used for screen readers)
- `placeholder` — input placeholder text
- `type` — HTML input type. Default: `'text'`
- `error` — error message string; sets `aria-invalid` and renders error text
- `disabled` — boolean. Default: `false`
- `icon` — Snippet. Optional leading icon inside the input field.
- `id` — explicit element id (auto-generated if omitted)
- `onchange` / `oninput` — event handlers

---

## Actions

### `Button`

Standard button with three variants. Uses `--accent` token.

```svelte
<!-- Primary (default) -->
<Button onclick={save}>Save</Button>

<!-- Secondary -->
<Button variant="secondary" onclick={cancel}>Cancel</Button>

<!-- Ghost -->
<Button variant="ghost" onclick={openSettings}>Settings</Button>

<!-- Disabled -->
<Button disabled={converting}>Convert</Button>
```

**Props:**
- `variant` — `'primary' | 'secondary' | 'ghost'`. Default: `'primary'`
- `disabled` — boolean. Default: `false`
- `type` — HTML button type. Default: `'button'`
- `class` — extra classes
- `onclick` — click handler

---

### `IconButton`

Icon-only button. `label` is required for screen reader accessibility.

```svelte
<IconButton label="Close" onclick={close}>
  <svg width="14" height="14" viewBox="0 0 24 24" ...>
    <line x1="18" y1="6" x2="6" y2="18"/>
    <line x1="6" y1="6" x2="18" y2="18"/>
  </svg>
</IconButton>

<!-- Danger variant (red hover) -->
<IconButton label="Delete file" variant="danger" onclick={deleteFile}>
  <svg ...></svg>
</IconButton>
```

**Props:**
- `label` — aria-label (required)
- `title` — tooltip. Defaults to `label`
- `size` — Tailwind size classes. Default: `'w-8 h-8'`
- `variant` — `'default' | 'danger'`. Default: `'default'`
- `disabled` — boolean
- `class` — extra classes
- `onclick` — click handler

---

## Layout helpers

### `ScrollArea`

Scrollable container with consistent Libre scrollbar styling. Suitable for
flex column layouts where overflow must be contained.

```svelte
<!-- Vertical scroll (default) -->
<ScrollArea class="flex-1">
  <div>...long content...</div>
</ScrollArea>

<!-- Horizontal scroll -->
<ScrollArea direction="x">
  <div class="flex gap-2 w-max">...</div>
</ScrollArea>
```

**Props:**
- `direction` — `'y' | 'x' | 'both'`. Default: `'y'`
- `class` — extra classes

---

## API utilities

### `call<T>(cmd, args?)` — `src/api/commands.ts`

Generic typed invoke wrapper. The base for all command calls.

```ts
import { call } from '@libre/ui/src/api/commands.ts';

const files = await call<FileEntry[]>('list_dir', { path: '/home/user' });
```

### `getTheme()` / `getAccent()`

Core commands available in every Libre app.

```ts
import { getTheme, getAccent } from '@libre/ui/src/api/commands.ts';

const theme = await getTheme();   // 'dark' | 'light' | 'system'
const accent = await getAccent(); // '#0066cc'
```

### `openFileDialog(options?)` / `saveFileDialog(options?)` — `src/api/dialogs.ts`

Typed file dialog wrappers. Requires the Tauri backend to expose
`open_file_dialog` and `save_file_dialog` commands.

```ts
import { openFileDialog, saveFileDialog } from '@libre/ui/src/api/dialogs.ts';

const path = await openFileDialog();
const dest = await saveFileDialog({ defaultName: 'untitled.md' });
```

---

## Design tokens — `src/tokens.css`

Import in every app's `app.css`:

```css
@import "@libre/ui/src/tokens.css";
```

| Token | Light | Dark |
|---|---|---|
| `--accent` | `#0066cc` | `#0066cc` |
| `--accent-hover` | `#1e5fa6` | `#1e5fa6` |
| `--surface` | `#ffffff` | `#111827` |
| `--surface-raised` | `#f5f5f5` | `#1f2937` |
| `--titlebar-bg` | `#f7f7f7` | `#1f2937` |
| `--border` | `#e5e7eb` | `#374151` |
| `--text-primary` | `#111827` | `#f9fafb` |
| `--text-secondary` | `#6b7280` | `#9ca3af` |

Also includes: base reset, focus ring, scrollbar styles, drag region,
window chrome (rounded corners + shadow), and `animate-fade-in` keyframe.

---

## Scaffolding

Create a new app from the correct template:

```bash
# Run from apps/
node common-js/scripts/create-libre-app.js my-new-app

cd my-new-app
npm install
npm run tauri dev
```
