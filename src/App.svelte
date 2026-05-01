<script>
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { LogicalSize } from '@tauri-apps/api/dpi';
  import { open } from '@tauri-apps/plugin-shell';
  import { initTheme } from '@libre/ui/src/theme.js';

  let recording    = $state(false);
  let transcribing = $state(false);
  let activeTab    = $state('history');

  // History
  let history  = $state([]);
  let copiedTs = $state(null);

  // Models tab
  let cfgModels      = $state([]);
  let cfgModel       = $state('');
  let newModelPath   = $state('');
  let modelsSaveMsg  = $state('');

  // Settings tab
  let cfgBin         = $state('');
  let cfgCleanupMode = $state('regex');
  let cfgOllamaUrl   = $state('');
  let cfgLlmModel    = $state('');
  let settingsSaveMsg = $state('');

  // Single ref for the auto-fit container (whichever tab is active)
  let autofitEl = $state(null);

  const HISTORY_H  = 280;
  const WINDOW_W   = 380;
  const TITLEBAR_H = 40;

  $effect(() => {
    if (activeTab === 'history' || !autofitEl) {
      if (activeTab === 'history') {
        getCurrentWindow().setSize(new LogicalSize(WINDOW_W, HISTORY_H));
      }
      return;
    }
    const ro = new ResizeObserver(() => {
      const h = Math.min(TITLEBAR_H + autofitEl.offsetHeight, 700);
      getCurrentWindow().setSize(new LogicalSize(WINDOW_W, h));
    });
    ro.observe(autofitEl);
    return () => ro.disconnect();
  });

  // ── History ───────────────────────────────────────────────────────────────

  async function copyItem(item) {
    await navigator.clipboard.writeText(item.text);
    copiedTs = item.ts;
    setTimeout(() => { if (copiedTs === item.ts) copiedTs = null; }, 1500);
  }

  // ── Models ────────────────────────────────────────────────────────────────

  const MODEL_CATALOG = [
    {
      name: 'ggml-base.en',
      size: '141 MB',
      description: 'Fast · English only · recommended',
      url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin',
    },
    {
      name: 'ggml-small.en',
      size: '466 MB',
      description: 'Better accuracy · English only',
      url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin',
    },
    {
      name: 'ggml-medium.en',
      size: '1.5 GB',
      description: 'High accuracy · English only',
      url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.en.bin',
    },
  ];

  async function openModels() {
    const cfg = await invoke('get_config');
    cfgModel  = cfg.whisper.model;
    cfgModels = cfg.whisper.models ?? [];
    if (cfgModels.length === 0 && cfgModel) cfgModels = [cfgModel];
    modelsSaveMsg = '';
  }

  function addModel() {
    const path = newModelPath.trim();
    if (!path || cfgModels.includes(path)) return;
    cfgModels = [...cfgModels, path];
    if (cfgModels.length === 1) cfgModel = path;
    newModelPath = '';
  }

  function removeModel(path) {
    cfgModels = cfgModels.filter(m => m !== path);
    if (cfgModel === path) cfgModel = cfgModels[0] ?? '';
  }

  async function refreshModels() {
    const found = await invoke('scan_models_dir');
    const merged = [...new Set([...cfgModels, ...found])];
    cfgModels = merged;
    if (!cfgModel && merged.length > 0) cfgModel = merged[0];
  }

  async function saveModels() {
    const cfg = await invoke('get_config');
    cfg.whisper.model  = cfgModel;
    cfg.whisper.models = cfgModels;
    try {
      await invoke('save_config', { cfg });
      modelsSaveMsg = 'Saved.';
    } catch (e) {
      modelsSaveMsg = 'Error: ' + e;
    }
  }

  // ── Settings ──────────────────────────────────────────────────────────────

  async function openSettings() {
    const cfg = await invoke('get_config');
    cfgBin         = cfg.whisper.bin;
    cfgCleanupMode = cfg.cleanup.mode;
    cfgOllamaUrl   = cfg.cleanup.ollama_url;
    cfgLlmModel    = cfg.cleanup.classifier_model;
    settingsSaveMsg = '';
  }

  async function saveSettings() {
    const cfg = await invoke('get_config');
    cfg.whisper.bin              = cfgBin;
    cfg.cleanup.mode             = cfgCleanupMode;
    cfg.cleanup.ollama_url       = cfgOllamaUrl;
    cfg.cleanup.classifier_model = cfgLlmModel;
    try {
      await invoke('save_config', { cfg });
      settingsSaveMsg = 'Saved.';
    } catch (e) {
      settingsSaveMsg = 'Error: ' + e;
    }
  }

  function switchTab(tab) {
    activeTab = tab;
    if (tab === 'models')   openModels();
    if (tab === 'settings') openSettings();
  }

  onMount(() => {
    initTheme(invoke);
    const unlisteners = [];
    listen('ptt-down', () => { recording = true; }).then(u => unlisteners.push(u));
    listen('ptt-up',   () => { recording = false; }).then(u => unlisteners.push(u));
    listen('transcript', (e) => {
      const text = e.payload;
      if (text) history = [{ text, ts: Date.now() }, ...history].slice(0, 50);
    }).then(u => unlisteners.push(u));
    return () => unlisteners.forEach(u => u());
  });
