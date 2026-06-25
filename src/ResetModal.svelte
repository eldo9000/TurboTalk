<script>
  import { commands } from './bindings.ts';

  let {
    open = false,
    closing = false,
    resetBusy = false,
    resetError = '',
    warmupResetBusy = false,
    warmupResetMsg = '',
    bugNote = $bindable(''),
    diagnosticMsg = '',
    platform = 'macos',
    onClose,
    onResetTurboTalk,
    onClearWarmupCache,
    onCreateBugReport,
  } = $props();
</script>

{#if open}
  <div
    class="about-backdrop {closing ? 'about-backdrop-out' : 'about-backdrop-in'}"
    onclick={(event) => {
      if (event.target === event.currentTarget) {
        onClose?.();
      }
    }}
    onkeydown={(event) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        onClose?.();
      }
    }}
    role="button"
    tabindex="0"
    aria-label="Close reset"
  >
    <div
      class="about-card reset-card {closing ? 'about-card-out' : 'about-card-in'}"
      role="dialog"
      aria-modal="true"
      tabindex="-1"
    >
      <button
        onclick={onClose}
        class="reset-close-x"
        aria-label="Close"
      >✕</button>
      <div class="reset-inner">
      <div class="flex flex-col items-start gap-1 pb-3">
        <span class="text-[18px] font-semibold tracking-tight text-[var(--text-primary)]">Reset TurboTalk</span>
        <p class="text-[var(--text-secondary)] text-[11px] leading-snug mt-1.5 w-full">
          Clear local settings and transcript history,<br>disable Launch at Login, and return to setup.
        </p>
        <div class="reset-platform-note">
          <span class="reset-platform-icon">⚠</span>
          <span>
            {#if platform === 'windows'}
              Microphone permissions stay in Settings › Privacy & security › Microphone.
            {:else if platform === 'linux'}
              Microphone permissions are managed through your system's portal or audio settings.
            {:else}
              Microphone permissions stay in macOS System Settings.
            {/if}
          </span>
        </div>
      </div>
      <div class="flex flex-col gap-1 pt-2.5">
        <div class="reset-action-row">
          <button onclick={() => onResetTurboTalk(false)} disabled={resetBusy} class="tt-btn reset-action-btn justify-center">
            Reset, Keep Models
          </button>
          <p class="reset-action-desc">Clears settings, transcript history, and warm-up. Keeps downloaded transcription models.</p>
        </div>
        <div class="reset-action-row">
          <button onclick={() => onResetTurboTalk(true)} disabled={resetBusy} class="tt-btn tt-btn-danger-hover reset-action-btn justify-center">
            Reset Everything
          </button>
          <p class="reset-action-desc">Clears everything including downloaded models. You'll need to download them again.</p>
        </div>
        <div class="reset-action-row">
          <button onclick={() => { commands.resetOnboarding(); onClose(); }} disabled={resetBusy} class="tt-btn reset-action-btn justify-center">
            Re-run Welcome Screen
          </button>
          <p class="reset-action-desc">Shows the setup wizard again without clearing any settings or models.</p>
        </div>
        <div class="reset-action-row">
          <button onclick={onClearWarmupCache} disabled={resetBusy || warmupResetBusy} class="tt-btn reset-action-btn justify-center">
            {warmupResetBusy ? 'Clearing…' : 'Clear warmup cache'}
          </button>
          <p class="reset-action-desc">Clears the transcription model warm-up so it reloads next time.</p>
        </div>

        {#if warmupResetMsg}
          <p class="text-[10px] text-[var(--text-muted)] break-all leading-snug">{warmupResetMsg}</p>
        {/if}
        {#if resetError}
          <p class="text-[10px] text-red-400 leading-snug">{resetError}</p>
        {/if}

        <div class="reset-action-row mt-1">
          <button onclick={onCreateBugReport} class="tt-btn reset-action-btn justify-center">Create Bug Report</button>
          <textarea
            id="bug-note"
            bind:value={bugNote}
            rows="2"
            placeholder={"Optional — what happened?\nThe report gathers the technical details."}
            class="tt-input reset-action-desc"
          ></textarea>
        </div>
        {#if diagnosticMsg}
          <p class="text-[10px] text-[var(--text-muted)] break-all leading-snug">{diagnosticMsg}</p>
        {/if}
      </div>
      </div>
    </div>
  </div>
{/if}
