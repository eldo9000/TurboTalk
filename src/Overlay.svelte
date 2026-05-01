<script>
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWindow, primaryMonitor } from '@tauri-apps/api/window';
  import { LogicalPosition } from '@tauri-apps/api/dpi';

  let mode = $state('idle'); // 'idle' | 'recording' | 'transcribing'

  const W = 260;  // logical width — must match tauri.conf.json
  const H = 80;
  const BOTTOM_GAP = 110;

  onMount(async () => {
    const win = getCurrentWindow();

    // Position bottom-center of primary monitor
    const mon = await primaryMonitor();
    if (mon) {
      const sf = mon.scaleFactor;
      const mw = mon.size.width  / sf;
      const mh = mon.size.height / sf;
      await win.setPosition(new LogicalPosition(
        Math.round((mw - W) / 2),
        Math.round(mh - H - BOTTOM_GAP),
      ));
    }

    const uns = [];
    listen('ptt-down',   () => { mode = 'recording'; }).then(u => uns.push(u));
    listen('ptt-up',     () => { mode = 'transcribing'; }).then(u => uns.push(u));
    listen('transcript', () => {
      setTimeout(() => { mode = 'idle'; }, 350);
    }).then(u => uns.push(u));

    return () => uns.forEach(u => u());
  });
</script>

<style>
  :global(html, body, #app) {
    background: transparent !important;
    width: 100%; height: 100%;
    margin: 0; padding: 0; overflow: hidden;
  }

  .pill {
    opacity: 0;
    transform: scale(0.88) translateY(6px);
    transition: opacity 180ms ease-out, transform 180ms ease-out;
    pointer-events: none;
  }
  .pill.show {
    opacity: 1;
    transform: scale(1) translateY(0);
  }

  @keyframes wave {
    0%, 100% { transform: scaleY(0.22); }
    50%       { transform: scaleY(1); }
  }
  .bar {
    width: 3px;
    height: 26px;
    border-radius: 999px;
    transform-origin: center;
    animation: wave 0.7s ease-in-out infinite;
  }
  .bar.slow { animation-duration: 1.3s; }
</style>

<!-- full-window centering wrapper -->
<div class="w-full h-full flex items-center justify-center">
  <div
    class="pill flex items-center gap-3.5 px-5 py-3.5 rounded-2xl"
    class:show={mode !== 'idle'}
    style="background: rgba(16,16,16,0.87); backdrop-filter: blur(18px) saturate(160%);
           box-shadow: 0 8px 32px rgba(0,0,0,0.5), inset 0 1px 0 rgba(255,255,255,0.06);"
  >
    <!-- Animated waveform bars (symmetric pyramid delays) -->
    <div class="flex items-center gap-[3.5px]">
      {#each [0, 80, 160, 240, 160, 80, 0] as delay, i}
        <div
          class="bar {mode === 'transcribing' ? 'slow' : ''}"
          style="
            animation-delay: {delay}ms;
            background: {mode === 'recording' ? '#f87171' : '#fbbf24'};
          "
        ></div>
      {/each}
    </div>

    <!-- Status label -->
    <span
      class="text-[11px] font-semibold tracking-wide select-none"
      style="color: {mode === 'recording' ? '#f87171' : '#fbbf24'};"
    >
      {mode === 'recording' ? 'Recording' : 'Transcribing…'}
    </span>
  </div>
</div>
