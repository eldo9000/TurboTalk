<script>
  import { onMount, tick } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { getVersion } from '@tauri-apps/api/app';
  import { LogicalSize } from '@tauri-apps/api/dpi';
  import { open as openFilePicker } from '@tauri-apps/plugin-dialog';
  import { initTheme } from '@libre/ui/src/theme.js';
  import HistoryTab from './HistoryTab.svelte';
  import ModelsTab from './ModelsTab.svelte';
  import EditsTab from './EditsTab.svelte';
  import Onboarding from './Onboarding.svelte';
  import ErrorToast from './ErrorToast.svelte';
  import TitleBar from './TitleBar.svelte';
  import SettingsTab from './SettingsTab.svelte';
  import AboutModal from './AboutModal.svelte';
  import NoModelPopup from './NoModelPopup.svelte';
  import ResetModal from './ResetModal.svelte';
  import BottomBar from './BottomBar.svelte';
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

  async function completeOnboarding() {
    await commands.clearForceOnboarding();
    await syncAppStateFromBackend();
    showOnboarding = false;
    cfgLaunchLogin = await commands.getLaunchAtLogin();
    const res = await commands.prewarmModel();
    if (res.status === 'error') {
      console.warn('[onboarding] prewarm skipped:', res.error);
    }
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

  let cfgCleanupMode          = $state('off');
  let cfgOllamaUrl            = $state('');
  let cfgLlmModel             = $state('');
  let cfgVocabulary           = $state('');
  let cfgAntiVocabulary       = $state('');
  let cfgFormatPunct          = $state(true);
  let cfgFormatLiteral        = $state(true);
  let cfgFormatStripFillers   = $state(true);
  let cfgFormatStripArtifacts = $state(true);
  let cfgFormatCapitalize     = $state(true);
  let editsSaveMsg            = $state('');

  // Ollama setup state (legacy — kept for settings save/load compatibility)
  let ollamaReachable         = $state(null);
  let ollamaModelPresent      = $state(null);
  let ollamaModelPartial      = $state(false);
  let ollamaPullState         = $state({ inFlight: false, pct: 0, status: '', error: '' });

  // Settings tab
  const cfgBin             = 'auto';
  let cfgLaunchLogin       = $state(false);
  let cfgShowSplash        = $state(true);
  let cfgDevice            = $state('default');
  let audioDevices         = $state([]);
  let settingsSaveMsg      = $state('');
  let diagnosticMsg        = $state('');
  let splashActive         = $state(true);
  let version              = $state('');

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
  let cfgAutoTapThreshold  = $state(400);
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

  function applyZoomAndSave() {
    document.documentElement.style.zoom = `${ZOOM_LEVELS[zoomIdx]}%`;
    localStorage.setItem('tt-zoom', String(zoomIdx));
    initWindowSizeLimits();
  }
  function zoomIn()  { if (zoomIdx < ZOOM_LEVELS.length - 1) { zoomIdx++; applyZoomAndSave(); } }
  function zoomOut() { if (zoomIdx > 0) { zoomIdx--; applyZoomAndSave(); } }

  // Zoom CSS + localStorage sync for any zoomIdx write (Settings tab buttons
  // or any other binding). Runs on mount too, so the CSS is consistent.
  $effect(() => {
    document.documentElement.style.zoom = `${ZOOM_LEVELS[zoomIdx]}%`;
    localStorage.setItem('tt-zoom', String(zoomIdx));
  });

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
      hotkey: { key: cfgHotkeyKey, mode: cfgHotkeyMode, cancel_on_esc: cfgCancelOnEsc, cancel_on_hold: cfgCancelOnHold, auto_tap_threshold_ms: cfgAutoTapThreshold },
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
      show_splash: cfgShowSplash,
      cleanup: {
        mode: cfgCleanupMode,
        ollama_url: cfgOllamaUrl,
        classifier_model: cfgLlmModel,
        vocabulary: cfgVocabulary ? cfgVocabulary.split('\n').map(s => s.trim()).filter(Boolean) : [],
        antivocabulary: cfgAntiVocabulary ? cfgAntiVocabulary.split('\n').map(s => s.trim()).filter(Boolean) : [],
        format_punctuation: cfgFormatPunct,
        format_literal: cfgFormatLiteral,
        format_strip_fillers: cfgFormatStripFillers,
        format_strip_artifacts: cfgFormatStripArtifacts,
        format_capitalize: cfgFormatCapitalize,
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

  // ── Edits ─────────────────────────────────────────────────────────────────

  async function openEdits() {
    editsSaveMsg = '';
  }

  async function saveEdits() {
    const res = await commands.saveConfig(buildFullConfig());
    editsSaveMsg = res.status === 'ok' ? 'Saved.' : 'Error: ' + res.error;
  }

  async function handleModeClick(v) {
    cfgCleanupMode = v;
    saveEdits();
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
    cfgAutoTapThreshold  = cfg.hotkey?.auto_tap_threshold_ms    ?? 400;
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
    cfgShowSplash        = cfg.show_splash                     ?? true;
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

  function editsActions() {
    return {
      setCleanupMode: (v) => {
        cfgCleanupMode = v;
        saveEdits();
      },

      setVadEnabled: (v) => { cfgVadEnabled = v; saveEdits(); },
      setVocabulary: (v) => { cfgVocabulary = v; saveEdits(); },
      setAntiVocabulary: (v) => { cfgAntiVocabulary = v; saveEdits(); },
      setOllamaUrl: (v) => { cfgOllamaUrl = v; saveEdits(); },
      setLlmModel: (v) => { cfgLlmModel = v; saveEdits(); },
      setFormatPunct: (v) => { cfgFormatPunct = v; saveEdits(); },
      setFormatLiteral: (v) => { cfgFormatLiteral = v; saveEdits(); },
      setFormatStripFillers: (v) => { cfgFormatStripFillers = v; saveEdits(); },
      setFormatStripArtifacts: (v) => { cfgFormatStripArtifacts = v; saveEdits(); },
      setFormatCapitalize: (v) => { cfgFormatCapitalize = v; saveEdits(); },
    };
  }

  // Measure the Settings tab content and clamp the window max height so the
  // window can't stretch past where the settings content ends. Requires the
  // settings tab to be rendered (settingsContentEl bound).
  function applySettingsMaxHeight() {
    if (!settingsContentEl) return;
    const contentH = Array.from(settingsContentEl.children).reduce(
      (a, c) => a + c.offsetHeight, 0,
    );
    if (!contentH) return;
    const totalCss = 40 + contentH + 2 + 28; // titlebar + content + gap + bottombar
    const maxH = Math.round(totalCss * ZOOM_LEVELS[zoomIdx] / 100);
    const win = getCurrentWindow();
    win.setMaxSize(new LogicalSize(550, maxH));
  }

  // Run the same max-height clamp at startup, before the user ever visits the
  // Settings tab. Renders the settings tab for one microtask-batch to measure
  // it, then restores the previous tab — all inside a single JS task, so the
  // browser never paints the intermediate state (no flash).
  async function initWindowSizeLimits() {
    const prev = activeTab;
    activeTab = 'settings';
    await tick();
    applySettingsMaxHeight();
    activeTab = prev;
    await tick();
  }

  function switchTab(tab) {
    activeTab = tab;
    if (tab === 'models')   openModels();
    if (tab === 'edits')    openEdits();
    if (tab === 'settings') {
      openSettings();
      // Re-measure on entry — keeps the clamp accurate after zoom changes.
      tick().then(applySettingsMaxHeight);
    }
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
      cfgAutoTapThreshold  = initialCfg.hotkey?.auto_tap_threshold_ms    ?? 400;
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
      cfgShowSplash        = initialCfg.show_splash                     ?? true;
      // Splash card: show for 2 seconds on startup, then auto-dismiss.
      if (cfgShowSplash) {
        setTimeout(() => { splashActive = false; }, 2000);
      } else {
        splashActive = false;
      }
      getVersion().then(v => { version = v; });
      cfgModel             = initialCfg.whisper?.model                   ?? '';
      cfgModels            = initialCfg.whisper?.models                  ?? [];
      cfgCleanupMode          = initialCfg.cleanup?.mode                    ?? 'off';
      // TextFormatter is Whisper-only — force Off for other backends.
      if (cfgBackend !== 'whisper') cfgCleanupMode = 'off';
      cfgOllamaUrl            = initialCfg.cleanup?.ollama_url               ?? '';
      cfgLlmModel             = initialCfg.cleanup?.classifier_model         ?? '';
      cfgVocabulary           = (initialCfg.cleanup?.vocabulary ?? []).join('\n');
      cfgAntiVocabulary       = (initialCfg.cleanup?.antivocabulary ?? []).join('\n');
      cfgFormatPunct          = initialCfg.cleanup?.format_punctuation        ?? true;
      cfgFormatLiteral        = initialCfg.cleanup?.format_literal            ?? true;
      cfgFormatStripFillers   = initialCfg.cleanup?.format_strip_fillers      ?? true;
      cfgFormatStripArtifacts = initialCfg.cleanup?.format_strip_artifacts    ?? true;
      cfgFormatCapitalize     = initialCfg.cleanup?.format_capitalize         ?? true;
      if (savedHistory.length) history = savedHistory;

      // Detect Logitech mouse — fast ioreg call, fire-and-forget
      commands.detectLogitechMouse().then(v => { hasLogitechMouse = v; });

      function handleKeydown(e) {
        if (e.metaKey || e.ctrlKey) {
          if (e.key === '=' || e.key === '+') { e.preventDefault(); zoomIn(); }
          else if (e.key === '-')             { e.preventDefault(); zoomOut(); }
          else if (e.key === '0')             { e.preventDefault(); zoomIdx = 0; applyZoomAndSave(); }
        }
      }
      window.addEventListener('keydown', handleKeydown);
      addCleanup(() => window.removeEventListener('keydown', handleKeydown));

      function handleContextMenu(e) {
        const tag = e.target?.tagName;
        if (tag !== 'INPUT' && tag !== 'TEXTAREA') {
          e.preventDefault();
        }
      }
      window.addEventListener('contextmenu', handleContextMenu);
      addCleanup(() => window.removeEventListener('contextmenu', handleContextMenu));

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
          // prewarmOllama removed — legacy LLM classifier no longer used
        }
      });
      syncAppStateFromBackend();

      // Apply initial CSS zoom, then measure settings content and clamp
      // max height — so the bottom-edge limit is enforced from launch.
      applyZoomAndSave();
    };

    init();

    return () => {
      disposed = true;
      pendingTimeouts.forEach(id => clearTimeout(id));
      pendingTimeouts.clear();
      cleanups.splice(0).forEach(cleanup => cleanup());
    };
  });
