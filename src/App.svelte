<script>
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-shell';
  import { initTheme } from '@libre/ui/src/theme.js';

  let recording    = $state(false);
  let transcribing = $state(false);
  let activeTab    = $state('history');

  // History — in-memory for now, most recent first
  let history = $state([]);

  // Settings form
  let cfgBin   = $state('');
  let cfgModel = $state('');
  let saveMsg  = $state('');

  async function openSettings() {
    const cfg = await invoke('get_config');
    cfgBin   = cfg.whisper.bin;
    cfgModel = cfg.whisper.model;
    saveMsg  = '';
  }

  async function saveSettings() {
    const cfg = await invoke('get_config');
    cfg.whisper.bin   = cfgBin;
    cfg.whisper.model = cfgModel;
    try {
      await invoke('save_config', { cfg });
      saveMsg = 'Saved.';
    } catch (e) {
      saveMsg = 'Error: ' + e;
    }
  }

  function switchTab(tab) {
    activeTab = tab;
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

  <!-- Titlebar: drag region + tabs in one row -->
  <div data-tauri-drag-region class="h-10 shrink-0 flex items-end select-none border-b border-[var(--border,#2a2a2a)]">
    <!-- traffic light spacer -->
    <div class="w-[76px] shrink-0 h-full" data-tauri-drag-region></div>

    <!-- tabs -->
    {#each ['history', 'settings'] as tab}
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

    <!-- drag region fill -->
    <div class="flex-1 h-full" data-tauri-drag-region></div>

    <!-- recording indicator in titlebar -->
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
        <div class="flex-1 min-h-0 overflow-y-auto px-4 py-3 flex flex-col gap-2">
          {#each history as item (item.ts)}
            <p class="text-[var(--text-primary)] text-sm leading-relaxed
                       border-b border-[var(--border,#2a2a2a)] pb-2 last:border-0">
              {item.text}
            </p>
          {/each}
        </div>
      {/if}
    </div>
  {/if}

  <!-- Settings tab -->
  {#if activeTab === 'settings'}
    <div class="flex-1 min-h-0 overflow-y-auto flex flex-col gap-3 px-4 py-4">

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
        <span class="text-[var(--text-secondary)] text-xs">Model path</span>
        <input
          bind:value={cfgModel}
          class="text-xs bg-[var(--surface-raised)] border border-[var(--border)]
                 rounded px-2 py-1.5 text-[var(--text-primary)] outline-none
                 focus:border-[var(--accent)]"
          spellcheck="false"
        />
        <span class="text-[var(--text-tertiary,#666)] text-[10px] leading-relaxed">
          Models live in <code class="font-mono">~/.config/librewin/turbotalk/models/</code>.
          <button
            onclick={() => open('https://huggingface.co/ggerganov/whisper.cpp/tree/main')}
            class="underline hover:text-[var(--text-secondary)] transition-colors"
          >Download from HuggingFace</button>
          or <code class="font-mono">brew install whisper-cpp</code> (includes whisper-cli).
        </span>
      </label>

      <div class="flex items-center gap-3 pt-1">
        <button
          onclick={saveSettings}
          class="text-xs px-3 py-1.5 rounded bg-[var(--accent)] text-white
                 hover:opacity-90 transition-opacity"
        >Save</button>
        {#if saveMsg}
          <span class="text-xs text-[var(--text-secondary)]">{saveMsg}</span>
        {/if}
      </div>
    </div>
  {/if}

</div>
