<script>
  // First-launch (and any-launch-with-missing-prerequisites) gate.
  //
  // Polls `commands.checkReadiness()` every second while open so each row
  // flips green the moment the user grants permission in System Settings or
  // a model finishes downloading. Step 1 is Input Monitoring; Step 2 is
  // Microphone; Step 3 is Accessibility; Step 4 is Model selection;
  // Step 5 is Launch at Login.
  //
  // Closes itself by calling `onComplete()` when readiness is fully green.

  import { onMount, onDestroy, tick } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { LogicalSize } from '@tauri-apps/api/dpi';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { commands } from './bindings.ts';

  let { onComplete, onUnsupportedContinue } = $props();

  let readiness          = $state(null);
  let launchAtLogin      = $state(false);
  let pollHandle         = null;
  let micPromptInFlight  = $state(false);
  let inputPromptInFlight = $state(false);
  let launchPromptInFlight = $state(false);
  let launchSkipped      = $state(false);
  let launchError        = $state('');
  let downloadingModel   = $state(null);
  let downloadPct        = $state(0);
  let downloadError      = $state('');
  let restartArmed       = $state(false);
  let cfgModel           = $state('');
  let cfgModels          = $state([]);
  let contentEl          = $state(null);
  let resizeObserver     = null;
  let resizeRaf          = null;
  let lastWindowHeight   = 0;

  // Stale-entry fix: track whether user has already clicked "Open System
  // Settings" for a given step. Once set, show the fix-stale-entry UI if
  // the step is still blocked after they return.
  let imOpenedSettings   = $state(false);
  let axOpenedSettings   = $state(false);
  let fixImInFlight      = $state(false);
  let fixAxInFlight      = $state(false);
  let fixImError         = $state('');
  let fixAxError         = $state('');

  // Backend family selection — shown in the model step before picking a model.
  let selectedBackend = $state('parakeet'); // 'whisper' | 'moonshine' | 'parakeet'

  const ENGINE_OPTIONS = [
    ['parakeet', 'Parakeet'],
    ['whisper', 'Whisper'],
    ['moonshine', 'Moonshine'],
  ];

  const BACKEND_EXPLAINERS = {
    parakeet:  'English-only · fastest · recommended default.',
    whisper:   'Multilingual · most accurate.',
    moonshine: 'English-only · low hallucination on silence.',
  };

  const RECOMMENDED_WHISPER = {
    id: 'ggml-large-v3-turbo',
    label: 'Large v3 Turbo',
    size: '1.6 GB',
    description: 'Best accuracy · multilingual',
    recommended: true,
  };

  const ALTERNATES = [
    { id: 'ggml-large-v3-turbo-q5_0', label: 'Large v3 Turbo (q5_0)',
      size: '574 MB', description: 'Low RAM · slightly reduced accuracy', recommended: false },
    { id: 'ggml-large-v3', label: 'Large v3',
      size: '3.1 GB', description: 'High accuracy · high RAM · slow', recommended: false },
  ];

  const ALL_MODELS = [RECOMMENDED_WHISPER, ...ALTERNATES];

  // Populated from listModelsForFamily — per-model installed state, not global model_present.
  let altModels = $state([]);
  const WINDOW_W = 440;
  const WINDOW_SIZE_SLACK = 18;

  function uniqueModels(paths) {
    return [...new Set(paths.filter(Boolean))];
  }

  function installedPath(modelId) {
    return cfgModels.find(path => path.endsWith(`${modelId}.bin`)) ?? '';
  }

  let selectedModelReady = $derived(
    selectedBackend === 'whisper'
      ? (!!cfgModel && cfgModels.includes(cfgModel))
      : (readiness?.model_present ?? false)
  );

  function currentZoom() {
    return (parseFloat(document.documentElement.style.zoom || '100') || 100) / 100;
  }

  async function resizeToContent() {
    await tick();
    if (!contentEl) return;
    const zoom = currentZoom();
    const contentHeight = Math.ceil(contentEl.scrollHeight * zoom) + WINDOW_SIZE_SLACK;
    const targetHeight = Math.max(360, contentHeight);
    if (Math.abs(targetHeight - lastWindowHeight) < 2) return;
    lastWindowHeight = targetHeight;
    await getCurrentWindow().setSize(new LogicalSize(Math.ceil(WINDOW_W * zoom), targetHeight));
  }

  function scheduleResize() {
    if (resizeRaf) cancelAnimationFrame(resizeRaf);
    resizeRaf = requestAnimationFrame(() => {
      resizeRaf = null;
      resizeToContent();
    });
  }

  function observeContentSize() {
    resizeObserver?.disconnect();
    resizeObserver = null;
    if (!contentEl) return;
    resizeObserver = new ResizeObserver(scheduleResize);
    resizeObserver.observe(contentEl);
    scheduleResize();
  }

  $effect(() => {
    if (!contentEl) return;
    observeContentSize();
    return () => {
      resizeObserver?.disconnect();
      resizeObserver = null;
    };
  });

  $effect(() => {
    readiness;
    launchAtLogin;
    cfgModel;
    cfgModels;
    downloadingModel;
    downloadPct;
    downloadError;
    restartArmed;
    unsupportedPlatform;
    imOpenedSettings;
    axOpenedSettings;
    fixImError;
    fixAxError;
    selectedBackend;
    scheduleResize();
  });

  async function loadAltModels(backend = selectedBackend) {
    if (backend === 'whisper') {
      altModels = [];
      return;
    }
    altModels = await commands.listModelsForFamily(backend).catch(() => []);
  }

  async function saveBackendToConfig(family) {
    try {
      const cfg = await commands.getConfig();
      cfg.backend = family;
      await commands.saveConfig(cfg);
      await loadAltModels(family);
      await refresh();
    } catch (e) {
      console.warn('saveBackendToConfig failed', e);
    }
  }

  function permissionSatisfied(status) {
    return status === 'granted' || status === 'unsupported';
  }

  function modelReadyForBackend(r, backend) {
    if (backend === 'whisper') {
      return !!cfgModel && cfgModels.includes(cfgModel);
    }
    return r?.model_present ?? false;
  }

  async function refresh() {
    const [nextReadiness, nextLaunchAtLogin, cfg, scannedModels] = await Promise.all([
      commands.checkReadiness(),
      commands.getLaunchAtLogin(),
      commands.getConfig(),
      commands.scanModelsDir(),
    ]);
    const nextCfgModel = cfg.whisper?.model ?? '';
    const nextCfgModels = uniqueModels(scannedModels ?? []);
    readiness = nextReadiness;
    launchAtLogin = nextLaunchAtLogin;
    cfgModel = nextCfgModel;
    cfgModels = nextCfgModels;
    // Restore backend selection from persisted config (e.g. if user reopens onboarding).
    if (cfg.backend && cfg.backend !== selectedBackend) {
      selectedBackend = cfg.backend;
    }
    const backend = cfg.backend ?? selectedBackend;
    const nextModelReady = modelReadyForBackend(nextReadiness, backend);
    await loadAltModels(backend);
    if (
      !downloadingModel
      && permissionSatisfied(nextReadiness.accessibility)
      && permissionSatisfied(nextReadiness.input_monitoring)
      && permissionSatisfied(nextReadiness.microphone)
      && nextModelReady
      && nextLaunchAtLogin
      && !nextReadiness.force_onboarding
    ) {
      stopPolling();
      onComplete?.();
    }
  }

  function startPolling() {
    if (pollHandle) return;
    pollHandle = setInterval(refresh, 1000);
  }
  function stopPolling() {
    if (pollHandle) { clearInterval(pollHandle); pollHandle = null; }
  }

  onMount(async () => {
    await refresh();
    startPolling();
    scheduleResize();
  });
  onDestroy(() => {
    stopPolling();
    resizeObserver?.disconnect();
    if (resizeRaf) cancelAnimationFrame(resizeRaf);
  });

  async function openAccessibility() {
    axOpenedSettings = true;
    // Calls AXIsProcessTrustedWithOptions(prompt: true). Side-effects:
    //   1. macOS auto-adds Turbo Talk to the Accessibility list (off).
    //   2. macOS shows its native "would like to use Accessibility" prompt
    //      with a built-in "Open System Preferences" button.
    await commands.promptForAccessibility();
    await refresh();
    restartArmed = true;
  }

  async function restart() {
    await commands.restartApp();
  }

  function delay(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
  }

  async function openInputMonitoring() {
    imOpenedSettings = true;
    inputPromptInFlight = true;
    try {
      const status = await commands.requestInputMonitoringPermission();
      // macOS may need a moment to add the current bundle to the Input
      // Monitoring list after IOHIDRequestAccess. Opening Settings too soon
      // can land on a pane where Turbo Talk is not listed yet, forcing the
      // user through the manual file-picker path.
      if (status !== 'granted') {
        await delay(2000);
        await refresh();
        if (readiness?.input_monitoring !== 'granted') {
          await commands.openSystemSettings('input_monitoring');
        }
      }
    } finally {
      inputPromptInFlight = false;
    }
    restartArmed = true;
    await refresh();
  }

  async function grantMic() {
    micPromptInFlight = true;
    try {
      const status = await commands.requestMicrophonePermission();
      if (status === 'denied') {
        await commands.openSystemSettings('microphone');
      }
    } finally {
      micPromptInFlight = false;
      await refresh();
    }
  }

  async function openMicSettings() {
    await commands.openSystemSettings('microphone');
  }

  async function enableLaunchAtLogin() {
    launchPromptInFlight = true;
    launchError = '';
    try {
      const res = await commands.setLaunchAtLogin(true);
      if (res.status === 'error') {
        launchError = res.error || 'Could not enable launch at login.';
      } else {
        launchAtLogin = true;
        stopPolling();
        if (!downloadingModel) onComplete?.();
      }
    } finally {
      launchPromptInFlight = false;
    }
  }

  async function saveSelectedModel(path) {
    const cfg = await commands.getConfig();
    if (!cfg.whisper) cfg.whisper = { bin: 'auto', model: '', models: [] };
    cfg.whisper.model = path;
    cfg.whisper.models = uniqueModels([...(cfg.whisper.models ?? []), path]);
    await commands.saveConfig(cfg);
    await refresh();
  }

  async function selectModel(path) {
    await saveSelectedModel(path);
  }

  async function downloadAltModel(model) {
    downloadingModel = model.id;
    downloadPct      = 0;
    downloadError    = '';
    const variant = model.id.replace(/^moonshine-|^parakeet-/, '');
    const progressKey = `${selectedBackend}-${variant}`;
    const unlisten = await listen('download-progress', (e) => {
      const name = e.payload?.name ?? '';
      if (name === model.id || name === progressKey || name.startsWith(progressKey)) {
        downloadPct = e.payload.pct ?? 0;
      }
    });
    try {
      const res = selectedBackend === 'moonshine'
        ? await commands.downloadMoonshineModel(variant)
        : await commands.downloadParakeetModel(variant);
      if (res.status === 'error') {
        downloadError = res.error || 'Download failed.';
        return;
      }
      await refresh();
    } finally {
      unlisten();
      downloadingModel = null;
    }
  }

  async function downloadModel(model) {
    downloadingModel = model.id;
    downloadPct      = 0;
    downloadError    = '';
    const unlisten = await listen('download-progress', (e) => {
      if (e.payload?.name === model.id) {
        downloadPct = e.payload.pct ?? 0;
      }
    });
    try {
      const res = await commands.downloadModel(model.id);
      if (res.status === 'error') {
        downloadError = res.error || 'Download failed.';
        return;
      }
      await saveSelectedModel(res.data);
    } finally {
      unlisten();
      downloadingModel = null;
    }
  }

  // Reset a stale TCC entry so the user can re-grant from a clean slate.
  // Called when the user opened System Settings but the step is still blocked,
  // indicating a stale entry from a previous install is in the way.
  async function fixStaleTccIm() {
    fixImInFlight = true;
    fixImError = '';
    try {
      await commands.resetTccEntry('input_monitoring');
      await openInputMonitoring();
    } catch (e) {
      fixImError = typeof e === 'string'
        ? e
        : 'Reset failed. Try manually removing Turbo Talk from Input Monitoring in System Settings, then click Open System Settings above.';
    } finally {
      fixImInFlight = false;
    }
  }

  async function fixStaleTccAx() {
    fixAxInFlight = true;
    fixAxError = '';
    try {
      await commands.resetTccEntry('accessibility');
      await openAccessibility();
    } catch (e) {
      fixAxError = typeof e === 'string'
        ? e
        : 'Reset failed. Try manually removing Turbo Talk from Accessibility in System Settings, then click Open System Settings above.';
    } finally {
      fixAxInFlight = false;
    }
  }

  let stepStates = $derived.by(() => {
    if (!readiness) return { accessibility: 'active', input_monitoring: 'pending', microphone: 'pending', model: 'pending', launch: 'pending' };
    const a = permissionSatisfied(readiness.accessibility);
    const i = permissionSatisfied(readiness.input_monitoring);
    const m = permissionSatisfied(readiness.microphone);
    const p = selectedModelReady;
    // macOS TCC steps only gate model selection; Windows/Linux skip that wait.
    const permissionsReady = readiness.platform === 'macos' ? (a && i && m) : readiness.platform !== 'linux';
    return {
      input_monitoring: i ? 'done' : 'active',
      microphone:    m ? 'done' : (i ? 'active' : 'pending'),
      accessibility: a ? 'done' : (i && m ? 'active' : 'pending'),
      model:         p ? 'done' : (permissionsReady ? 'active' : 'pending'),
      launch:        (launchAtLogin || launchSkipped) ? 'done' : (a && i && m && p ? 'active' : 'pending'),
    };
  });

  // Linux beta is not fully supported (Wayland hotkey gap). Windows returns
  // `unsupported` for macOS-only TCC checks — that is not a platform block.
  let unsupportedPlatform = $derived(readiness?.platform === 'linux');

  function stepClass(state) {
    if (state === 'active') return 'border-[var(--accent)]/40 bg-[var(--accent)]/5';
    if (state === 'done')   return 'border-[var(--border,#2a2a2a)]';
    return 'border-[var(--border,#2a2a2a)] opacity-50';
  }
  function badgeClass(state) {
    if (state === 'done')   return 'bg-green-600 text-white';
    if (state === 'active') return 'bg-[var(--accent)] text-white';
    return 'bg-[var(--border,#2a2a2a)] text-[var(--text-secondary)]';
  }

  // Connector line between two adjacent steps. Green when the upper step is
  // done, muted otherwise. `w-1` (4 px) gives a clearly visible thick line.
  function connectorClass(upperStepDone) {
    return upperStepDone
      ? 'bg-green-600'
      : 'bg-[var(--border,#2a2a2a)]';
  }
