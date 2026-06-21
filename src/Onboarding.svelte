<script>
  // First-launch (and any-launch-with-missing-prerequisites) gate.
  //
  // Polls `commands.checkReadiness()` every second while open so each row
  // flips green the moment the user grants permission in System Settings or
  // a model finishes downloading. Step 1 is Input Monitoring; Step 2 is
  // Microphone; Step 3 is optional auto-paste; Step 4 is Model selection;
  // Step 5 is Launch at Login.
  //
  // Closes itself by calling `onComplete()` when readiness is fully green.

  import { onMount, onDestroy } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
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

  // Stale-entry fix: track whether user has already clicked "Open System
  // Settings" for a given step. Once set, show the fix-stale-entry UI if
  // the step is still blocked after they return.
  let imOpenedSettings   = $state(false);
  let axOpenedSettings   = $state(false);
  let fixImInFlight      = $state(false);
  let fixAxInFlight      = $state(false);
  let fixImError         = $state('');
  let fixAxError         = $state('');

  // Onboarding is Parakeet-only — other engines live in Settings after setup.
  const ONBOARDING_BACKEND = 'parakeet';

  // Populated from listModelsForFamily — we show the recommended variant only.
  let altModels = $state([]);
  let onboardingModel = $derived(altModels.find(m => m.recommended) ?? altModels[0] ?? null);

  let selectedModelReady = $derived(
    (readiness?.model_present ?? false) || (onboardingModel?.installed ?? false)
  );

  async function loadAltModels() {
    altModels = await commands.listModelsForFamily(ONBOARDING_BACKEND).catch(() => []);
    return altModels;
  }

  async function ensureParakeetBackend() {
    try {
      const cfg = await commands.getConfig();
      const models = await loadAltModels();
      const rec = models.find(m => m.recommended) ?? models[0];
      let changed = false;
      if (cfg.backend !== ONBOARDING_BACKEND) {
        cfg.backend = ONBOARDING_BACKEND;
        changed = true;
      }
      if (rec?.installed) {
        const variant = rec.id.replace(/^parakeet-/, '');
        if (cfg.backend_variant !== variant) {
          cfg.backend_variant = variant;
          changed = true;
        }
      }
      if (changed) await commands.saveConfig(cfg);
    } catch (e) {
      console.warn('ensureParakeetBackend failed', e);
    }
  }

  function permissionSatisfied(status) {
    return status === 'granted' || status === 'unsupported';
  }

  async function refresh() {
    launchAtLogin = await commands.getLaunchAtLogin();
    await ensureParakeetBackend();
    const nextReadiness = await commands.checkReadiness();
    readiness = nextReadiness;
    const rec = altModels.find(m => m.recommended) ?? altModels[0];
    const modelReady = nextReadiness.model_present || !!rec?.installed;
    if (
      !downloadingModel
      && permissionSatisfied(nextReadiness.input_monitoring)
      && permissionSatisfied(nextReadiness.microphone)
      && modelReady
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
    await ensureParakeetBackend();
    await refresh();
    startPolling();
  });
  onDestroy(() => {
    stopPolling();
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
        await refresh();
      }
    } finally {
      launchPromptInFlight = false;
    }
  }

  async function downloadParakeetModel() {
    const model = onboardingModel;
    if (!model) return;
    downloadingModel = model.id;
    downloadPct      = 0;
    downloadError    = '';
    const variant = model.id.replace(/^parakeet-/, '');
    const progressKey = `${ONBOARDING_BACKEND}-${variant}`;
    const unlisten = await listen('download-progress', (e) => {
      const name = e.payload?.name ?? '';
      if (name === model.id || name === progressKey || name.startsWith(progressKey)) {
        downloadPct = e.payload.pct ?? 0;
      }
    });
    try {
      const res = await commands.downloadParakeetModel(variant);
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
    const autoPaste = permissionSatisfied(readiness.automatic_paste);
    const i = permissionSatisfied(readiness.input_monitoring);
    const m = permissionSatisfied(readiness.microphone);
    const p = selectedModelReady;
    // macOS TCC steps only gate model selection; Windows/Linux skip that wait.
    const permissionsReady = readiness.platform === 'macos' ? (i && m) : readiness.platform !== 'linux';
    return {
      input_monitoring: i ? 'done' : 'active',
      microphone:    m ? 'done' : (i ? 'active' : 'pending'),
      accessibility: autoPaste ? 'done' : (i && m ? 'active' : 'pending'),
      model:         p ? 'done' : (permissionsReady ? 'active' : 'pending'),
      launch:        (launchAtLogin || launchSkipped) ? 'done' : (i && m && p ? 'active' : 'pending'),
    };
  });

  let dictationReady = $derived.by(() => {
    if (!readiness || readiness.platform === 'linux') return false;
    return permissionSatisfied(readiness.input_monitoring)
      && permissionSatisfied(readiness.microphone)
      && selectedModelReady;
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
  <div class="max-w-[420px] w-full mx-auto px-6 py-6 pb-6 flex flex-col gap-3.5">

    <div class="flex flex-col gap-1.5">
      <h1 class="text-[18px] font-semibold leading-tight text-[var(--text-primary)]">Welcome to Turbo Talk</h1>
      <p class="text-[12px] text-[var(--text-secondary)] leading-relaxed">
        {unsupportedPlatform
          ? 'This beta is not fully supported on Linux yet.'
          : 'Download the Parakeet model to start dictating. Other engines are in Settings later.'}
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

        <!-- Step 3: Auto-paste (optional; ad-hoc builds fall back to clipboard) -->
        <div class="flex gap-3 {stepStates.accessibility === 'done' ? 'p-3' : 'p-3.5'} rounded-lg border {stepClass(stepStates.accessibility)}">
          <div class="shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-[11px] font-semibold {badgeClass(stepStates.accessibility)}">
            {stepStates.accessibility === 'done' ? '✓' : '3'}
          </div>
          <div class="flex flex-col gap-2 min-w-0 flex-1">
            <div class="flex flex-col gap-0.5">
              <h2 class="text-[13px] font-medium leading-tight text-[var(--text-primary)]">Enable Auto-Paste</h2>
              {#if stepStates.accessibility !== 'done'}
                <p class="text-[11px] text-[var(--text-secondary)] leading-snug">
                  Dictation works without this. Turbo Talk will copy text to the clipboard and ask you to press Command-V.
                </p>
              {/if}
            </div>
            {#if stepStates.accessibility === 'active'}
              <div class="flex gap-2 flex-wrap">
                <button onclick={openAccessibility}
                  class="px-3 py-1.5 rounded-md bg-[var(--accent)] text-white text-[12px] font-medium hover:opacity-90 transition-opacity">
                  Try Auto-Paste
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
                  Toggle Turbo Talk on under Privacy &amp; Security → Accessibility, then restart to retry automatic paste.
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

        <!-- Step 4: Parakeet model download -->
        <div class="flex gap-3 p-3.5 rounded-lg border {stepClass(stepStates.model)}">
          <div class="shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-[11px] font-semibold {badgeClass(stepStates.model)}">
            {stepStates.model === 'done' ? '✓' : '4'}
          </div>
          <div class="flex flex-col gap-2 min-w-0 flex-1">
            <div class="flex flex-col gap-0.5">
              <h2 class="text-[13px] font-medium leading-tight text-[var(--text-primary)]">Download the Parakeet model</h2>
              <p class="text-[11px] text-[var(--text-secondary)] leading-snug">
                Fast English dictation — runs fully on your device. Whisper and Moonshine are available in Settings after setup.
              </p>
            </div>
            {#if stepStates.model === 'active' || stepStates.model === 'done'}
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
              {:else if onboardingModel}
                <div class="flex items-start justify-between gap-2 p-2 rounded-md border border-[var(--border,#2a2a2a)]">
                  <div class="flex flex-col gap-0.5 min-w-0">
                    <div class="flex items-center gap-2 min-w-0 flex-wrap">
                      <span class="text-[12px] font-medium text-[var(--text-primary)]">{onboardingModel.label}</span>
                    </div>
                    <span class="text-[11px] text-[var(--text-secondary)] leading-snug">{onboardingModel.description}</span>
                  </div>
                  <div class="shrink-0 flex items-center gap-2">
                    <span class="text-[11px] font-mono text-[var(--text-secondary)]">{onboardingModel.size}</span>
                    {#if onboardingModel.installed}
                      <span class="text-[11px] text-emerald-400">Ready</span>
                    {:else}
                      <button onclick={downloadParakeetModel}
                        class="px-2 py-1 rounded-md bg-[var(--accent)] text-white text-[11px] font-medium hover:opacity-90 transition-opacity">
                        Download
                      </button>
                    {/if}
                  </div>
                </div>
              {:else}
                <p class="text-[11px] text-[var(--text-secondary)] leading-snug">Loading model…</p>
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

      {#if readiness && !unsupportedPlatform && !readiness.force_onboarding}
        {#if dictationReady}
          <button
            onclick={() => { stopPolling(); onComplete?.(); }}
            class="mt-1 px-5 py-2 rounded-md bg-green-600 hover:bg-green-500 text-white text-[13px] font-semibold transition-colors"
          >
            Get started
          </button>
        {/if}
      {/if}
    {:else}
      <p class="text-[12px] text-[var(--text-secondary)]">Checking system…</p>
    {/if}

    {#if readiness?.force_onboarding}
      <div class="flex flex-col items-center gap-1.5 mt-1">
        {#if dictationReady}
          <p class="text-[11px] font-medium text-green-400">All checks complete</p>
        {/if}
        <button
          onclick={() => { stopPolling(); onComplete?.(); }}
          class="px-5 py-2 rounded-md text-white text-[13px] font-semibold transition-colors
                 {dictationReady ? 'bg-green-600 hover:bg-green-500' : 'bg-orange-500 hover:bg-orange-400'}">
          Close ✓
        </button>
      </div>
    {/if}

  </div>
</div>
