<script>
  import { onMount, tick } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import { currentMonitor, getCurrentWindow } from '@tauri-apps/api/window';
  import { LogicalSize, PhysicalPosition } from '@tauri-apps/api/dpi';
  import { open as openFilePicker } from '@tauri-apps/plugin-dialog';
  import { initTheme } from '@libre/ui/src/theme.js';
  import HistoryTab from './HistoryTab.svelte';
  import ModelsTab from './ModelsTab.svelte';
  import ModesTab from './ModesTab.svelte';
  import Onboarding from './Onboarding.svelte';
  import ErrorToast from './ErrorToast.svelte';
  import TitleBar from './TitleBar.svelte';
  import SettingsTab from './SettingsTab.svelte';
  import AboutModal from './AboutModal.svelte';
  import NoModelPopup from './NoModelPopup.svelte';
  import ResetModal from './ResetModal.svelte';
  import BottomBar from './BottomBar.svelte';
  import { PROMPT_PRESETS, DEFAULT_CLASSIFIER_PROMPT } from './lib/prompts';
  import { KNOWN_FILENAMES, altModelVariant, altModelActive } from './lib/catalog';

  const HOTKEY_KEY_ITEMS_MAC = [
    { category: 'Keyboard' },
    { value: 'option',         label: 'Option ⌥' },
    { value: 'control',        label: 'Control ⌃' },
    { value: 'command',        label: 'Command ⌘' },
    { value: 'shift',          label: 'Shift ⇧' },
    { category: 'Mouse' },
    { value: 'mouse_back',     label: 'Mouse Back' },
    { value: 'mouse_forward',  label: 'Mouse Fwd' },
    { value: 'mouse_middle',   label: 'Mouse Middle' },
    { category: 'Function Keys' },
    { value: 'f13',            label: 'F13' },
    { value: 'f14',            label: 'F14' },
    { value: 'f15',            label: 'F15' },
    { value: 'f16',            label: 'F16' },
    { value: 'f17',            label: 'F17' },
    { value: 'f18',            label: 'F18' },
    { value: 'f19',            label: 'F19' },
  ];

  const HOTKEY_KEY_ITEMS_WIN = [
    { category: 'Keyboard' },
    { value: 'option',         label: 'Alt' },
    { value: 'control',        label: 'Ctrl' },
    { value: 'command',        label: 'Win' },
    { value: 'shift',          label: 'Shift' },
    { category: 'Mouse' },
    { value: 'mouse_back',     label: 'Mouse Back' },
    { value: 'mouse_forward',  label: 'Mouse Fwd' },
    { value: 'mouse_middle',   label: 'Mouse Middle' },
    { category: 'Function Keys' },
    { value: 'f13',            label: 'F13' },
    { value: 'f14',            label: 'F14' },
    { value: 'f15',            label: 'F15' },
    { value: 'f16',            label: 'F16' },
    { value: 'f17',            label: 'F17' },
    { value: 'f18',            label: 'F18' },
    { value: 'f19',            label: 'F19' },
    { value: 'f20',            label: 'F20' },
    { value: 'f21',            label: 'F21' },
    { value: 'f22',            label: 'F22' },
    { value: 'f23',            label: 'F23' },
    { value: 'f24',            label: 'F24' },
  ];

  const hotkeyKeyItems = $derived(
    platform === 'windows' ? HOTKEY_KEY_ITEMS_WIN : HOTKEY_KEY_ITEMS_MAC
  );

  const HISTORY_AUTO_DELETE_ITEMS = [
    { value: 'restart', label: 'On app restart' },
    { value: '1d',      label: 'After 1 day'    },
    { value: '5d',      label: 'After 5 days'   },
    { value: '10d',     label: 'After 10 days'  },
    { value: '30d',     label: 'After 30 days'  },
  ];
  // Typed Rust↔TS contract. `commands.*` are wrappers around
  // `invoke()` whose argument and return shapes are derived from the Rust
  // structs in `src-tauri/src/settings.rs`. Adding/removing/renaming a field
  // there produces a TypeScript error here.
  import { commands } from './bindings.ts';

  // First-launch readiness gate. Set to true on mount and on every window
  // focus if any prerequisite (Accessibility, Input Monitoring, Microphone,
  // model) regresses. Onboarding clears it when all four pass.
  let showOnboarding = $state(true);
  let unsupportedPlatformDismissed = $state(false);

  let platform = $state('macos');

  function defaultHotkeyKey() {
    return platform === 'windows' ? 'right_control' : 'right_option';
  }
  function defaultHotkeyMode() {
    return platform === 'windows' ? 'toggle' : 'hold';
  }

  function hotkeyDisplayName(key) {
    const shared = {
      mouse_back: 'Mouse Back', mouse_forward: 'Mouse Fwd', mouse_middle: 'Mouse Middle',
    };
    if (shared[key]) return shared[key];
    if (platform === 'windows') {
      const win = {
        left_option: 'Left Alt', right_option: 'Right Alt',
        left_control: 'Left Ctrl', right_control: 'Right Ctrl',
        left_command: 'Left Win', right_command: 'Right Win',
        left_shift: 'Left Shift', right_shift: 'Right Shift',
      };
      return win[key] ?? key.toUpperCase();
    }
    const mac = {
      right_option: 'Right Option ⌥', left_option: 'Left Option ⌥',
      right_control: 'Right Control ⌃', left_control: 'Left Control ⌃',
      right_command: 'Right Command ⌘', left_command: 'Left Command ⌘',
      right_shift: 'Right Shift ⇧', left_shift: 'Left Shift ⇧',
    };
    return mac[key] ?? key.toUpperCase();
  }

  async function recheckReadiness() {
    const r = await commands.checkReadiness();
    platform = r.platform ?? 'macos';
    readinessModelPresent = r.model_present;
    const unsupportedPlatform = r.platform === 'linux';
    const downloading = Object.keys(downloadProgress).length > 0;

    // Refresh alt-backend install state so modelConfigured stays accurate
    // after external file deletes or engine switches.
    if (!showOnboarding && cfgBackend !== 'whisper') {
      altModels = await commands.listModelsForFamily(cfgBackend).catch(() => []);
    }

    if (showOnboarding) {
      // Initial mount with the default `showOnboarding = true`. If no gate is
      // actually missing, call completeOnboarding() directly so ONBOARDING_ACTIVE
      // is cleared and the model prewarm fires — without ever showing the wizard.
      const needsOnboarding = r.force_onboarding || !r.ready;
      if (!needsOnboarding && !(unsupportedPlatform && unsupportedPlatformDismissed)) {
        await completeOnboarding();
      }
      return;
    }

    // Don't bounce back to onboarding while a model download is in flight in
    // the main Models tab — model_present() is false until the file lands.
    showOnboarding = (r.force_onboarding || (!r.ready && !downloading))
      && !(unsupportedPlatform && unsupportedPlatformDismissed);
  }

  // Theme — override OS setting with user preference
  let cfgTheme = $state('auto');
  let _themeCleanup = null;

  async function applyTheme(mode) {
    _themeCleanup?.();
    _themeCleanup = null;
    if (mode === 'dark') {
      document.documentElement.classList.add('dark');
    } else if (mode === 'light') {
      document.documentElement.classList.remove('dark');
    } else {
      _themeCleanup = await initTheme(invoke);
    }
  }

  $effect(() => { applyTheme(cfgTheme); });

  let recording    = $state(false);
  let transcribing = $state(false);
  let activeTab    = $state('history');

  let aboutOpen    = $state(false);
  let aboutClosing = $state(false);
  let noModelPopupOpen    = $state(false);
  let noModelPopupClosing = $state(false);

  function closeNoModelPopup() {
    noModelPopupClosing = true;
    setTimeout(() => { noModelPopupOpen = false; noModelPopupClosing = false; }, 250);
  }

  function tryStartRecording() {
    if (!modelConfigured) {
      noModelPopupOpen = true;
      noModelPopupClosing = false;
      return;
    }
    commands.startRecording();
  }
  let resetOpen    = $state(false);
  let resetClosing = $state(false);
  let resetBusy    = $state(false);
  let resetError   = $state('');
  let warmupResetBusy = $state(false);
  let warmupResetMsg  = $state('');

  function closeAbout() {
    aboutClosing = true;
    setTimeout(() => { aboutOpen = false; aboutClosing = false; }, 500);
  }

  function closeReset() {
    if (resetBusy) return;
    resetClosing = true;
    setTimeout(() => { resetOpen = false; resetClosing = false; resetError = ''; }, 500);
  }

  async function resetTurboTalk(deleteModels) {
    resetBusy = true;
    resetError = '';
    const res = await commands.resetTurbotalk(deleteModels);
    resetBusy = false;
    if (res.status === 'error') {
      resetError = res.error;
      return;
    }
    history = [];
    cfgLaunchLogin = false;
    resetOpen = false;
    resetClosing = false;
    showOnboarding = true;
    unsupportedPlatformDismissed = false;
    await recheckReadiness();
  }

  async function clearWarmupCache() {
    warmupResetBusy = true;
    warmupResetMsg = '';
    const res = await commands.resetWarmupCache();
    warmupResetBusy = false;
    if (res.status === 'error') {
      warmupResetMsg = res.error;
      return;
    }
    warmupResetMsg = 'Warmup cleared. Next dictation will show the warm-up overlay.';
    setTimeout(() => {
      if (warmupResetMsg.startsWith('Warmup cleared.')) warmupResetMsg = '';
    }, 4000);
  }

  async function completeOnboarding() {
    await commands.clearForceOnboarding();
    await syncAppStateFromBackend();
    showOnboarding = false;
    await restoreMainWindowSize();
    cfgLaunchLogin = await commands.getLaunchAtLogin();
    const res = await commands.prewarmModel();
    if (res.status === 'error') {
      console.warn('[onboarding] prewarm skipped:', res.error);
    }
    commands.prewarmOllama(); // fire-and-forget — no await
  }

  // History
  let history         = $state([]);
  let copiedTs        = $state(null);
  let transcriptError = $state('');
  let transcriptNotice = $state('');

  // Hallucination-rejected transcript. When the backend detects a
  // garbage transcript and emits `transcription-rejected`, we show the text
  // here with a "⚠ filtered" badge instead of adding it to history or pasting.
  // Cleared when the user dismisses or a new recording starts.
  let filteredEntry = $state(/** @type {{ text: string, reason: string } | null} */(null));

  // Unified backend error channel. Any `ui-error` event arriving from Rust is
  // pushed here and rendered in a small dismissible toast stack. Auto-dismisses
  // after 5s; click to dismiss early.
  let uiErrors  = $state([]);
  let uiErrorId = 0;

  // Models tab
  let cfgModels       = $state([]);
  let cfgModel        = $state('');
  let newModelPath    = $state('');
  // { [modelName: string]: number } — key present = downloading, value = pct 0-99
  let downloadProgress = $state({});
  // Set of model identifiers currently being deleted (path for whisper, id for alt)
  let deletingModels  = $state(/** @type {Set<string>} */ (new Set()));
  // Parakeet model descriptors (loaded from backend on tab open)
  let altModels       = $state(/** @type {import('./bindings').ModelDescriptor[]} */ ([]));

  // Modes tab — Chaperone classifier presets.
  //
  // The Chaperone is a classifier, not a rewriter (see cleanup.rs). It must
  // return one of PROSE / CODE / COMMAND / RAW. Each preset biases the
  // classifier toward different use cases — none of them break the
  // four-token output contract. The four-button row above the prompt
  // textarea highlights the matching preset when the textarea text equals
  // its prompt verbatim, and goes grey on any user edit.

  // Reset button compatibility — restores the Balanced preset.

  let cfgCleanupMode          = $state('regex');
  let cfgStripFillers         = $state(true);
  let cfgAppendPeriod         = $state(false);
  let cfgStripArtifacts       = $state(true);
  let cfgOllamaUrl            = $state('');
  let cfgLlmModel             = $state('');
  let cfgVocabulary           = $state('');
  let cfgAntiVocabulary       = $state('');
  let cfgClassifierPrompt     = $state('');
  let showPromptEditor        = $state(false);
  let modesSaveMsg            = $state('');

  // Ollama setup state (Advanced panel)
  let ollamaReachable         = $state(null);  // null = not yet probed
  let ollamaModelPresent      = $state(null);
  let ollamaModelPartial      = $state(false); // partial blobs detected
  let ollamaPullState         = $state({ inFlight: false, pct: 0, status: '', error: '' });

  // Active preset = the one whose prompt exactly equals the textarea's
  // current content. Edits of any kind drop this back to null, which the
  // button row renders as "all grey, none active".
  let activePresetId = $derived(
    PROMPT_PRESETS.find(p => p.prompt === cfgClassifierPrompt)?.id ?? null
  );

  function applyPreset(id) {
    const prompt = PROMPT_PRESETS.find(p => p.id === id)?.prompt ?? DEFAULT_CLASSIFIER_PROMPT;
    cfgClassifierPrompt = prompt;
    saveModes();
  }

  // Settings tab
  const cfgBin             = 'auto';
  let cfgLaunchLogin       = $state(false);
  let cfgDevice            = $state('default');
  let audioDevices         = $state([]);
  let settingsSaveMsg      = $state('');
  let diagnosticMsg        = $state('');

  function logUi(event, detail = '') {
    commands.logClientEvent(event, detail || null).catch(() => {});
  }

  async function createBugReport() {
    diagnosticMsg = 'Creating report…';
    try {
      const note = bugNote.trim();
      const res = await commands.submitBugReport(note);
      diagnosticMsg = res.uploaded
        ? `Sent report #${res.report_id}. Local copy: ${res.report_path}`
        : `Saved report #${res.report_id}: ${res.report_path}`;
      bugNote = '';
      logUi('bug-report-created', `id=${res.report_id} uploaded=${res.uploaded}`);
      await commands.openLogsFolder();
    } catch (e) {
      diagnosticMsg = `Report failed: ${e}`;
    }
  }

  let bugNote          = $state('');

  let cfgHotkeyKey         = $state('right_option');
  let cfgHotkeyMode        = $state('hold');
  let cfgCancelOnEsc       = $state(true);
  let cfgCancelOnHold      = $state(true);

  let hotkeySide           = $state('right');  // 'left' | 'right'
  let hotkeyKeyPart        = $state('option'); // key name without side prefix, or unsided key (f13–f24, mouse_*)
  let hasLogitechMouse     = $state(false);    // set on mount; drives which warning to show when a mouse hotkey is selected

  function isUnsidedKey(k) {
    return k.startsWith('numpad_') || k.startsWith('mouse_') || /^f\d+$/.test(k);
  }
  function parseHotkeyKey(full) {
    if (!full) return { side: 'right', keyPart: 'option' };
    if (isUnsidedKey(full)) return { side: 'right', keyPart: full };
    const idx = full.indexOf('_');
    return idx === -1 ? { side: 'right', keyPart: full } : { side: full.slice(0, idx), keyPart: full.slice(idx + 1) };
  }
  function applyHotkeyKey() {
    cfgHotkeyKey = isUnsidedKey(hotkeyKeyPart) ? hotkeyKeyPart : `${hotkeySide}_${hotkeyKeyPart}`;
    saveSettings();
  }

  // Tooltip state — not reactive, only used in event handlers
  let tipText = $state('');
  let _tipTarget = null;
  let _hideTimer = null;

  function _onIndicatorOver(e) {
    // Priority: specific button tip > row-level tip
    const btn = e.target?.closest?.('[data-tip]');
    const target = btn ?? null;
    if (target) {
      if (_hideTimer) { clearTimeout(_hideTimer); _hideTimer = null; }
      if (target !== _tipTarget) {
        _tipTarget = target;
        tipText = target.dataset.tip ?? '';
      }
    } else if (_tipTarget) {
      if (!_hideTimer) {
        _hideTimer = setTimeout(() => {
          _tipTarget = null;
          _hideTimer = null;
          tipText = '';
        }, 60);
      }
    }
  }

  function _onIndicatorLeave() {
    if (_hideTimer) { clearTimeout(_hideTimer); _hideTimer = null; }
    _tipTarget = null;
    tipText = '';
  }

  let cfgHistoryAutoDelete = $state('10d');
  let cfgSaveHistory       = $state(true);
  let cfgShowOverlay       = $state(true);
  let cfgOverlaySize       = $state('medium');
  let cfgOverlayPosition   = $state('bottom');
  let cfgCursorDotIndicator   = $state(false);
  let cfgSoundOnStart      = $state(false);
  let cfgSoundOnFinish     = $state(false);
  let cfgSoundOnCancel     = $state(false);
  let cfgSoundOnError      = $state(true);
  let cfgSoundVolume       = $state(0.7);
  let cfgVadEnabled        = $state(true);
  let cfgBackend           = $state('parakeet'); // 'whisper' | 'parakeet'
  let cfgBackendVariant    = $state('');
  let cfgPauseMediaOnDictate = $state(true);
  let readinessModelPresent = $state(false);
  let cfgIdleTimeout       = $state(0);
  let pendingTimeouts = new Set();
  let resizeTimeout = null;

  function resolvedAltVariant() {

    if (cfgBackendVariant) return cfgBackendVariant;
    return cfgBackend === 'parakeet' ? 'tdt-0.6b-v2' : '';
  }

  let modelConfigured = $derived.by(() => {
    if (cfgBackend === 'whisper') {
      return !!cfgModel && cfgModels.includes(cfgModel);
    }
    const v = resolvedAltVariant();
    if (!v) return false;
    const m = altModels.find(x => altModelVariant(x) === v);
    return !!m?.installed;
  });

  async function syncAppStateFromBackend() {
    const [cfg, scanned, r] = await Promise.all([
      commands.getConfig(),
      commands.scanModelsDir(),
      commands.checkReadiness(),
    ]);
    cfgBackend = cfg.backend ?? 'parakeet';
    cfgBackendVariant = cfg.backend_variant ?? '';
    cfgModel = cfg.whisper?.model ?? '';
    cfgModels = [...new Set([...(scanned ?? []), ...(cfg.whisper?.models ?? [])])].filter(Boolean);
    if (cfgModels.length === 0 && cfgModel) cfgModels = [cfgModel];
    if (cfgBackend !== 'whisper') {
      altModels = await commands.listModelsForFamily(cfgBackend).catch(() => []);
    }
    readinessModelPresent = r.model_present;
  }

  // ── Zoom ──────────────────────────────────────────────────────────────────

  const ZOOM_LEVELS = [100, 125, 150, 175, 200];
  let zoomIdx = $state(parseInt(localStorage.getItem('tt-zoom') ?? '0'));

  // Main window: preferred utility size with compact-screen escape hatches.
  const WINDOW_W_DEFAULT = 550;
  const WINDOW_H_DEFAULT = 560;
  const WINDOW_W_MIN = 420;
  const WINDOW_H_MIN = 420;
  const WINDOW_HEIGHT_KEY = 'tt-window-height';
  // Chrome heights (CSS px, unzoomed): titlebar h-10 = 40, bottom bar h-7 = 28.
  // The settings scroll container adds 2px bottom padding (pb-0.5); the last row
  // keeps its own 4px, for a 6px gap below the last button.
  const TITLEBAR_H = 40;
  const BOTTOMBAR_H = 28;
  const CONTENT_BOTTOM_GAP = 2;
  let suppressWindowResizeTrack = false;

  function savedLogicalHeight() {
    const raw = parseInt(localStorage.getItem(WINDOW_HEIGHT_KEY) ?? String(WINDOW_H_DEFAULT), 10);
    return Number.isFinite(raw) ? Math.max(WINDOW_H_MIN, raw) : WINDOW_H_DEFAULT;
  }

  function persistLogicalHeight(logicalH) {
    localStorage.setItem(
      WINDOW_HEIGHT_KEY,
      String(Math.max(WINDOW_H_MIN, Math.round(logicalH))),
    );
  }

  // Single authority for the main window's size. Min scales with the UI zoom;
  // width grows to the zoom-scaled comfortable width and is capped at 2× min.
  // Height is clipped to fit the Settings content exactly (no dead space below
  // the last button) when that tab is open; otherwise it's capped at 2× min.
  // All natural-content math is in CSS px and multiplied by zoom to logical px
  // (verified: windowLogical = contentCss × zoom).
  async function applyWindowSizing() {
    const zoomPct = ZOOM_LEVELS[zoomIdx];
    const zoom = zoomPct / 100;
    const win = getCurrentWindow();
    const scale = await win.scaleFactor().catch(() => 1);
    const monitor = await currentMonitor().catch(() => null);
    // Large fallback so a failed monitor read never caps sizing.
    const workW = monitor?.workArea?.size?.width  ? monitor.workArea.size.width  / scale : 100000;
    const workH = monitor?.workArea?.size?.height ? monitor.workArea.size.height / scale : 100000;

    const minW = Math.round(WINDOW_W_MIN * zoom);
    const minH = Math.round(WINDOW_H_MIN * zoom);
    const defW = Math.round(WINDOW_W_DEFAULT * zoom);
    const maxW = Math.max(minW, Math.min(2 * minW, workW));

    // Height max: fit the Settings content when it's shown; else cap at 2× min.
    let maxH = Math.max(minH, Math.min(2 * minH, workH));
    let fitH = null;
    if (activeTab === 'settings' && settingsContentEl) {
      await tick();
      const contentCss = Array.from(settingsContentEl.children).reduce((a, c) => a + c.offsetHeight, 0);
      if (contentCss) {
        const totalCss = TITLEBAR_H + contentCss + CONTENT_BOTTOM_GAP + BOTTOMBAR_H;
        fitH = Math.max(minH, Math.min(Math.round(totalCss * zoom), Math.round(workH)));
        maxH = fitH;
      }
    }

    await win.setMinSize(new LogicalSize(minW, minH));
    await win.setMaxSize(new LogicalSize(maxW, maxH));

    const size = await win.innerSize().catch(() => null);
    if (!size) return;
    const curW = size.width / scale;
    const curH = size.height / scale;
    const newW = curW < defW - 1 ? defW : Math.min(curW, maxW);
    const newH = fitH != null ? fitH : Math.min(curH, maxH);
    if (Math.abs(newW - curW) > 1 || Math.abs(newH - curH) > 1) {
      suppressWindowResizeTrack = true;
      try {
        await win.setSize(new LogicalSize(newW, newH));
      } finally {
        suppressWindowResizeTrack = false;
      }
      // Resizing may push the window's right/bottom edge off a small screen.
      await nudgeWindowOnScreen();
    }
  }

  // Standard "don't lose the window" safety: keep the window inside the current
  // monitor's work area. Only called right after we grow the window on a zoom
  // change — never on the user's own drags — so it's never disruptive. All math
  // in physical pixels (outer bounds + work area are both physical).
  async function nudgeWindowOnScreen() {
    const win = getCurrentWindow();
    const [pos, size, monitor] = await Promise.all([
      win.outerPosition().catch(() => null),
      win.outerSize().catch(() => null),
      currentMonitor().catch(() => null),
    ]);
    const wa = monitor?.workArea;
    if (!pos || !size || !wa) return;
    const workRight  = wa.position.x + wa.size.width;
    const workBottom = wa.position.y + wa.size.height;
    // Furthest top-left that still fits; if the window is larger than the work
    // area, pin to the top-left corner so the title bar stays reachable.
    const maxX = Math.max(wa.position.x, workRight  - size.width);
    const maxY = Math.max(wa.position.y, workBottom - size.height);
    const x = Math.min(Math.max(pos.x, wa.position.x), maxX);
    const y = Math.min(Math.max(pos.y, wa.position.y), maxY);
    if (x !== pos.x || y !== pos.y) {
      await win.setPosition(new PhysicalPosition(x, y)).catch(() => {});
    }
  }

  async function applyWindowSizeLimits() {
    const win = getCurrentWindow();
    await win.setResizable(true);
    await win.setMaximizable(false);
    await applyWindowSizing();
  }

  async function applyWindowSizeFromPrefs() {
    const win = getCurrentWindow();
    const monitor = await currentMonitor().catch(() => null);
    const scale = monitor?.scaleFactor ?? await win.scaleFactor().catch(() => 1);
    const zoomPct = ZOOM_LEVELS[zoomIdx];
    const scaledMinW = Math.round(WINDOW_W_MIN * zoomPct / 100);
    const scaledMinH = Math.round(WINDOW_H_MIN * zoomPct / 100);
    const scaledDefaultW = Math.round(WINDOW_W_DEFAULT * zoomPct / 100);
    const maxW = monitor?.workArea?.size?.width ? monitor.workArea.size.width / scale : scaledDefaultW;
    const maxH = monitor?.workArea?.size?.height ? monitor.workArea.size.height / scale : savedLogicalHeight();
    const w = Math.min(scaledDefaultW, Math.max(scaledMinW, maxW));
    const h = Math.min(savedLogicalHeight(), Math.max(scaledMinH, maxH));
    suppressWindowResizeTrack = true;
    try {
      await win.setSize(new LogicalSize(w, h));
    } finally {
      suppressWindowResizeTrack = false;
    }
  }

  async function restoreMainWindowSize() {
    await applyWindowSizeLimits();
    await applyWindowSizeFromPrefs();
  }

  // Persist the user's chosen height on resize — no clamping or snapping. The
  // OS already prevents resizing below the min size set in applyWindowSizing.
  async function trackWindowHeight() {
    if (showOnboarding || suppressWindowResizeTrack) return;
    const win = getCurrentWindow();
    const size = await win.innerSize().catch(() => null);
    const factor = await win.scaleFactor().catch(() => 1);
    if (size) persistLogicalHeight(size.height / factor);
  }

  $effect(() => {
    if (showOnboarding) return;
    void applyWindowSizeLimits();
  });

  $effect(() => {
    const zoomPct = ZOOM_LEVELS[zoomIdx];
    document.documentElement.style.zoom = `${zoomPct}%`;
    localStorage.setItem('tt-zoom', String(zoomIdx));
    void applyWindowSizing();
  });



  function zoomIn()  { if (zoomIdx < ZOOM_LEVELS.length - 1) zoomIdx++; }
  function zoomOut() { if (zoomIdx > 0) zoomIdx--; }

  let outerEl = $state(null);
  let settingsContentEl = $state(null);

  // ── History ───────────────────────────────────────────────────────────────

  async function clearHistory() {
    history = [];
    await commands.saveHistory([]);
  }

  async function copyHistoryItem(item) {
    copiedTs = item.ts;
    const res = await commands.copyHistoryItem(item.text);
    if (res.status === 'error') {
      transcriptError = 'Copy failed: ' + res.error;
      const t = window.setTimeout(() => { transcriptError = ''; pendingTimeouts.delete(t); }, 4000);
      pendingTimeouts.add(t);
    }
    const ts = window.setTimeout(() => { if (copiedTs === item.ts) copiedTs = null; pendingTimeouts.delete(ts); }, 1500);
    pendingTimeouts.add(ts);
  }

  async function setTranscriptionEngine(v) {
    cfgBackend = v;
    await saveSettings();
    await syncAppStateFromBackend();
    if (activeTab === 'models') {
      await openModels();
    }
  }

  async function openModels() {
    if (cfgBackend !== 'whisper') {
      altModels = await commands.listModelsForFamily(cfgBackend).catch(() => []);
    }
  }

  async function startAltDownload(m) {
    downloadProgress = { ...downloadProgress, [m.id]: 0 };
    const variant = altModelVariant(m);
    const res = await commands.downloadParakeetModel(variant);
    const { [m.id]: _removed, ...rest } = downloadProgress;
    downloadProgress = rest;
    if (res.status === 'ok') {
      cfgBackendVariant = altModelVariant(m);
      altModels = await commands.listModelsForFamily(cfgBackend).catch(() => []);
      await syncAppStateFromBackend();
    }
  }

  async function selectAltModel(m) {
    const variant = altModelVariant(m);
    cfgBackendVariant = variant;
    const res = await commands.saveConfig(buildFullConfig());
    if (res.status === 'ok') {
      altModels = await commands.listModelsForFamily(cfgBackend).catch(() => []);
      await syncAppStateFromBackend();
    }
  }

  async function removeAltModel(m) {
    const variant = altModelVariant(m);
    deletingModels = new Set([...deletingModels, m.id]);
    try {
      const res = await commands.deleteBackendModel(cfgBackend, variant);
      if (res.status === 'error') return;
      if (altModelActive(m, cfgBackendVariant, cfgBackend)) cfgBackendVariant = '';
      altModels = await commands.listModelsForFamily(cfgBackend).catch(() => []);
      await syncAppStateFromBackend();
    } finally {
      const next = new Set(deletingModels);
      next.delete(m.id);
      deletingModels = next;
    }
  }

  function cancelAltDownload(m) {
    commands.cancelDownload(m.id);
    const { [m.id]: _removed, ...rest } = downloadProgress;
    downloadProgress = rest;
  }

  function buildFullConfig() {
    return {
      whisper: { bin: cfgBin, model: cfgModel, models: cfgModels, vad_enabled: cfgVadEnabled },
      audio: { device: cfgDevice, idle_timeout_secs: cfgIdleTimeout },
      hotkey: { key: cfgHotkeyKey, mode: cfgHotkeyMode, cancel_on_esc: cfgCancelOnEsc, cancel_on_hold: cfgCancelOnHold },
      theme: cfgTheme,
      history_auto_delete: cfgHistoryAutoDelete,
      save_history: cfgSaveHistory,
      show_overlay: cfgShowOverlay,
      overlay_size: cfgOverlaySize,
      overlay_position: cfgOverlayPosition,
      cursor_dot_indicator: cfgCursorDotIndicator,
      sound_on_start: cfgSoundOnStart,
      sound_on_finish: cfgSoundOnFinish,
      sound_on_cancel: cfgSoundOnCancel,
      sound_on_error: cfgSoundOnError,
      sound_volume: cfgSoundVolume,
      backend: cfgBackend,
      backend_variant: cfgBackendVariant,
      pause_media_on_dictate: cfgPauseMediaOnDictate,
      cleanup: {
        mode: cfgCleanupMode,
        strip_fillers: cfgStripFillers,
        append_period: cfgAppendPeriod,
        strip_whisper_artifacts: cfgStripArtifacts,
        ollama_url: cfgOllamaUrl,
        classifier_model: cfgLlmModel,
        vocabulary: cfgVocabulary ? cfgVocabulary.split('\n').map(s => s.trim()).filter(Boolean) : [],
        antivocabulary: cfgAntiVocabulary ? cfgAntiVocabulary.split('\n').map(s => s.trim()).filter(Boolean) : [],
        classifier_prompt: cfgClassifierPrompt,
      },
    };
  }

  async function setCustomModel(path) {
    const trimmed = path.trim();
    if (!trimmed || cfgModels.includes(trimmed)) return;
    // Replace any existing custom slot; keep catalog entries
    cfgModels = [...cfgModels.filter(p => KNOWN_FILENAMES.some(fn => p.endsWith(fn))), trimmed];
    cfgModel = trimmed;
    newModelPath = '';
    await saveModels();
  }

  async function browseCustomModel() {
    const picked = await openFilePicker({ filters: [{ name: 'Whisper model', extensions: ['bin'] }] });
    if (picked) await setCustomModel(picked);
  }

  async function removeModel(path) {
    // Best-effort file delete. Backend skips silently for safe cases (file
    // gone, custom path outside models dir); only genuine failures (permission,
    // bad extension) come back as errors. Either way we still drop the entry
    // from the user's config so the row disappears from the UI.
    deletingModels = new Set([...deletingModels, path]);
    try {
      await commands.deleteModelFile(path);
      cfgModels = cfgModels.filter(m => m !== path);
      if (cfgModel === path) cfgModel = '';
      await saveModels();
    } finally {
      const next = new Set(deletingModels);
      next.delete(path);
      deletingModels = next;
    }
  }

  async function saveModels() {
    await commands.saveConfig(buildFullConfig());
  }

  async function startDownload(m) {
    downloadProgress = { ...downloadProgress, [m.name]: 0 };
    const res = await commands.downloadModel(m.name);
    const { [m.name]: _removed, ...rest } = downloadProgress;
    downloadProgress = rest;
    if (res.status === 'error') {
      const id = ++uiErrorId;
      uiErrors = [...uiErrors, { id, kind: 'download-error', message: res.error, recoverable: true }];
      const td = window.setTimeout(() => { uiErrors = uiErrors.filter(x => x.id !== id); pendingTimeouts.delete(td); }, 5000);
      pendingTimeouts.add(td);
      return;
    }
    const path = res.data;
    if (!cfgModels.includes(path)) {
      cfgModels = [...cfgModels, path];
      if (!cfgModel) cfgModel = path;
    }
    await saveModels();
  }

  async function selectModel(path) {
    cfgModel = path;
    await saveModels();
  }

  // ── Modes ─────────────────────────────────────────────────────────────────

  async function openModes() {
    modesSaveMsg = '';
  }

  async function saveModes() {
    const res = await commands.saveConfig(buildFullConfig());
    modesSaveMsg = res.status === 'ok' ? 'Saved.' : 'Error: ' + res.error;
  }

  async function handleModeClick(v) {
    cfgCleanupMode = v;
    saveModes();
    if (v === 'chaperone') commands.prewarmOllama(); // fire-and-forget
  }

  // ── Ollama setup helpers ───────────────────────────────────────────────────

  async function refreshOllamaSetup() {
    const ping = await commands.pingOllama();
    if (ping.status === 'error') {
      ollamaReachable = false;
      ollamaModelPresent = null;
      ollamaModelPartial = false;
      return;
    }
    ollamaReachable = ping.data.reachable;
    if (ping.data.reachable) {
      const model = cfgLlmModel || 'llama3.2:3b';
      const res = await commands.checkOllamaModel(model);
      ollamaModelPresent = res.status === 'ok' ? res.data : false;
      ollamaModelPartial = ollamaModelPresent
        ? await commands.checkOllamaPartialBlobs()
        : false;
    } else {
      ollamaModelPresent = null;
      ollamaModelPartial = false;
    }
  }

  async function startOllamaPull() {
    const model = cfgLlmModel || 'llama3.2:3b';
    ollamaPullState = { inFlight: true, pct: 0, status: 'starting…', error: '' };
    try {
      const res = await commands.pullOllamaModel(model);
      if (res.status === 'ok') {
        ollamaPullState = { inFlight: false, pct: 100, status: 'success', error: '' };
        await refreshOllamaSetup();
        commands.prewarmOllama(); // fire-and-forget — warm immediately after download
      } else {
        const msg = String(res.error ?? 'unknown error');
        console.error('[ollama-pull] error:', msg);
        ollamaPullState = { inFlight: false, pct: 0, status: '', error: msg };
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      console.error('[ollama-pull] threw:', msg);
      ollamaPullState = { inFlight: false, pct: 0, status: '', error: msg };
    }
  }

  async function installOllama() {
    const res = await commands.openUrl('https://ollama.com/download');
    if (res.status === 'error') {
      uiErrors = [...uiErrors, { kind: 'open-url-error', message: `Could not open browser: ${res.error}`, recoverable: false }];
    }
  }

  // Polling effect: runs when Advanced panel is visible
  $effect(() => {
    const isAdv = activeTab === 'modes' && cfgCleanupMode === 'chaperone';
    if (!isAdv) return;

    refreshOllamaSetup();

    const interval = setInterval(() => {
      if (!ollamaPullState.inFlight) {
        refreshOllamaSetup();
      }
    }, 5000);

    const onWindowFocus = () => { refreshOllamaSetup(); };
    window.addEventListener('focus', onWindowFocus);

    return () => {
      clearInterval(interval);
      window.removeEventListener('focus', onWindowFocus);
    };
  });

  // ── Settings ──────────────────────────────────────────────────────────────

  async function openSettings(loadAudioDevices = true) {
    const [cfg, devs, launch] = await Promise.all([
      commands.getConfig(),
      loadAudioDevices ? commands.listAudioDevices() : Promise.resolve(audioDevices),
      commands.getLaunchAtLogin(),
    ]);
    cfgDevice            = cfg.audio?.device                   ?? 'default';
    cfgIdleTimeout       = cfg.audio?.idle_timeout_secs         ?? 0;
    cfgHotkeyKey         = cfg.hotkey?.key                     ?? defaultHotkeyKey();
    cfgHotkeyMode        = cfg.hotkey?.mode                    ?? defaultHotkeyMode();
    cfgCancelOnEsc       = cfg.hotkey?.cancel_on_esc            ?? true;
    cfgCancelOnHold      = cfg.hotkey?.cancel_on_hold           ?? true;
    const parsed         = parseHotkeyKey(cfgHotkeyKey);
    hotkeySide           = parsed.side;
    hotkeyKeyPart        = parsed.keyPart;
    cfgHistoryAutoDelete = cfg.history_auto_delete             ?? '10d';
    cfgSaveHistory       = cfg.save_history                    ?? true;
    cfgShowOverlay       = cfg.show_overlay                    ?? true;
    cfgOverlaySize       = cfg.overlay_size                    ?? 'medium';
    cfgOverlayPosition   = cfg.overlay_position                ?? 'bottom';
    cfgCursorDotIndicator   = cfg.cursor_dot_indicator         ?? false;
    cfgSoundOnStart      = cfg.sound_on_start                  ?? false;
    cfgSoundOnFinish     = cfg.sound_on_finish                  ?? false;
    cfgSoundOnCancel     = cfg.sound_on_cancel                  ?? false;
    cfgSoundOnError      = cfg.sound_on_error                   ?? true;
    cfgSoundVolume       = cfg.sound_volume                     ?? 0.7;
    cfgVadEnabled        = cfg.whisper?.vad_enabled             ?? true;
    cfgBackend           = cfg.backend                          ?? 'parakeet';
    cfgBackendVariant    = cfg.backend_variant                   ?? '';
    cfgPauseMediaOnDictate = cfg.pause_media_on_dictate          ?? true;
    cfgLaunchLogin       = launch;
    audioDevices         = devs;
    settingsSaveMsg      = '';
  }

  async function saveSettings() {
    const saveRes = await commands.saveConfig(buildFullConfig());
    if (saveRes.status === 'error') {
      settingsSaveMsg = 'Error: ' + saveRes.error;
      return;
    }
    const launchRes = await commands.setLaunchAtLogin(cfgLaunchLogin);
    settingsSaveMsg = launchRes.status === 'ok' ? 'Saved.' : 'Error: ' + launchRes.error;
    if (launchRes.status === 'ok') {
      logUi('settings-saved', JSON.stringify({
        hotkey: cfgHotkeyKey,
        mode: cfgHotkeyMode,
        backend: cfgBackend,
        overlay: cfgShowOverlay ? cfgOverlaySize : 'off',
      }));
    }
  }

  function historyActions() {
    return {
      toggleRecording: () => {
        if (recording) commands.stopRecording();
        else tryStartRecording();
      },
      clearHistory,
      copyHistoryItem,
      dismissTranscriptError: () => { transcriptError = ''; },
      dismissTranscriptNotice: () => { transcriptNotice = ''; },
      dismissFilteredEntry: () => { filteredEntry = null; },
    };
  }

  function modelsActions() {
    return {
      setTranscriptionEngine,
      startDownload,
      cancelDownload: (name) => commands.cancelDownload(name),
      selectModel,
      removeModel,
      startAltDownload,
      cancelAltDownload,
      selectAltModel,
      removeAltModel,
      browseCustomModel,
      setCustomModel,
      setNewModelPath: (v) => { newModelPath = v; },
    };
  }

  function modesActions() {
    return {
      setCleanupMode: (v) => {
        cfgCleanupMode = v;
        saveModes();
        if (v === 'chaperone') commands.prewarmOllama(); // fire-and-forget
      },
      setStripFillers: (v) => { cfgStripFillers = v; saveModes(); },
      setAppendPeriod: (v) => { cfgAppendPeriod = v; saveModes(); },
      setStripArtifacts: (v) => { cfgStripArtifacts = v; saveModes(); },
      setVadEnabled: (v) => { cfgVadEnabled = v; saveModes(); },
      setVocabulary: (v) => { cfgVocabulary = v; saveModes(); },
      setAntiVocabulary: (v) => { cfgAntiVocabulary = v; saveModes(); },
      setOllamaUrl: (v) => { cfgOllamaUrl = v; saveModes(); },
      setLlmModel: (v) => { cfgLlmModel = v; saveModes(); },
      setClassifierPrompt: (v) => { cfgClassifierPrompt = v; saveModes(); },
      applyPreset,
      resetClassifierPrompt: () => {
        cfgClassifierPrompt = DEFAULT_CLASSIFIER_PROMPT;
        saveModes();
      },
      refreshOllamaSetup,
      startOllamaPull,
      installOllama,
    };
  }

  function switchTab(tab) {
    activeTab = tab;
    if (tab === 'models')   openModels();
    if (tab === 'modes')    openModes();
    if (tab === 'settings') openSettings();
  }

  /**
   * Centralized dispatch for Tauri backend events.
   * Owns all mutations to recording, transcribing, transcriptError, history,
   * uiErrors, filteredEntry, downloadProgress, and ollamaPullState.
   * Each onMount listener is a one-liner → this function.
   */
  function applyBackendEvent(name, payload) {
    switch (name) {
      case 'ptt-down':
        recording = true;
        transcribing = false;
        filteredEntry = null;
        logUi('ptt-down');
        break;

      case 'ptt-up':
        recording = false;
        transcribing = true;
        logUi('ptt-up');
        break;

      case 'download-progress': {
        const pct = payload.pct;
        const name_ = payload.name;
        const altKey = name_.startsWith('parakeet-')
          ? `parakeet-${name_.slice('parakeet-'.length)}`
          : null;
        if (pct >= 100) {
          const next = { ...downloadProgress };
          delete next[name_];
          if (altKey) delete next[altKey];
          downloadProgress = next;
          syncAppStateFromBackend();
        } else {
          const patch = { [name_]: pct };
          if (altKey) patch[altKey] = pct;
          downloadProgress = { ...downloadProgress, ...patch };
        }
        break;
      }

      case 'transcript': {
        recording = false;
        transcribing = false;
        const text = payload;
        logUi('transcript', text ? `${text.length} chars` : 'empty');
        if (text) {
          history = [{ text, ts: Date.now() }, ...history];
          (async () => { await commands.saveHistory(history); })();
        }
        break;
      }

      case 'ui-error': {
        const id = ++uiErrorId;
        const p = payload || {};
        logUi('ui-error', `${p.kind || 'unknown'}: ${p.message || ''}`);
        uiErrors = [...uiErrors, {
          id,
          kind: p.kind || 'unknown',
          message: p.message || 'An error occurred',
          recoverable: p.recoverable !== false,
        }];
        if (p.kind !== 'user-permission-lost') {
          const t1 = window.setTimeout(() => { uiErrors = uiErrors.filter(x => x.id !== id); pendingTimeouts.delete(t1); }, 5000);
          pendingTimeouts.add(t1);
        }
        break;
      }

      case 'transcription-rejected': {
        recording = false;
        transcribing = false;
        const p = payload || {};
        logUi('transcription-rejected', p.reason || 'filtered');
        filteredEntry = { text: p.text || '', reason: p.reason || 'Hallucination detected' };
        if (p.text) {
          history = [{ text: p.text, ts: Date.now(), flaky: p.flaky ?? true }, ...history];
          commands.saveHistory(history);
        }
        if (p.flaky) {
          const id = ++uiErrorId;
          uiErrors = [...uiErrors, {
            id,
            kind: 'transcription-rejected',
            message: `⚠ Flaky: ${p.reason || 'Hallucination detected'} — pasted with warning.`,
            recoverable: true,
          }];
          const t2 = window.setTimeout(() => { uiErrors = uiErrors.filter(x => x.id !== id); pendingTimeouts.delete(t2); }, 8000);
          pendingTimeouts.add(t2);
        }
        break;
      }

      case 'transcript-error': {
        recording = false;
        transcribing = false;
        transcriptError = payload || 'Transcription failed.';
        const t3 = window.setTimeout(() => { transcriptError = ''; pendingTimeouts.delete(t3); }, 5000);
        pendingTimeouts.add(t3);
        break;
      }

      case 'paste-error': {
        recording = false;
        transcribing = false;
        transcriptError = payload || "Couldn't paste — check Accessibility permission";
        const t4 = window.setTimeout(() => { transcriptError = ''; pendingTimeouts.delete(t4); }, 5000);
        pendingTimeouts.add(t4);
        break;
      }

      case 'paste-copied': {
        recording = false;
        transcribing = false;
        transcriptNotice = payload || 'Auto-paste blocked. Copied to clipboard; press Command-V.';
        const t5 = window.setTimeout(() => { transcriptNotice = ''; pendingTimeouts.delete(t5); }, 5000);
        pendingTimeouts.add(t5);
        break;
      }

      case 'focus-changed-before-paste': {
        const p = payload || {};
        const start = p.focus_at_start ?? 'unknown';
        const now = p.focus_at_paste ?? 'unknown';
        transcriptError = `Focus changed: pasted into ${now} (started in ${start}).`;
        const t6 = window.setTimeout(() => { transcriptError = ''; pendingTimeouts.delete(t6); }, 4000);
        pendingTimeouts.add(t6);
        break;
      }

      case 'recording-discarded': {
        logUi('recording-discarded', String(payload ?? ''));
        recording = false;
        transcribing = false;
        if (payload === 'empty-final-text') {
          transcriptError = 'Nothing to paste — try speaking more clearly.';
          const t7 = window.setTimeout(() => { transcriptError = ''; pendingTimeouts.delete(t7); }, 3000);
          pendingTimeouts.add(t7);
        }
        break;
      }

      case 'recording-cancelled':
        logUi('recording-cancelled');
        recording = false;
        transcribing = false;
        break;

      case 'recording-recovered': {
        recording = false;
        transcribing = false;
        const text = payload;
        logUi('recording-recovered', text ? `${text.length} chars` : 'empty');
        if (text) {
          history = [{ text, ts: Date.now() }, ...history];
          (async () => { await commands.saveHistory(history); })();
        }
        break;
      }

      case 'recording-too-short': {
        recording = false;
        transcribing = false;
        const ms = typeof payload === 'number' ? payload : 0;
        transcriptError = ms > 0
          ? `Too short (${ms} ms) — try holding the hotkey a bit longer.`
          : 'Too short — try holding the hotkey a bit longer.';
        const t8 = window.setTimeout(() => { transcriptError = ''; pendingTimeouts.delete(t8); }, 3500);
        pendingTimeouts.add(t8);
        break;
      }

      case 'device-lost':
        recording = false;
        transcribing = false;
        transcriptError = 'Microphone disconnected — pick a different device or reconnect.';
        const t9 = window.setTimeout(() => { transcriptError = ''; pendingTimeouts.delete(t9); }, 5000);
        pendingTimeouts.add(t9);
        break;

      case 'ollama-pull-progress':
        ollamaPullState = { inFlight: true, pct: payload.pct, status: payload.status };
        break;
    }
  }

  onMount(() => {
    let disposed = false;
    const cleanups = [];
    const addCleanup = (cleanup) => {
      if (disposed) {
        cleanup();
        return;
      }
      cleanups.push(cleanup);
    };
    const listenTracked = (eventName, handler) => {
      listen(eventName, handler).then(addCleanup);
    };

    const init = async () => {
      // Load saved theme + history before anything renders
      const [initialCfg, savedHistory] = await Promise.all([
        commands.getConfig(),
        commands.loadHistory(),
      ]);
      if (disposed) return;
      cfgDevice            = initialCfg.audio?.device                   ?? 'default';
      cfgIdleTimeout       = initialCfg.audio?.idle_timeout_secs         ?? 0;
      cfgHotkeyKey         = initialCfg.hotkey?.key                     ?? defaultHotkeyKey();
      cfgHotkeyMode        = initialCfg.hotkey?.mode                    ?? defaultHotkeyMode();
      cfgCancelOnEsc       = initialCfg.hotkey?.cancel_on_esc            ?? true;
      cfgCancelOnHold      = initialCfg.hotkey?.cancel_on_hold           ?? true;
      cfgTheme             = initialCfg.theme                            ?? 'auto';
      cfgHistoryAutoDelete = initialCfg.history_auto_delete             ?? '10d';
      cfgSaveHistory       = initialCfg.save_history                    ?? true;
      cfgShowOverlay       = initialCfg.show_overlay                    ?? true;
      cfgOverlaySize       = initialCfg.overlay_size                    ?? 'medium';
      cfgOverlayPosition   = initialCfg.overlay_position                ?? 'bottom';
      cfgCursorDotIndicator   = initialCfg.cursor_dot_indicator         ?? false;
      cfgSoundOnStart      = initialCfg.sound_on_start                  ?? false;
      cfgSoundOnFinish     = initialCfg.sound_on_finish                  ?? false;
      cfgSoundOnCancel     = initialCfg.sound_on_cancel                  ?? false;
      cfgSoundOnError      = initialCfg.sound_on_error                   ?? true;
      cfgSoundVolume       = initialCfg.sound_volume                     ?? 0.7;
      cfgVadEnabled        = initialCfg.whisper?.vad_enabled             ?? true;
      cfgBackend           = initialCfg.backend                          ?? 'parakeet';
      cfgBackendVariant    = initialCfg.backend_variant                   ?? '';
      cfgPauseMediaOnDictate = initialCfg.pause_media_on_dictate          ?? true;
      cfgModel             = initialCfg.whisper?.model                   ?? '';
      cfgModels            = initialCfg.whisper?.models                  ?? [];
      cfgCleanupMode       = initialCfg.cleanup?.mode                    ?? 'regex';
      cfgStripFillers      = initialCfg.cleanup?.strip_fillers            ?? true;
      cfgAppendPeriod      = initialCfg.cleanup?.append_period            ?? false;
      cfgStripArtifacts    = initialCfg.cleanup?.strip_whisper_artifacts  ?? true;
      cfgOllamaUrl         = initialCfg.cleanup?.ollama_url               ?? '';
      cfgLlmModel          = initialCfg.cleanup?.classifier_model         ?? '';
      cfgVocabulary        = (initialCfg.cleanup?.vocabulary ?? []).join('\n');
      cfgAntiVocabulary    = (initialCfg.cleanup?.antivocabulary ?? []).join('\n');
      cfgClassifierPrompt  = initialCfg.cleanup?.classifier_prompt        ?? DEFAULT_CLASSIFIER_PROMPT;
      if (savedHistory.length) history = savedHistory;

      // Detect Logitech mouse — fast ioreg call, fire-and-forget
      commands.detectLogitechMouse().then(v => { hasLogitechMouse = v; });

      function handleKeydown(e) {
        if (e.metaKey || e.ctrlKey) {
          if (e.key === '=' || e.key === '+') { e.preventDefault(); zoomIn(); }
          else if (e.key === '-')             { e.preventDefault(); zoomOut(); }
          else if (e.key === '0')             { e.preventDefault(); zoomIdx = 0; }
        }
      }
      window.addEventListener('keydown', handleKeydown);
      addCleanup(() => window.removeEventListener('keydown', handleKeydown));

      listenTracked('ptt-down',                () => applyBackendEvent('ptt-down'));
      listenTracked('ptt-up',                  () => applyBackendEvent('ptt-up'));
      listenTracked('download-progress',        (e) => applyBackendEvent('download-progress', e.payload));
      listenTracked('transcript',               (e) => applyBackendEvent('transcript', e.payload));
      listenTracked('ui-error',                 (e) => applyBackendEvent('ui-error', e.payload));
      listenTracked('transcription-rejected',   (e) => applyBackendEvent('transcription-rejected', e.payload));
      listenTracked('transcript-error',         (e) => applyBackendEvent('transcript-error', e.payload));
      listenTracked('paste-error',              (e) => applyBackendEvent('paste-error', e.payload));
      listenTracked('paste-copied',             (e) => applyBackendEvent('paste-copied', e.payload));
      listenTracked('focus-changed-before-paste', (e) => applyBackendEvent('focus-changed-before-paste', e.payload));
      listenTracked('recording-discarded',      (e) => applyBackendEvent('recording-discarded', e.payload));
      listenTracked('recording-cancelled',      () => applyBackendEvent('recording-cancelled'));
      listenTracked('recording-recovered',      (e) => applyBackendEvent('recording-recovered', e.payload));
      listenTracked('recording-too-short',      (e) => applyBackendEvent('recording-too-short', e.payload));
      listenTracked('device-lost',              () => applyBackendEvent('device-lost'));
      listenTracked('open-history',             () => switchTab('history'));
      listenTracked('ollama-pull-progress',     (e) => applyBackendEvent('ollama-pull-progress', e.payload));

      // Re-check readiness on window focus — catches "user revoked permission
      // between sessions" or "model file deleted" without paying for constant
      // polling. Cheap because checkReadiness is one filesystem stat + two
      // syscalls. Debounced at 250ms to coalesce rapid focus events.
      let readinessTimeout = null;
      const onFocus = () => {
        if (readinessTimeout) clearTimeout(readinessTimeout);
        readinessTimeout = window.setTimeout(() => {
          recheckReadiness();
          readinessTimeout = null;
        }, 250);
      };
      window.addEventListener('focus', onFocus);
      addCleanup(() => {
        window.removeEventListener('focus', onFocus);
        if (readinessTimeout) clearTimeout(readinessTimeout);
      });
      // Initial check — replaces the default `showOnboarding = true` once the
      // backend confirms what's actually granted.
      recheckReadiness().then(async () => {
        logUi('app-ready', platform);
        if (!showOnboarding) {
          await applyWindowSizeLimits();
          commands.prewarmOllama(); // fire-and-forget — loads LLM before first dictation
        }
      });
      syncAppStateFromBackend();

      getCurrentWindow().onResized(() => {
        if (resizeTimeout) clearTimeout(resizeTimeout);
        resizeTimeout = window.setTimeout(() => {
          trackWindowHeight();
          resizeTimeout = null;
        }, 150);
      }).then(addCleanup);
    };

    init();

    return () => {
      disposed = true;
      pendingTimeouts.forEach(id => clearTimeout(id));
      pendingTimeouts.clear();
      if (resizeTimeout) clearTimeout(resizeTimeout);
      cleanups.splice(0).forEach(cleanup => cleanup());
    };
  });
</script>

<div bind:this={outerEl} class="flex flex-col h-full overflow-hidden bg-[var(--surface-raised)]"
>

  <ErrorToast
    bind:uiErrors
    onDismiss={(id) => { uiErrors = uiErrors.filter(x => x.id !== id); }}
    onOpenSettings={(tab) => switchTab(tab)}
  />

  {#if showOnboarding}
    <Onboarding
      onComplete={completeOnboarding}
      onUnsupportedContinue={() => {
        commands.clearForceOnboarding();
        unsupportedPlatformDismissed = true;
        showOnboarding = false;
        restoreMainWindowSize();
      }}
    />
  {/if}

  <TitleBar {activeTab} {recording} {transcribing} onTabSwitch={switchTab} />

  {#if activeTab === 'history'}
    <HistoryTab {history} {copiedTs} {transcriptError} {transcriptNotice} {filteredEntry} {recording} {transcribing} hotkeyLabel={hotkeyDisplayName(cfgHotkeyKey)} {cfgHotkeyMode} actions={historyActions()} />
  {/if}

  {#if activeTab === 'models'}
    <ModelsTab {cfgBackend} {cfgModels} {cfgModel} {downloadProgress} {deletingModels} {altModels} {newModelPath} {modelConfigured} {cfgBackendVariant} actions={modelsActions()} />
  {/if}

  {#if activeTab === 'modes'}
    <ModesTab {cfgBackend} {cfgCleanupMode} {cfgStripFillers} {cfgAppendPeriod} {cfgStripArtifacts} {cfgOllamaUrl} {cfgLlmModel} {cfgVocabulary} {cfgAntiVocabulary} {cfgClassifierPrompt} {activePresetId} {cfgVadEnabled} {ollamaReachable} {ollamaModelPresent} {ollamaModelPartial} {ollamaPullState} actions={modesActions()} />
  {/if}

  {#if activeTab === 'settings'}
    <div class="flex-1 min-h-0 overflow-y-auto pb-0.5 bg-[var(--surface)] text-[12px]">
      <SettingsTab
        bind:cfgHotkeyMode
        bind:cfgCancelOnEsc
        bind:cfgCancelOnHold
        bind:cfgTheme
        bind:cfgLaunchLogin
        bind:cfgDevice
        {audioDevices}
        bind:cfgSaveHistory
        bind:cfgHistoryAutoDelete
        bind:cfgShowOverlay
        bind:cfgOverlaySize
        bind:cfgOverlayPosition
        bind:cfgCursorDotIndicator
        bind:cfgSoundOnStart
        bind:cfgSoundOnFinish
        bind:cfgSoundOnCancel
        bind:cfgSoundOnError
        bind:cfgSoundVolume
        bind:cfgPauseMediaOnDictate
        {ZOOM_LEVELS}
        bind:zoomIdx
        bind:hotkeySide
        bind:hotkeyKeyPart
        {hotkeyKeyItems}
        {hasLogitechMouse}
        {platform}
        bind:settingsContentEl
        onIndicatorOver={_onIndicatorOver}
        onIndicatorLeave={_onIndicatorLeave}
        onSaveSettings={saveSettings}
        onResetOpen={() => { resetOpen = true; resetClosing = false; resetError = ''; }}
        onApplyHotkey={applyHotkeyKey}
      />
    </div>
  {/if}

  <AboutModal
    open={aboutOpen}
    closing={aboutClosing}
    onClose={closeAbout}
  />

  <NoModelPopup
    open={noModelPopupOpen}
    closing={noModelPopupClosing}
    onClose={closeNoModelPopup}
    onOpenModels={() => switchTab('models')}
  />

  <ResetModal
    open={resetOpen}
    closing={resetClosing}
    {resetBusy}
    {resetError}
    {warmupResetBusy}
    {warmupResetMsg}
    bind:bugNote
    {diagnosticMsg}
    {platform}
    onClose={closeReset}
    onResetTurboTalk={(deleteModels) => resetTurboTalk(deleteModels)}
    onClearWarmupCache={clearWarmupCache}
    onCreateBugReport={createBugReport}
  />

  <BottomBar
    {ZOOM_LEVELS}
    bind:zoomIdx
    {tipText}
    onZoomIn={zoomIn}
    onZoomOut={zoomOut}
    onAboutOpen={() => { aboutOpen = true; aboutClosing = false; }}
  />

</div>