</script>

<div class="fixed inset-0 z-[100] bg-[var(--surface)] text-[var(--text-primary)] flex flex-col overflow-y-auto">
  <div bind:this={contentEl} class="max-w-[420px] w-full mx-auto px-6 py-6 pb-6 flex flex-col gap-3.5">

    <div class="flex flex-col gap-1.5">
      <h1 class="text-[18px] font-semibold leading-tight text-[var(--text-primary)]">Welcome to Turbo Talk</h1>
      <p class="text-[12px] text-[var(--text-secondary)] leading-relaxed">
        {unsupportedPlatform
          ? 'This beta is not fully supported on Linux yet.'
          : readiness?.platform === 'windows'
            ? 'Download a model to start dictating. Parakeet is recommended on Windows.'
            : 'Finish setup before you can start dictating.'}
      </p>
    </div>

    {#if readiness}
      {#if unsupportedPlatform}
        <div class="flex flex-col gap-3 p-3.5 rounded-lg border border-yellow-500/30 bg-yellow-500/10">
          <div class="flex flex-col gap-1">
            <h2 class="text-[13px] font-medium leading-tight text-yellow-700 dark:text-yellow-200">Unsupported platform</h2>
            <p class="text-[11px] text-[var(--text-secondary)] leading-snug">
              Turbo Talk's Linux beta does not yet support global push-to-talk on all
              desktop sessions. Use macOS or Windows for the full dictation loop.
            </p>
          </div>
          <button onclick={() => onUnsupportedContinue?.()}
            class="self-start px-3 py-1.5 rounded-md border border-yellow-600/50 dark:border-yellow-500/40 text-[12px] font-medium text-yellow-700 dark:text-yellow-100 hover:bg-yellow-500/15 transition-colors">
            Continue without dictation
          </button>
        </div>
      {:else}
      <!-- Steps wrapped in a flex-col with no gap; connector divs provide spacing
           and draw the progress line between badge circles. Badge center is at
           12 px (p-3 padding) + 12 px (half of w-6) = 24 px from the card left
           edge. Connectors use ml-[23px] w-1 so the 4 px line is centered at
           23 + 2 = 25 px — 1 px off, invisible at this scale. -->
      <div class="flex flex-col">

        <!-- Step 1: Input Monitoring -->
        <div class="flex gap-3 {stepStates.input_monitoring === 'done' ? 'p-3' : 'p-3.5'} rounded-lg border {stepClass(stepStates.input_monitoring)}">
          <div class="shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-[11px] font-semibold {badgeClass(stepStates.input_monitoring)}">
            {stepStates.input_monitoring === 'done' ? '✓' : '1'}
          </div>
          <div class="flex flex-col gap-2 min-w-0 flex-1">
            <div class="flex flex-col gap-0.5">
              <h2 class="text-[13px] font-medium leading-tight text-[var(--text-primary)]">Allow Input Monitoring</h2>
              {#if stepStates.input_monitoring !== 'done'}
                <p class="text-[11px] text-[var(--text-secondary)] leading-snug">
                  This lets Turbo Talk receive the push-to-talk key while another app is focused.
                  Restart once after turning it on.
                </p>
              {/if}
            </div>
            {#if stepStates.input_monitoring === 'active'}
              <div class="flex gap-2 flex-wrap">
                <button onclick={openInputMonitoring}
                  disabled={inputPromptInFlight}
                  class="px-3 py-1.5 rounded-md bg-[var(--accent)] text-white text-[12px] font-medium hover:opacity-90 transition-opacity">
                  {inputPromptInFlight ? 'Waiting for macOS…' : 'Open System Settings'}
                </button>
                {#if restartArmed}
                  <button onclick={restart}
                    class="px-3 py-1.5 rounded-md border border-[var(--border,#3a3a3a)] text-[12px] hover:bg-white/5 transition-colors">
                    Restart Turbo Talk
                  </button>
                {/if}
              </div>
              <p class="text-[11px] text-[var(--text-secondary)] leading-snug">
                Toggle Turbo Talk on under Privacy &amp; Security → Input Monitoring.
                If it doesn't appear in the list, click <strong>+</strong> to add it manually.
              </p>
              {#if imOpenedSettings}
                <div class="flex flex-col gap-1.5 p-2.5 rounded-md border border-orange-500/30 bg-orange-500/8">
                  <p class="text-[11px] text-orange-200/90 leading-snug font-medium">Already in the list but still not working?</p>
                  <p class="text-[11px] text-[var(--text-secondary)] leading-snug">
                    A stale entry from a previous install may be blocking it. This removes the old entry and re-registers Turbo Talk automatically.
                  </p>
                  <button onclick={fixStaleTccIm} disabled={fixImInFlight || inputPromptInFlight}
                    class="self-start px-2.5 py-1 rounded-md border border-orange-500/40 text-[11px] font-medium text-orange-100/90 hover:bg-orange-500/15 disabled:opacity-50 transition-colors">
                    {fixImInFlight ? 'Resetting…' : 'Fix stale entry'}
                  </button>
                  {#if fixImError}
                    <p class="text-[11px] text-red-400 leading-snug">{fixImError}</p>
                  {/if}
                </div>
              {/if}
            {/if}
          </div>
        </div>

        <!-- Connector 1 → 2 -->
        <div class="h-3.5 ml-[23px] w-1 rounded-full {connectorClass(stepStates.input_monitoring === 'done')}"></div>

        <!-- Step 2: Microphone -->
        <div class="flex gap-3 {stepStates.microphone === 'done' ? 'p-3' : 'p-3.5'} rounded-lg border {stepClass(stepStates.microphone)}">
          <div class="shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-[11px] font-semibold {badgeClass(stepStates.microphone)}">
            {stepStates.microphone === 'done' ? '✓' : '2'}
          </div>
          <div class="flex flex-col gap-2 min-w-0 flex-1">
            <div class="flex flex-col gap-0.5">
              <h2 class="text-[13px] font-medium leading-tight text-[var(--text-primary)]">Allow Microphone</h2>
              {#if stepStates.microphone !== 'done'}
                <p class="text-[11px] text-[var(--text-secondary)] leading-snug">
                  So Turbo Talk can record your voice while you hold the hotkey. Audio never leaves your machine.
                </p>
              {/if}
            </div>
            {#if stepStates.microphone === 'active'}
              {#if readiness.microphone === 'not_determined'}
                <button onclick={grantMic} disabled={micPromptInFlight}
                  class="self-start px-3 py-1.5 rounded-md bg-[var(--accent)] text-white text-[12px] font-medium hover:opacity-90 disabled:opacity-50 transition-opacity">
                  {micPromptInFlight ? 'Waiting for prompt…' : 'Grant Microphone Access'}
                </button>
              {:else}
                <button onclick={openMicSettings}
                  class="self-start px-3 py-1.5 rounded-md bg-[var(--accent)] text-white text-[12px] font-medium hover:opacity-90 transition-opacity">
                  Open System Settings
                </button>
                <p class="text-[11px] text-[var(--text-secondary)] leading-snug">
                  Toggle Turbo Talk on under Privacy &amp; Security → Microphone.
                </p>
              {/if}
            {/if}
          </div>
        </div>

        <!-- Connector 2 → 3 -->
        <div class="h-3.5 ml-[23px] w-1 rounded-full {connectorClass(stepStates.microphone === 'done')}"></div>

        <!-- Step 3: Accessibility (restart required, surfaced after native prompts) -->
        <div class="flex gap-3 {stepStates.accessibility === 'done' ? 'p-3' : 'p-3.5'} rounded-lg border {stepClass(stepStates.accessibility)}">
          <div class="shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-[11px] font-semibold {badgeClass(stepStates.accessibility)}">
            {stepStates.accessibility === 'done' ? '✓' : '3'}
          </div>
          <div class="flex flex-col gap-2 min-w-0 flex-1">
            <div class="flex flex-col gap-0.5">
              <h2 class="text-[13px] font-medium leading-tight text-[var(--text-primary)]">Allow Accessibility</h2>
              {#if stepStates.accessibility !== 'done'}
                <p class="text-[11px] text-[var(--text-secondary)] leading-snug">
                  Turbo Talk needs Accessibility permission to read your push-to-talk hotkey globally.
                  Granting this requires restarting the app once.
                </p>
              {/if}
            </div>
            {#if stepStates.accessibility === 'active'}
              <div class="flex gap-2 flex-wrap">
                <button onclick={openAccessibility}
                  class="px-3 py-1.5 rounded-md bg-[var(--accent)] text-white text-[12px] font-medium hover:opacity-90 transition-opacity">
                  Open System Settings
                </button>
                {#if restartArmed}
                  <button onclick={restart}
                    class="px-3 py-1.5 rounded-md border border-[var(--border,#3a3a3a)] text-[12px] hover:bg-white/5 transition-colors">
                    Restart Turbo Talk
                  </button>
                {/if}
              </div>
              {#if restartArmed}
                <p class="text-[11px] text-[var(--text-secondary)] leading-snug">
                  Toggle Turbo Talk on under Privacy &amp; Security → Accessibility, then click Restart.
                </p>
              {/if}
              {#if axOpenedSettings}
                <div class="flex flex-col gap-1.5 p-2.5 rounded-md border border-orange-500/30 bg-orange-500/8">
                  <p class="text-[11px] text-orange-200/90 leading-snug font-medium">Already in the list but still not working?</p>
                  <p class="text-[11px] text-[var(--text-secondary)] leading-snug">
                    A stale entry from a previous install may be blocking it. This removes the old entry and re-registers Turbo Talk automatically.
                  </p>
                  <button onclick={fixStaleTccAx} disabled={fixAxInFlight}
                    class="self-start px-2.5 py-1 rounded-md border border-orange-500/40 text-[11px] font-medium text-orange-100/90 hover:bg-orange-500/15 disabled:opacity-50 transition-colors">
                    {fixAxInFlight ? 'Resetting…' : 'Fix stale entry'}
                  </button>
                  {#if fixAxError}
                    <p class="text-[11px] text-red-400 leading-snug">{fixAxError}</p>
                  {/if}
                </div>
              {/if}
            {/if}
          </div>
        </div>

        <!-- Connector 3 → 4 -->
        <div class="h-3.5 ml-[23px] w-1 rounded-full {connectorClass(stepStates.accessibility === 'done')}"></div>

        <!-- Step 4: Model (includes backend family picker) -->
        <div class="flex gap-3 p-3.5 rounded-lg border {stepClass(stepStates.model)}">
          <div class="shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-[11px] font-semibold {badgeClass(stepStates.model)}">
            {stepStates.model === 'done' ? '✓' : '4'}
          </div>
          <div class="flex flex-col gap-2 min-w-0 flex-1">
            <div class="flex flex-col gap-0.5">
              <h2 class="text-[13px] font-medium leading-tight text-[var(--text-primary)]">Choose an engine and download a model</h2>
              <p class="text-[11px] text-[var(--text-secondary)] leading-snug">
                Parakeet is recommended for most English dictation. All engines run fully locally on your device.
              </p>
            </div>
            {#if stepStates.model === 'active' || stepStates.model === 'done'}
              <!-- Backend family picker -->
              <div class="flex gap-1.5">
                {#each ENGINE_OPTIONS as [v, lbl]}
                  <button
                    onclick={async () => { selectedBackend = v; await saveBackendToConfig(v); }}
                    class="px-2.5 py-1 rounded-md border text-[11px] font-medium transition-colors
                           {selectedBackend === v
                             ? 'border-[var(--accent)] bg-[var(--accent)]/10 text-[var(--text-primary)]'
                             : 'border-[var(--border,#2a2a2a)] text-[var(--text-secondary)] hover:bg-white/5'}">
                    {lbl}
                  </button>
                {/each}
              </div>
              <p class="text-[10px] text-[var(--text-secondary)] leading-snug -mt-1">{BACKEND_EXPLAINERS[selectedBackend]}</p>

              {#if downloadingModel}
                <div class="flex flex-col gap-1.5">
                  <div class="flex items-center justify-between text-[11px]">
                    <span class="truncate text-[var(--text-secondary)]">{downloadingModel}</span>
                    <span class="text-[var(--text-primary)]">{downloadPct}%</span>
                  </div>
                  <div class="h-1.5 rounded-full bg-[var(--border,#2a2a2a)] overflow-hidden">
                    <div class="h-full bg-[var(--accent)] transition-all" style="width: {downloadPct}%"></div>
                  </div>
                </div>
              {:else if selectedBackend === 'whisper'}
                <!-- Whisper model list — downloadable and selectable -->
                {#each ALL_MODELS as model (model.id)}
                  {@const path = installedPath(model.id)}
                  {@const isSelected = path && path === cfgModel}
                  <div class="flex items-start justify-between gap-2 p-2 rounded-md border border-[var(--border,#2a2a2a)]">
                    <div class="flex flex-col gap-0.5 min-w-0">
                      <div class="flex items-center gap-2 min-w-0">
                        <span class="text-[12px] font-medium text-[var(--text-primary)] truncate">{model.label}</span>
                        {#if model.recommended}
                          <span class="shrink-0 text-[9px] uppercase tracking-wide px-1.5 py-0.5 rounded bg-emerald-400 text-black">Recommended</span>
                        {/if}
                      </div>
                      <span class="text-[11px] text-[var(--text-secondary)] leading-snug">{model.description}</span>
                    </div>
                    <div class="shrink-0 flex items-center gap-2">
                      <span class="text-[11px] font-mono text-[var(--text-secondary)]">{model.size}</span>
                      {#if path}
                        <button onclick={() => selectModel(path)}
                          disabled={isSelected}
                          class="px-2 py-1 rounded-md border border-[var(--border,#3a3a3a)] text-[11px] font-medium text-[var(--text-primary)] hover:bg-white/5 disabled:opacity-60 disabled:hover:bg-transparent transition-colors">
                          {isSelected ? 'Selected' : 'Select'}
                        </button>
                      {:else}
                        <button onclick={() => downloadModel(model)}
                          class="px-2 py-1 rounded-md bg-[var(--accent)] text-white text-[11px] font-medium hover:opacity-90 transition-opacity">
                          Download
                        </button>
                      {/if}
                    </div>
                  </div>
                {/each}
              {:else if selectedBackend === 'moonshine' || selectedBackend === 'parakeet'}
                {#each altModels as model (model.id)}
                  <div class="flex items-start justify-between gap-2 p-2 rounded-md border border-[var(--border,#2a2a2a)]">
                    <div class="flex flex-col gap-0.5 min-w-0">
                      <div class="flex items-center gap-2 min-w-0 flex-wrap">
                        <span class="text-[12px] font-medium text-[var(--text-primary)]">{model.tier}</span>
                        <span class="text-[10px] font-mono px-1.5 py-0.5 rounded border border-[var(--border,#3a3a3a)] text-[var(--text-secondary)]">{model.label}</span>
                        {#if model.recommended}
                          <span class="shrink-0 text-[9px] uppercase tracking-wide px-1.5 py-0.5 rounded bg-emerald-400 text-black">Recommended</span>
                        {/if}
                      </div>
                      <span class="text-[11px] text-[var(--text-secondary)] leading-snug">{model.description}</span>
                    </div>
                    <div class="shrink-0 flex items-center gap-2">
                      <span class="text-[11px] font-mono text-[var(--text-secondary)]">{model.size}</span>
                      {#if downloadingModel === model.id}
                        <span class="text-[11px] text-[var(--text-primary)]">{downloadPct}%</span>
                      {:else if model.installed}
                        <span class="text-[11px] text-emerald-400">Ready</span>
                      {:else}
                        <button onclick={() => downloadAltModel(model)}
                          class="px-2 py-1 rounded-md bg-[var(--accent)] text-white text-[11px] font-medium hover:opacity-90 transition-opacity">
                          Download
                        </button>
                      {/if}
                    </div>
                  </div>
                {/each}
                {#if altModels.length === 0}
                  <p class="text-[11px] text-[var(--text-secondary)] leading-snug">Loading models…</p>
                {/if}
              {/if}

              {#if downloadError}
                <p class="text-[11px] text-red-400 leading-snug">{downloadError}</p>
              {/if}
            {/if}
          </div>
        </div>

        <!-- Connector 4 → 5 -->
        <div class="h-3.5 ml-[23px] w-1 rounded-full {connectorClass(stepStates.model === 'done')}"></div>

        <!-- Step 5: Launch at Login -->
        <div class="flex gap-3 p-3.5 rounded-lg border {stepClass(stepStates.launch)}">
          <div class="shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-[11px] font-semibold {badgeClass(stepStates.launch)}">
            {stepStates.launch === 'done' ? '✓' : '5'}
          </div>
          <div class="flex flex-col gap-2 min-w-0 flex-1">
            <div class="flex flex-col gap-0.5">
              <h2 class="text-[13px] font-medium leading-tight text-[var(--text-primary)]">Launch at Login</h2>
              <p class="text-[11px] text-[var(--text-secondary)] leading-snug">
                Turbo Talk starts quietly when you sign in, so the menu bar trigger is ready.
              </p>
            </div>
            {#if stepStates.launch === 'active'}
              <div class="flex gap-2">
                <button onclick={enableLaunchAtLogin} disabled={launchPromptInFlight}
                  class="px-3 py-1.5 rounded-md bg-green-600 text-white text-[12px] font-medium hover:bg-green-500 disabled:opacity-50 transition-colors">
                  {launchPromptInFlight ? 'Enabling…' : 'Enable Automatic Login'}
                </button>
                <button onclick={() => { launchSkipped = true; stopPolling(); onComplete?.(); }} disabled={launchPromptInFlight}
                  class="px-3 py-1.5 rounded-md bg-[var(--accent)] text-white text-[12px] font-medium hover:opacity-90 disabled:opacity-50 transition-opacity">
                  Skip →
                </button>
              </div>
              {#if launchError}
                <p class="text-[11px] text-red-400 leading-snug">{launchError}</p>
              {/if}
            {/if}
          </div>
        </div>

      </div>
      {/if}
    {:else}
      <p class="text-[12px] text-[var(--text-secondary)]">Checking system…</p>
    {/if}

    {#if readiness?.force_onboarding}
      {@const allDone = Object.values(stepStates).every(s => s === 'done')}
      <div class="flex flex-col items-center gap-1.5 mt-1">
        {#if allDone}
          <p class="text-[11px] font-medium text-green-400">All checks complete</p>
        {/if}
        <button
          onclick={() => { stopPolling(); onComplete?.(); }}
          class="px-5 py-2 rounded-md text-white text-[13px] font-semibold transition-colors
                 {allDone ? 'bg-green-600 hover:bg-green-500' : 'bg-orange-500 hover:bg-orange-400'}">
          Close ✓
        </button>
      </div>
    {/if}

  </div>
</div>
