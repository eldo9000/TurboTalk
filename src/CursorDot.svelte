<script>
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';

  // 'recording' = red, 'transcribing' = yellow
  let phase = $state('recording');

  onMount(() => {
    const uns = [];
    const reset = [
      'transcript', 'transcript-error', 'transcription-rejected', 'recording-discarded',
      'recording-cancelled', 'recording-too-short', 'device-lost', 'paste-error', 'paste-miss',
    ];

    listen('ptt-down',  () => { phase = 'recording';    }).then(u => uns.push(u));
    listen('ptt-armed', () => { phase = 'recording';    }).then(u => uns.push(u));
    listen('ptt-up',    () => { phase = 'transcribing'; }).then(u => uns.push(u));
    reset.forEach(ev => {
      listen(ev, () => { phase = 'recording'; }).then(u => uns.push(u));
    });

    return () => uns.forEach(u => u());
  });
</script>

<div class="wrap">
  <div class="dot" class:yellow={phase === 'transcribing'}></div>
</div>

<style>
  :global(html, body, #app) {
    background: transparent !important;
    margin: 0;
    padding: 0;
    overflow: hidden;
  }

  .wrap {
    width: 20px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
  }

  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: #ef4444;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.45);
    transition: background-color 120ms ease-out;
  }

  .dot.yellow {
    background: #fbbf24;
  }
</style>
