<script>
  // First-launch (and any-launch-with-missing-prerequisites) gate.
  //
  // Polls `commands.checkReadiness()` every second while open so each row
  // flips green the moment the user grants permission in System Settings or
  // a model finishes downloading. Step 1 is Accessibility; Step 2 is
  // Input Monitoring; Step 3 is Microphone; Step 4 is Model selection;
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
  let launchError        = $state('');
  let downloadingModel   = $state(null);
  let downloadPct        = $state(0);
  let downloadError      = $state('');
  let restartArmed       = $state(false);

  const RECOMMENDED = {
    id: 'ggml-large-v3-turbo',
    label: 'Large v3 Turbo',
    size: '1.6 GB',
    description: 'Best accuracy for daily dictation · multilingual · fast',
  };

  const ALTERNATES = [
    { id: 'ggml-large-v3-turbo-q5_0', label: 'Large v3 Turbo (q5_0)',
      size: '574 MB', description: 'Quantized · lower accuracy, lower RAM' },
    { id: 'ggml-large-v3', label: 'Large v3',
      size: '3.1 GB', description: 'Maximum accuracy · slowest' },
  ];

  async function refresh() {
    const [nextReadiness, nextLaunchAtLogin] = await Promise.all([
      commands.checkReadiness(),
      commands.getLaunchAtLogin(),
    ]);
    readiness = nextReadiness;
    launchAtLogin = nextLaunchAtLogin;
    if (readiness.ready && launchAtLogin) {
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
    if (!unsupportedPlatform && !launchAtLogin) {
      await enableLaunchAtLogin();
    }
    startPolling();
  });
  onDestroy(stopPolling);

  async function openAccessibility() {
    // Calls AXIsProcessTrustedWithOptions(prompt: true). Side-effects:
    //   1. macOS auto-adds Turbo Talk to the Accessibility list (off).
    //   2. macOS shows its native "would like to use Accessibility" prompt
    //      with a built-in "Open System Preferences" button.
    // We then deep-link to the pane ourselves as a fallback in case the
    // user dismissed the prompt without clicking through.
    await commands.promptForAccessibility();
    await commands.openSystemSettings('accessibility');
    restartArmed = true;
  }

  async function restart() {
    await commands.restartApp();
  }

  function delay(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
  }

  async function openInputMonitoring() {
    inputPromptInFlight = true;
    try {
      const status = await commands.requestInputMonitoringPermission();
      // macOS may need a moment to add the current bundle to the Input
      // Monitoring list after IOHIDRequestAccess. Opening Settings too soon
      // can land on a pane where Turbo Talk is not listed yet, forcing the
      // user through the manual file-picker path.
      if (status !== 'granted') {
        await delay(1200);
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
      }
    } finally {
      launchPromptInFlight = false;
      await refresh();
    }
  }

  async function downloadModel(modelId) {
    downloadingModel = modelId;
    downloadPct      = 0;
    downloadError    = '';
    const unlisten = await listen('download-progress', (e) => {
      if (e.payload?.name === modelId) {
        downloadPct = e.payload.pct ?? 0;
      }
    });
    try {
      const res = await commands.downloadModel(modelId);
      if (res.status === 'error') {
        downloadError = res.error || 'Download failed.';
        return;
      }
      const cfg = await commands.getConfig();
      if (!cfg.whisper) cfg.whisper = { bin: 'auto', model: '', models: [] };
      cfg.whisper.model = res.data;
      if (!cfg.whisper.models.includes(res.data)) {
        cfg.whisper.models = [...cfg.whisper.models, res.data];
      }
      await commands.saveConfig(cfg);
      await refresh();
    } finally {
      unlisten();
      downloadingModel = null;
    }
  }

  let stepStates = $derived.by(() => {
    if (!readiness) return { accessibility: 'active', input_monitoring: 'pending', microphone: 'pending', model: 'pending', launch: 'pending' };
    const a = readiness.accessibility === 'granted';
    const i = readiness.input_monitoring === 'granted';
    const m = readiness.microphone    === 'granted';
    const p = readiness.model_present;
    return {
      accessibility: a ? 'done' : 'active',
      input_monitoring: i ? 'done' : (a ? 'active' : 'pending'),
      microphone:    m ? 'done' : (a && i ? 'active' : 'pending'),
      model:         p ? 'done' : (a && i && m ? 'active' : 'pending'),
      launch:        launchAtLogin ? 'done' : (a && i && m && p ? 'active' : 'pending'),
    };
  });

  let unsupportedPlatform = $derived(
    readiness?.accessibility === 'unsupported'
      || readiness?.input_monitoring === 'unsupported'
      || readiness?.microphone === 'unsupported'
  );

  function stepClass(state) {
    if (state === 'active') return 'border-[var(--accent)]/40 bg-[var(--accent)]/5';
    if (state === 'done')   return 'border-[var(--border,#2a2a2a)] opacity-60';
    return 'border-[var(--border,#2a2a2a)] opacity-40';
  }
  function badgeClass(state) {
    if (state === 'done')   return 'bg-emerald-500/20 text-emerald-400';
    if (state === 'active') return 'bg-[var(--accent)] text-white';
    return 'bg-[var(--border,#2a2a2a)] text-[var(--muted,#9a9a9a)]';
  }
