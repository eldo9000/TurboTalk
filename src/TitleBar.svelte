<script>
  let { activeTab, recording, transcribing, onTabSwitch } = $props();
</script>

<div data-tauri-drag-region class="relative h-10 shrink-0 flex items-end select-none bg-white dark:bg-[color-mix(in_srgb,#000_18%,var(--surface-raised))]">

  <!-- Traffic-light spacer (left) -->
  <div class="w-[76px] shrink-0 h-full" data-tauri-drag-region></div>

  <!-- Drag fill (right) -->
  <div class="flex-1 h-full" data-tauri-drag-region></div>

  <!-- Recording status dot (right side, doesn't affect centering) -->
  {#if recording || transcribing}
    <div class="flex items-center pb-1.5 pr-3">
      <span class={
        recording ? 'w-2 h-2 rounded-full bg-red-500 animate-pulse'
                  : 'w-2 h-2 rounded-full bg-yellow-400 animate-pulse'
      }></span>
    </div>
  {/if}

  <!-- All tabs — centered in the titlebar -->
  <div class="absolute inset-y-0 left-0 right-0 flex items-end justify-center pointer-events-none">
    {#each ['history', 'models', 'edits', 'settings'] as tab}
      <button
        onclick={() => onTabSwitch(tab)}
        class="tt-tab relative px-3 h-full text-[12px] font-medium capitalize transition-[color,opacity] pointer-events-auto focus:outline-none focus-visible:outline-none
               {activeTab === tab
                 ? 'text-[var(--text-primary)]'
                 : 'text-[var(--text-secondary)] opacity-40 hover:opacity-90'}"
      >
        {tab}
        {#if activeTab === tab}
          <span class="absolute bottom-0 left-2 right-2 h-[2px] rounded-t bg-[var(--accent)]"></span>
        {/if}
      </button>
    {/each}
  </div>

</div>
