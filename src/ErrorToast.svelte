<script>
  import { commands } from './bindings.ts';

  let { uiErrors = $bindable([]), onDismiss, onOpenSettings } = $props();
</script>

{#if uiErrors.length > 0}
  <div class="fixed top-12 left-1/2 -translate-x-1/2 z-50 flex flex-col gap-1.5 pointer-events-none w-[calc(100%-1.5rem)] max-w-[400px]">
    {#each uiErrors as err (err.id)}
      <button
        onclick={async () => {
          if (err.kind === 'hotkey-permission') {
            await commands.openSystemSettings('accessibility');
          } else if (err.kind === 'hotkey-input-monitoring') {
            await commands.openSystemSettings('input_monitoring');
          } else if (err.kind === 'mic-permission') {
            await commands.openSystemSettings('microphone');
          } else if (err.kind === 'chaperone-fallback') {
            onOpenSettings?.('modes');
          }
          onDismiss?.(err.id);
        }}
        class="pointer-events-auto px-3 py-2 rounded-lg flex items-center justify-between gap-2 text-left
               bg-red-500/10 border border-red-500/25 backdrop-blur-sm
               hover:bg-red-500/15 transition-colors cursor-pointer"
      >
        <div class="flex flex-col gap-0.5 min-w-0">
          <span class="text-[10px] uppercase tracking-wide text-red-400/70 font-mono">{err.kind}</span>
          <span class="text-[11px] text-red-400 leading-snug">{err.message}</span>
          {#if err.kind === 'hotkey-permission' || err.kind === 'hotkey-input-monitoring' || err.kind === 'mic-permission'}
            <span class="text-[10px] text-red-400/60 leading-snug">Click to open System Settings →</span>
          {/if}
        </div>
        <span class="shrink-0 text-red-400/60 hover:text-red-400 text-base leading-none">×</span>
      </button>
    {/each}
  </div>
{/if}