</script>

<div class="fixed inset-0 z-[100] bg-[var(--surface)] flex flex-col overflow-hidden">
  <div class="max-w-[420px] w-full mx-auto px-6 py-8 flex flex-col gap-5">

    <div class="flex flex-col gap-1.5">
      <h1 class="text-[18px] font-semibold leading-tight">Welcome to Turbo Talk</h1>
      <p class="text-[12px] text-[var(--muted,#9a9a9a)] leading-relaxed">
        {unsupportedPlatform
          ? 'This beta is macOS-only for recording, global hotkeys, and paste.'
          : 'Three quick setup steps before you can start dictating.'}
      </p>
    </div>

    {#if readiness}
      {#if unsupportedPlatform}
        <div class="flex flex-col gap-3 p-3.5 rounded-lg border border-yellow-500/30 bg-yellow-500/10">
          <div class="flex flex-col gap-1">
            <h2 class="text-[13px] font-medium leading-tight text-yellow-200">Unsupported platform</h2>
            <p class="text-[11px] text-[var(--muted,#9a9a9a)] leading-snug">
              Turbo Talk's beta dictation loop currently depends on macOS Accessibility,
              Input Monitoring, microphone permission, and paste APIs. Those controls are unavailable here,
              so recording and paste will not work on this platform.
            </p>
          </div>
          <button onclick={() => onUnsupportedContinue?.()}
            class="self-start px-3 py-1.5 rounded-md border border-yellow-500/40 text-[12px] font-medium text-yellow-100 hover:bg-yellow-500/15 transition-colors">
            Continue without dictation
          </button>
        </div>
      {:else}
      <!-- Step 1: Accessibility (restart required, surfaced first) -->
      <div class="flex gap-3 p-3.5 rounded-lg border {stepClass(stepStates.accessibility)}">
        <div class="shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-[11px] font-semibold {badgeClass(stepStates.accessibility)}">
          {stepStates.accessibility === 'done' ? '✓' : '1'}
        </div>
        <div class="flex flex-col gap-2 min-w-0 flex-1">
          <div class="flex flex-col gap-0.5">
            <h2 class="text-[13px] font-medium leading-tight">Allow Accessibility</h2>
            <p class="text-[11px] text-[var(--muted,#9a9a9a)] leading-snug">
              Turbo Talk needs Accessibility permission to read your push-to-talk hotkey globally.
              Granting this requires restarting the app once.
            </p>
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
              <p class="text-[11px] text-[var(--muted,#9a9a9a)] leading-snug">
                Toggle Turbo Talk on under Privacy &amp; Security → Accessibility, then click Restart.
              </p>
            {/if}
          {/if}
        </div>
      </div>

      <!-- Step 2: Input Monitoring -->
      <div class="flex gap-3 p-3.5 rounded-lg border {stepClass(stepStates.input_monitoring)}">
        <div class="shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-[11px] font-semibold {badgeClass(stepStates.input_monitoring)}">
          {stepStates.input_monitoring === 'done' ? '✓' : '2'}
        </div>
        <div class="flex flex-col gap-2 min-w-0 flex-1">
          <div class="flex flex-col gap-0.5">
            <h2 class="text-[13px] font-medium leading-tight">Allow Input Monitoring</h2>
            <p class="text-[11px] text-[var(--muted,#9a9a9a)] leading-snug">
              This lets Turbo Talk receive the push-to-talk key while another app is focused.
              Restart once after turning it on.
            </p>
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
            <p class="text-[11px] text-[var(--muted,#9a9a9a)] leading-snug">
              Toggle Turbo Talk on under Privacy &amp; Security → Input Monitoring.
            </p>
          {/if}
        </div>
      </div>

      <!-- Step 3: Microphone -->
      <div class="flex gap-3 p-3.5 rounded-lg border {stepClass(stepStates.microphone)}">
        <div class="shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-[11px] font-semibold {badgeClass(stepStates.microphone)}">
          {stepStates.microphone === 'done' ? '✓' : '3'}
        </div>
        <div class="flex flex-col gap-2 min-w-0 flex-1">
          <div class="flex flex-col gap-0.5">
            <h2 class="text-[13px] font-medium leading-tight">Allow Microphone</h2>
            <p class="text-[11px] text-[var(--muted,#9a9a9a)] leading-snug">
              So Turbo Talk can record your voice while you hold the hotkey. Audio never leaves your machine.
            </p>
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
              <p class="text-[11px] text-[var(--muted,#9a9a9a)] leading-snug">
                Toggle Turbo Talk on under Privacy &amp; Security → Microphone.
              </p>
            {/if}
          {/if}
        </div>
      </div>

      <!-- Step 4: Model -->
      <div class="flex gap-3 p-3.5 rounded-lg border {stepClass(stepStates.model)}">
        <div class="shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-[11px] font-semibold {badgeClass(stepStates.model)}">
          {stepStates.model === 'done' ? '✓' : '4'}
        </div>
        <div class="flex flex-col gap-2 min-w-0 flex-1">
          <div class="flex flex-col gap-0.5">
            <h2 class="text-[13px] font-medium leading-tight">Download a transcription model</h2>
            <p class="text-[11px] text-[var(--muted,#9a9a9a)] leading-snug">
              Whisper runs locally on your Mac. Pick a model — the recommended one fits most users.
            </p>
          </div>
          {#if stepStates.model === 'active'}
            {#if downloadingModel}
              <div class="flex flex-col gap-1.5">
                <div class="flex items-center justify-between text-[11px]">
                  <span class="truncate text-[var(--muted,#9a9a9a)]">{downloadingModel}</span>
                  <span class="text-[var(--text-primary)]">{downloadPct}%</span>
                </div>
                <div class="h-1.5 rounded-full bg-[var(--border,#2a2a2a)] overflow-hidden">
                  <div class="h-full bg-[var(--accent)] transition-all" style="width: {downloadPct}%"></div>
                </div>
              </div>
            {:else}
              <button onclick={() => downloadModel(RECOMMENDED.id)}
                class="flex items-start justify-between gap-3 p-2.5 rounded-md border border-[var(--border,#2a2a2a)] hover:border-[var(--accent)]/60 text-left transition-colors">
                <div class="flex flex-col gap-0.5 min-w-0">
                  <div class="flex items-center gap-2">
                    <span class="text-[12px] font-medium">{RECOMMENDED.label}</span>
                    <span class="text-[9px] uppercase tracking-wide px-1.5 py-0.5 rounded bg-[var(--accent)]/15 text-[var(--accent)]">Recommended</span>
                  </div>
                  <span class="text-[11px] text-[var(--muted,#9a9a9a)] leading-snug">{RECOMMENDED.description}</span>
                </div>
                <span class="shrink-0 text-[11px] font-mono text-[var(--muted,#9a9a9a)]">{RECOMMENDED.size}</span>
              </button>
              {#each ALTERNATES as m (m.id)}
                <button onclick={() => downloadModel(m.id)}
                  class="flex items-start justify-between gap-3 p-2.5 rounded-md border border-[var(--border,#2a2a2a)] hover:border-[var(--accent)]/60 text-left transition-colors">
                  <div class="flex flex-col gap-0.5 min-w-0">
                    <span class="text-[12px] font-medium">{m.label}</span>
                    <span class="text-[11px] text-[var(--muted,#9a9a9a)] leading-snug">{m.description}</span>
                  </div>
                  <span class="shrink-0 text-[11px] font-mono text-[var(--muted,#9a9a9a)]">{m.size}</span>
                </button>
              {/each}
              {#if downloadError}
                <p class="text-[11px] text-red-400 leading-snug">{downloadError}</p>
              {/if}
            {/if}
          {/if}
        </div>
      </div>

      <!-- Step 5: Launch at Login -->
      <div class="flex gap-3 p-3.5 rounded-lg border {stepClass(stepStates.launch)}">
        <div class="shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-[11px] font-semibold {badgeClass(stepStates.launch)}">
          {stepStates.launch === 'done' ? '✓' : '5'}
        </div>
        <div class="flex flex-col gap-2 min-w-0 flex-1">
          <div class="flex flex-col gap-0.5">
            <h2 class="text-[13px] font-medium leading-tight">Launch at Login</h2>
            <p class="text-[11px] text-[var(--muted,#9a9a9a)] leading-snug">
              Turbo Talk starts quietly when you sign in, so the menu bar trigger is ready.
            </p>
          </div>
          {#if stepStates.launch === 'active'}
            <button onclick={enableLaunchAtLogin} disabled={launchPromptInFlight}
              class="self-start px-3 py-1.5 rounded-md bg-[var(--accent)] text-white text-[12px] font-medium hover:opacity-90 disabled:opacity-50 transition-opacity">
              {launchPromptInFlight ? 'Enabling…' : 'Enable Launch at Login'}
            </button>
            {#if launchError}
              <p class="text-[11px] text-red-400 leading-snug">{launchError}</p>
            {/if}
          {/if}
        </div>
      </div>
      {/if}
    {:else}
      <p class="text-[12px] text-[var(--muted,#9a9a9a)]">Checking system…</p>
    {/if}

  </div>
</div>