</script>

<div class="flex flex-col h-full bg-[var(--surface)] overflow-hidden">

  <!-- Titlebar -->
  <div data-tauri-drag-region class="h-10 shrink-0 flex items-end select-none border-b border-[var(--border,#2a2a2a)]">
    <div class="w-[76px] shrink-0 h-full" data-tauri-drag-region></div>

    {#each ['history', 'models', 'settings'] as tab}
      <button
        onclick={() => switchTab(tab)}
        class="relative px-3 h-full text-xs font-medium capitalize transition-colors
               {activeTab === tab
                 ? 'text-[var(--text-primary)]'
                 : 'text-[var(--text-tertiary,#666)] hover:text-[var(--text-secondary)]'}"
      >
        {tab}
        {#if activeTab === tab}
          <span class="absolute bottom-0 left-2 right-2 h-[2px] rounded-t bg-[var(--accent)]"></span>
        {/if}
      </button>
    {/each}

    <div class="flex-1 h-full" data-tauri-drag-region></div>

    {#if recording || transcribing}
      <div class="flex items-center gap-1.5 pr-3 pb-1.5">
        <span class={
          recording ? 'w-2 h-2 rounded-full bg-red-500 animate-pulse'
                    : 'w-2 h-2 rounded-full bg-yellow-400 animate-pulse'
        }></span>
      </div>
    {/if}
  </div>

  <!-- History tab -->
  {#if activeTab === 'history'}
    <div class="flex-1 min-h-0 flex flex-col">
      {#if history.length === 0}
        <div class="flex-1 flex flex-col items-center justify-center gap-2">
          <p class="text-[var(--text-tertiary,#666)] text-sm select-none">
            {recording ? 'Recording…' : transcribing ? 'Transcribing…' : 'Hold Right Alt to record'}
          </p>
        </div>
      {:else}
        <div class="flex-1 min-h-0 overflow-y-auto px-3 py-2 flex flex-col gap-1">
          {#each history as item (item.ts)}
            <button
              onclick={() => copyItem(item)}
              title="Click to copy"
              class="w-full text-left text-sm leading-relaxed px-2 py-1.5 rounded
                     transition-colors cursor-pointer select-text
                     border-b border-[var(--border,#2a2a2a)] last:border-0
                     hover:bg-[var(--surface-raised)]
                     {copiedTs === item.ts ? 'text-[var(--accent)]' : 'text-[var(--text-primary)]'}"
            >
              {copiedTs === item.ts ? 'Copied!' : item.text}
            </button>
          {/each}
        </div>
      {/if}
    </div>
  {/if}

  <!-- Models tab -->
  {#if activeTab === 'models'}
    <div bind:this={autofitEl} class="flex flex-col gap-3 px-4 py-4">

      <!-- Active model selector -->
      <label class="flex flex-col gap-1">
        <span class="text-[var(--text-secondary)] text-xs">Active model</span>
        <select
          bind:value={cfgModel}
          class="text-xs bg-[var(--surface-raised)] border border-[var(--border)]
                 rounded px-2 py-1.5 text-[var(--text-primary)] outline-none
                 focus:border-[var(--accent)]"
        >
          {#if cfgModels.length === 0}
            <option value="">No models added</option>
          {:else}
            {#each cfgModels as m}
              <option value={m}>{m.split('/').at(-1)}</option>
            {/each}
          {/if}
        </select>
      </label>

      <!-- Model list -->
      <div class="flex flex-col gap-1">
        <span class="text-[var(--text-secondary)] text-xs">Installed models</span>

        {#if cfgModels.length === 0}
          <p class="text-[var(--text-tertiary,#666)] text-xs py-1">No models yet.</p>
        {:else}
          <div class="flex flex-col gap-0.5">
            {#each cfgModels as m}
              <div class="flex items-center gap-2 group">
                <span class="flex-1 text-xs text-[var(--text-primary)] font-mono truncate
                             py-1 px-2 rounded bg-[var(--surface-raised)]
                             border border-[var(--border)]"
                      title={m}>{m}</span>
                <button
                  onclick={() => removeModel(m)}
                  class="shrink-0 w-5 h-5 flex items-center justify-center rounded
                         text-[var(--text-tertiary,#666)] hover:text-red-400
                         hover:bg-[var(--surface-raised)] transition-colors text-xs"
                  title="Remove"
                >×</button>
              </div>
            {/each}
          </div>
        {/if}

        <!-- Add row -->
        <div class="flex items-center gap-2 mt-1">
          <input
            bind:value={newModelPath}
            onkeydown={(e) => e.key === 'Enter' && addModel()}
            placeholder="Paste model path…"
            class="flex-1 text-xs bg-[var(--surface-raised)] border border-[var(--border)]
                   rounded px-2 py-1.5 text-[var(--text-primary)] outline-none
                   focus:border-[var(--accent)] placeholder:text-[var(--text-tertiary,#666)]"
            spellcheck="false"
          />
          <button
            onclick={addModel}
            class="shrink-0 w-6 h-6 flex items-center justify-center rounded
                   bg-[var(--accent)] text-white hover:opacity-90 transition-opacity
                   text-base leading-none"
            title="Add model"
          >+</button>
        </div>

      </div>

      <!-- Download catalog -->
      <div class="flex flex-col gap-0.5">
        <span class="text-[var(--text-secondary)] text-xs mb-0.5">Available models</span>
        {#each MODEL_CATALOG as m}
          <div class="flex items-center gap-2 py-1.5 border-b border-[var(--border,#2a2a2a)] last:border-0">
            <div class="flex-1 min-w-0">
              <span class="text-xs font-mono text-[var(--text-primary)]">{m.name}</span>
              <span class="text-[10px] text-[var(--text-tertiary,#666)] ml-1.5">{m.size}</span>
              <p class="text-[10px] text-[var(--text-tertiary,#666)] mt-0.5">{m.description}</p>
            </div>
            <button
              onclick={() => open(m.url)}
              class="shrink-0 text-[10px] px-2 py-1 rounded border border-[var(--accent)]
                     text-[var(--accent)] hover:bg-[var(--accent)] hover:text-white
                     transition-colors whitespace-nowrap"
            >↓ Download</button>
          </div>
        {/each}
      </div>

      <!-- Footer: directory hint + refresh -->
      <div class="flex items-start gap-2 pt-0.5">
        <p class="flex-1 text-[var(--text-tertiary,#666)] text-[10px] leading-relaxed">
          Place <code class="font-mono">.bin</code> files in
          <code class="font-mono">~/.config/librewin/turbotalk/models/</code>
          then refresh to add them to the list above.
        </p>
        <button
          onclick={refreshModels}
          title="Scan models directory"
          class="shrink-0 text-[10px] px-2 py-1 rounded border border-[var(--border,#2a2a2a)]
                 text-[var(--text-secondary)] hover:border-[var(--accent)]
                 hover:text-[var(--accent)] transition-colors whitespace-nowrap"
        >↻ Refresh</button>
      </div>

      <div class="flex items-center gap-3 pt-1 pb-1">
        <button
          onclick={saveModels}
          class="text-xs px-3 py-1.5 rounded bg-[var(--accent)] text-white
                 hover:opacity-90 transition-opacity"
        >Save</button>
        {#if modelsSaveMsg}
          <span class="text-xs text-[var(--text-secondary)]">{modelsSaveMsg}</span>
        {/if}
      </div>
    </div>
  {/if}

  <!-- Settings tab -->
  {#if activeTab === 'settings'}
    <div bind:this={autofitEl} class="flex flex-col gap-3 px-4 py-4">

      <label class="flex flex-col gap-1">
        <span class="text-[var(--text-secondary)] text-xs">Whisper binary</span>
        <input
          bind:value={cfgBin}
          class="text-xs bg-[var(--surface-raised)] border border-[var(--border)]
                 rounded px-2 py-1.5 text-[var(--text-primary)] outline-none
                 focus:border-[var(--accent)]"
          spellcheck="false"
        />
      </label>

      <label class="flex flex-col gap-1">
        <span class="text-[var(--text-secondary)] text-xs">Cleanup mode</span>
        <select
          bind:value={cfgCleanupMode}
          class="text-xs bg-[var(--surface-raised)] border border-[var(--border)]
                 rounded px-2 py-1.5 text-[var(--text-primary)] outline-none
                 focus:border-[var(--accent)]"
        >
          <option value="off">Off — paste raw whisper output</option>
          <option value="regex">Regex — capitalize, trim (default)</option>
          <option value="chaperone">Chaperone — local LLM via Ollama</option>
        </select>
      </label>

      {#if cfgCleanupMode === 'chaperone'}
        <label class="flex flex-col gap-1">
          <span class="text-[var(--text-secondary)] text-xs">Ollama URL</span>
          <input
            bind:value={cfgOllamaUrl}
            class="text-xs bg-[var(--surface-raised)] border border-[var(--border)]
                   rounded px-2 py-1.5 text-[var(--text-primary)] outline-none
                   focus:border-[var(--accent)]"
            spellcheck="false"
          />
        </label>

        <label class="flex flex-col gap-1">
          <span class="text-[var(--text-secondary)] text-xs">Classifier model</span>
          <input
            bind:value={cfgLlmModel}
            class="text-xs bg-[var(--surface-raised)] border border-[var(--border)]
                   rounded px-2 py-1.5 text-[var(--text-primary)] outline-none
                   focus:border-[var(--accent)]"
            spellcheck="false"
            placeholder="llama3.2:3b"
          />
          <span class="text-[var(--text-tertiary,#666)] text-[10px]">
            Run <code class="font-mono">ollama pull llama3.2:3b</code> to fetch the default model.
          </span>
        </label>
      {/if}

      <div class="flex items-center gap-3 pt-1 pb-1">
        <button
          onclick={saveSettings}
          class="text-xs px-3 py-1.5 rounded bg-[var(--accent)] text-white
                 hover:opacity-90 transition-opacity"
        >Save</button>
        {#if settingsSaveMsg}
          <span class="text-xs text-[var(--text-secondary)]">{settingsSaveMsg}</span>
        {/if}
      </div>
    </div>
  {/if}

</div>
