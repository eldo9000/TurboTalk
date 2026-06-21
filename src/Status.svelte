<script>
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';

  let mode = $state('idle'); // 'idle' | 'arming'
  let message = $state('');

  function hide() {
    mode = 'idle';
    message = '';
    getCurrentWindow().hide();
  }

  onMount(() => {
    const win = getCurrentWindow();
    const uns = [];

    // Warm-up: whisper-server still loading after hotkey press
    listen('ptt-armed', () => {
      mode = 'arming';
      message = 'Starting\u2026';
      win.show();
    }).then(u => uns.push(u));

    // Warm-up failed (e.g. model not found)
    listen('ptt-arm-failed', () => {
      mode = 'arming';
      message = 'Model failed to load';
      win.show();
      setTimeout(hide, 3000);
    }).then(u => uns.push(u));

    // Recording started → status no longer needed
    listen('ptt-down', () => {
      hide();
    }).then(u => uns.push(u));

    // Rejection / error feedback is now handled by the main overlay
    // (Overlay.svelte) so it interrupts the recording pill directly.

    // Any end-of-job event → hide status
    const hideEvents = [
      'transcript', 'transcript-error', 'recording-discarded',
      'recording-cancelled', 'recording-recovered', 'recording-too-short',
      'device-lost', 'paste-error', 'paste-copied',
    ];
    hideEvents.forEach(ev => {
      listen(ev, () => hide()).then(u => uns.push(u));
    });

    // Start hidden
    hide();

    return () => uns.forEach(u => u());
  });
</script>

<div class="root" class:arming={mode === 'arming'}>
  <div class="card">
    <span class="card-message">{message}</span>
  </div>
</div>

<style>
  :global(html), :global(body), :global(#app) {
    background: transparent !important;
    margin: 0;
    padding: 0;
    overflow: hidden;
  }

  .root {
    width: 100vw;
    height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
  }

  .card {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 18px 22px;
    border-radius: 20px;
    background: rgba(16, 16, 16, 0.87);
    backdrop-filter: blur(18px) saturate(160%);
    -webkit-backdrop-filter: blur(18px) saturate(160%);
    border: 1px solid transparent;
    user-select: none;
    min-width: 220px;
    justify-content: center;
  }

  .arming .card {
    border-color: rgba(251, 191, 36, 0.85);
    animation: pulse-arming 1.15s ease-in-out infinite;
  }

  @keyframes pulse-arming {
    0%, 100% {
      border-color: rgba(251, 191, 36, 0.28);
      box-shadow: 0 0 0 0 rgba(251, 191, 36, 0);
    }
    45%, 55% {
      border-color: rgba(251, 191, 36, 1);
      box-shadow:
        0 0 0 3px rgba(251, 191, 36, 0.22),
        0 0 22px 5px rgba(251, 191, 36, 0.34);
    }
  }

  .card-message {
    font-size: 14px;
    font-weight: 700;
    line-height: 1;
    letter-spacing: 0;
    color: #fbbf24;
  }
</style>