</script>

<div bind:this={outerEl} class="flex flex-col h-full overflow-hidden bg-[var(--surface)]"
>

  {#if splashActive}
    <div class="splash-card">
      <span class="splash-name">Turbo Talk</span>
      <span class="splash-ver">v{version}</span>
    </div>
  {/if}

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

  {#if activeTab === 'edits'}
    <EditsTab {cfgBackend} {cfgCleanupMode} {cfgVocabulary} {cfgAntiVocabulary} {cfgVadEnabled} {cfgFormatPunct} {cfgFormatLiteral} {cfgFormatStripFillers} {cfgFormatStripArtifacts} {cfgFormatCapitalize} actions={editsActions()} />
  {/if}

  {#if activeTab === 'settings'}
    <div class="settings-scroll flex-1 min-h-0 overflow-y-auto pb-0.5 bg-[var(--surface)] text-[12px]">
      <SettingsTab
        bind:cfgHotkeyMode
        bind:cfgCancelOnEsc
        bind:cfgCancelOnHold
        bind:cfgAutoTapThreshold
        bind:cfgTheme
        bind:cfgLaunchLogin
        bind:cfgShowSplash
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
    bind:bugNote
    {diagnosticMsg}
    {platform}
    onClose={closeReset}
    onResetTurboTalk={(deleteModels) => resetTurboTalk(deleteModels)}
    onCreateBugReport={createBugReport}
    onRerunWelcome={async () => { await commands.resetOnboarding(); await recheckReadiness(); }}
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

<style>
  .splash-card {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    z-index: 100;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: 16px 16px 12px;
    background: var(--surface-raised, #f8f8f8);
    border: 1px solid var(--border, #e0e0e0);
    border-radius: 14px;
    box-shadow: 0 8px 40px rgba(0,0,0,0.15), 0 2px 8px rgba(0,0,0,0.08);
    user-select: none;
    animation: splash-in 0.22s ease both;
  }
  :global(.dark) .splash-card {
    background: var(--surface-raised, #1a1a1a);
    border-color: var(--border, #2a2a2a);
    box-shadow: 0 24px 48px rgba(0,0,0,0.6), 0 4px 12px rgba(0,0,0,0.4);
  }
  .splash-name {
    font-family: -apple-system, BlinkMacSystemFont, sans-serif;
    font-size: 18px;
    font-weight: 600;
    letter-spacing: -0.3px;
    color: var(--text-primary, #1a1a1a);
  }
  :global(.dark) .splash-name { color: var(--text-primary, #f0f0f0); }
  .splash-ver {
    font-family: -apple-system, BlinkMacSystemFont, sans-serif;
    font-size: 10px;
    color: var(--text-muted, #999);
    font-variant-numeric: tabular-nums;
  }
  :global(.dark) .splash-ver { color: var(--text-muted, #666); }
  @keyframes splash-in {
    0% { opacity: 0; transform: translate(-50%, -50%) scale(0.92); }
    100% { opacity: 1; transform: translate(-50%, -50%) scale(1); }
  }
</style>
