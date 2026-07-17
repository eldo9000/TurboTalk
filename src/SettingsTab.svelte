<script>
  import { seg } from './lib/utils';
  import Select from '@libre/ui/src/components/Select.svelte';
  import UpdateManager from './UpdateManager.svelte';

  let {
    cfgHotkeyMode = $bindable(),
    cfgCancelOnEsc = $bindable(),
    cfgCancelOnHold = $bindable(),
    cfgAutoTapThreshold = $bindable(),
    cfgTheme = $bindable(),
    cfgLaunchLogin = $bindable(),
    cfgShowSplash = $bindable(),
    cfgDevice = $bindable(),
    audioDevices = [],
    cfgSaveHistory = $bindable(),
    cfgHistoryAutoDelete = $bindable(),
    cfgShowOverlay = $bindable(),
    cfgOverlaySize = $bindable(),
    cfgOverlayPosition = $bindable(),
    cfgCursorDotIndicator = $bindable(),
    cfgSoundOnStart = $bindable(),
    cfgSoundOnFinish = $bindable(),
    cfgSoundOnCancel = $bindable(),
    cfgSoundOnError = $bindable(),
    cfgSoundVolume = $bindable(),
    cfgPauseMediaOnDictate = $bindable(),
    ZOOM_LEVELS = [],
    zoomIdx = $bindable(),
    hotkeySide = $bindable(),
    hotkeyKeyPart = $bindable(),
    hotkeyKeyItems = [],
    hasLogitechMouse = false,
    platform = 'macos',
    settingsContentEl = $bindable(null),
    onIndicatorOver,
    onIndicatorLeave,
    onSaveSettings,
    onResetOpen,
    onApplyHotkey,
  } = $props();

  const HISTORY_AUTO_DELETE_ITEMS = [
    { value: 'restart', label: 'On app restart' },
    { value: '1d',      label: 'After 1 day'    },
    { value: '5d',      label: 'After 5 days'   },
    { value: '10d',     label: 'After 10 days'  },
    { value: '30d',     label: 'After 30 days'  },
  ];

  function isUnsidedKey(k) {
    return k.startsWith('numpad_') || k.startsWith('mouse_') || /^f\d+$/.test(k);
  }

  let volumeSaveTimer = null;
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_mouse_events_have_key_events -->
<div class="tt-set" bind:this={settingsContentEl}
  onmouseover={onIndicatorOver}
  onmouseleave={onIndicatorLeave}>

  <!-- Recording & Hotkey -->
  <div class="tt-section">
    <div class="subsection-hd"><span class="subsection-hd-title">Recording &amp; Hotkey</span></div>
    <div class="tt-row tt-row-field tt-recording-button-row" data-tip="Which side modifier to pair with the hotkey key">
      <span class="tt-lbl">Hotkey side</span>
      <div class="tt-seg" class:tt-seg-dim={isUnsidedKey(hotkeyKeyPart)}>
        {#each [['left','Left'],['right','Right']] as [v, lbl], i}
          <button onclick={() => { hotkeySide = v; onApplyHotkey(); }} class={seg(hotkeySide === v, i, 2)}
            data-tip="Press the {lbl} modifier (e.g. {lbl} Alt + your hotkey key)"
          >{lbl}</button>
        {/each}
      </div>
    </div>
    {#if hotkeyKeyPart.startsWith('mouse_')}
      {#if hasLogitechMouse}
        <div style="margin: 0 8px 6px; padding: 6px 9px; border-radius: 6px; font-size: 11px; line-height: 1.5; color: var(--warning, #c97d00); background: var(--warning-bg, #fff8e0); border: 1px solid color-mix(in srgb, var(--warning, #c97d00) 30%, transparent);">
          ⚠ Logitech Options+ blocks mouse button events — this hotkey won't trigger recording.<br>
          <strong>The fix:</strong> in Logi Options+, assign <em>Keystroke → F19</em> to the button, then pick <strong>F19</strong> above. Recording works and no native action fires.
        </div>
      {:else}
        <div style="margin: 0 8px 6px; padding: 6px 9px; border-radius: 6px; font-size: 11px; line-height: 1.5; color: var(--muted, #888); background: var(--bg-muted, #f5f5f5); border: 1px solid var(--border-subtle, #e0e0e0);">
          ⓘ Your mouse's back/forward action fires alongside recording — both happen at once. For a clean experience, assign <strong>F19</strong> to the button in your mouse software and pick it above.
        </div>
      {/if}
    {/if}
    <div class="tt-row tt-row-field tt-recording-button-row" data-tip="How the hotkey starts and stops recording">
      <span class="tt-lbl">Recording mode</span>
      <div class="tt-seg tt-seg-recording">
        {#each [['hold','Hold', 'Record while held; release to stop'], ['toggle','Toggle', 'Press to start, press again to stop'], ['auto','Auto', 'Tap to start/tap to stop, or hold for quick bursts']] as [v, lbl, btnTip], i}
          <button onclick={() => { cfgHotkeyMode = v; onSaveSettings(); }} class={seg(cfgHotkeyMode === v, i, 3)}
            data-tip={btnTip}
          >{lbl}</button>
        {/each}
      </div>
    </div>
    <div class="tt-row tt-row-field tt-recording-threshold-row" data-tip="Shorter is more responsive to taps; longer is more tolerant of slow presses">
      <label for="auto-threshold" class="tt-lbl tt-lbl-fixed" class:tt-threshold-disabled={cfgHotkeyMode !== 'auto'}>
        Tap threshold: {cfgAutoTapThreshold} ms
      </label>
      <input
        id="auto-threshold"
        type="range"
        min="200"
        max="1000"
        step="50"
        value={cfgAutoTapThreshold}
        disabled={cfgHotkeyMode !== 'auto'}
        oninput={(e) => { cfgAutoTapThreshold = Number(e.currentTarget.value); onSaveSettings(); }}
        class="tt-range"
        style="--pct:{((cfgAutoTapThreshold - 200) / 800) * 100}%"
      />
    </div>
    <div class="tt-row tt-row-field" data-tip="How to abort a recording in progress">
      <span class="tt-lbl">Cancel on</span>
      <div class="tt-multi">
        <button
          onclick={() => { cfgCancelOnEsc = !cfgCancelOnEsc; onSaveSettings(); }}
          class="tt-multi-btn" class:tt-multi-on={cfgCancelOnEsc}
          data-tip="Press Escape to cancel the current recording">Escape</button>
        <button
          onclick={() => { cfgCancelOnHold = !cfgCancelOnHold; onSaveSettings(); }}
          class="tt-multi-btn" class:tt-multi-on={cfgCancelOnHold}
          data-tip="Hold the hotkey for ~1 second during recording to cancel">Hold key</button>
      </div>
    </div>
    <div class="tt-row tt-row-field tt-recording-select-row" data-tip="Which key triggers push-to-talk">
      <span class="tt-lbl">Hotkey</span>
      <div class="tt-key-sel">
        <Select
          items={hotkeyKeyItems}
          bind:value={hotkeyKeyPart}
          onchange={onApplyHotkey}
          variant="flat"
          size="sm"
        />
      </div>
    </div>
    <div class="tt-row tt-row-field tt-recording-select-row" data-tip="Microphone to record from">
      <span class="tt-lbl">Microphone</span>
      <div class="tt-key-sel">
        <Select
          items={[
            { value: 'default', label: 'System default' },
            ...audioDevices.map(d => ({ value: d, label: d })),
          ]}
          bind:value={cfgDevice}
          onchange={() => onSaveSettings()}
          variant="flat"
          size="sm"
        />
      </div>
    </div>
  </div>

  <!-- Theme -->
  <div class="tt-section">
    <div class="subsection-hd"><span class="subsection-hd-title">Appearance</span></div>
    <div class="tt-row tt-row-field" data-tip="Follows your system theme — Auto matches macOS appearance">
      <span class="tt-lbl tt-lbl-fixed tt-appearance-label">Theme</span>
      <div class="tt-seg tt-setting-seg">
        {#each [['auto','Auto'],['light','Light'],['dark','Dark']] as [v, lbl], i}
          <button onclick={() => { cfgTheme = v; onSaveSettings(); }} class={seg(cfgTheme === v, i, 3)}>{lbl}</button>
        {/each}
      </div>
    </div>
    <div class="tt-row tt-row-field" data-tip="Scale the app interface — also adjustable with − / + in the footer">
      <span class="tt-lbl tt-lbl-fixed tt-zoom-label">Zoom</span>
      <div class="tt-seg tt-setting-seg">
        {#each ZOOM_LEVELS as level, i}
          <button onclick={() => { zoomIdx = i; }} class={seg(zoomIdx === i, i, ZOOM_LEVELS.length)}>{level}%</button>
        {/each}
      </div>
    </div>
  </div>

  <!-- Audio indicators (Volume embedded) -->
  <div class="tt-section">
    <div class="subsection-hd"><span class="subsection-hd-title">Indicators</span></div>
    <div class="tt-row tt-row-field" data-tip="Choose how much visual feedback the recording overlay shows">
      <span class="tt-lbl">Visual Overlay</span>
      <div class="tt-seg tt-setting-seg">
        <button
          onclick={() => { cfgShowOverlay = true; cfgOverlaySize = 'small'; onSaveSettings(); }}
          class={seg(cfgShowOverlay && cfgOverlaySize === 'small', 0, 3)}
          data-tip="Bare recording dot with timer">Small</button>
        <button
          onclick={() => { cfgShowOverlay = true; cfgOverlaySize = 'medium'; onSaveSettings(); }}
          class={seg(cfgShowOverlay && cfgOverlaySize === 'medium', 1, 3)}
          data-tip="Current compact waveform overlay">Medium</button>
        <button
          onclick={() => { cfgShowOverlay = true; cfgOverlaySize = 'large'; onSaveSettings(); }}
          class={seg(cfgShowOverlay && cfgOverlaySize === 'large', 2, 3)}
          data-tip="Expanded waveform overlay with stronger status text">Large</button>
      </div>
    </div>
    <div class="tt-row tt-row-field" data-tip="Where the recording overlay anchors on screen">
      <span class="tt-lbl">Overlay Position</span>
      <div class="tt-seg tt-setting-seg">
        <button
          onclick={() => { if (cfgShowOverlay) { cfgOverlayPosition = 'bottom'; onSaveSettings(); } }}
          class={seg(cfgOverlayPosition === 'bottom', 0, 2)}
          disabled={!cfgShowOverlay}
          data-tip="Pin the overlay near the bottom of the screen">Bottom</button>
        <button
          onclick={() => { if (cfgShowOverlay) { cfgOverlayPosition = 'top'; onSaveSettings(); } }}
          class={seg(cfgOverlayPosition === 'top', 1, 2)}
          disabled={!cfgShowOverlay}
          data-tip="Pin the overlay near the top of the screen">Top</button>
      </div>
    </div>
    <div class="tt-row tt-row-field" data-tip="Colored dot that follows the cursor while recording is active">
      <span class="tt-lbl">Cursor Dot</span>
      <div class="tt-multi">
        <button
          onclick={() => { cfgCursorDotIndicator = !cfgCursorDotIndicator; onSaveSettings(); }}
          class="tt-multi-btn" class:tt-multi-on={cfgCursorDotIndicator}
          data-tip="Track the cursor with a colored dot while recording">Follow Cursor</button>
      </div>
    </div>
    <div class="tt-row tt-row-field" data-tip="Play audio chimes for recording events">
      <span class="tt-lbl">Audio Notify</span>
      <div class="tt-multi">
        <button
          onclick={() => { cfgSoundOnStart = !cfgSoundOnStart; onSaveSettings(); }}
          class="tt-multi-btn" class:tt-multi-on={cfgSoundOnStart}
          data-tip="Play a chime when recording begins">on Start</button>
        <button
          onclick={() => { cfgSoundOnFinish = !cfgSoundOnFinish; onSaveSettings(); }}
          class="tt-multi-btn" class:tt-multi-on={cfgSoundOnFinish}
          data-tip="Play a chime when transcription completes">on Finish</button>
        <button
          onclick={() => { cfgSoundOnCancel = !cfgSoundOnCancel; onSaveSettings(); }}
          class="tt-multi-btn" class:tt-multi-on={cfgSoundOnCancel}
          data-tip="Play a chime when recording is cancelled">on Cancel</button>
        <button
          onclick={() => { cfgSoundOnError = !cfgSoundOnError; onSaveSettings(); }}
          class="tt-multi-btn" class:tt-multi-on={cfgSoundOnError}
          data-tip="Play a low beep when dictation has errors">on Error</button>
      </div>
    </div>
    <div class="tt-row tt-row-field" data-tip="Pause music/podcasts while dictating and resume after paste">
      <span class="tt-lbl">Media</span>
      <div class="tt-multi">
        <button
          onclick={() => { cfgPauseMediaOnDictate = !cfgPauseMediaOnDictate; onSaveSettings(); }}
          class="tt-multi-btn" class:tt-multi-on={cfgPauseMediaOnDictate}
          data-tip="Pause playback during dictation, resume after">Pause on Dictate</button>
      </div>
    </div>
    <div class="tt-row tt-row-field tt-recording-threshold-row" data-tip="Volume for chimes (macOS follows system Alert Volume)">
      <span class="tt-lbl tt-lbl-fixed">Volume: {Math.round(cfgSoundVolume * 100)}%</span>
      <input
        type="range"
        min="0" max="1" step="0.05"
        bind:value={cfgSoundVolume}
        oninput={() => { clearTimeout(volumeSaveTimer); volumeSaveTimer = setTimeout(onSaveSettings, 300); }}
        class="tt-range"
        style="--pct:{cfgSoundVolume * 100}%"
      />
    </div>
  </div>

  <!-- System -->
  <div class="tt-section tt-section-last">
    <div class="subsection-hd"><span class="subsection-hd-title">System</span></div>
    <div class="tt-row tt-row-field" data-tip="Save transcripts to disk and auto-delete after a set period">
      <button
        onclick={() => { cfgSaveHistory = !cfgSaveHistory; onSaveSettings(); }}
        class="tt-multi-btn" class:tt-multi-on={cfgSaveHistory}
        data-tip="Persist transcript history across app restarts">Save History</button>
      <div class="tt-key-sel tt-history-sel" data-tip="Automatically delete saved transcripts older than this">
        <Select
          items={HISTORY_AUTO_DELETE_ITEMS}
          bind:value={cfgHistoryAutoDelete}
          onchange={() => onSaveSettings()}
          disabled={!cfgSaveHistory}
          variant="flat"
          size="sm"
        />
      </div>
    </div>
    <div class="tt-row tt-row-field" data-tip="Start TurboTalk automatically when you log in to macOS">
      <span class="tt-lbl">Startup</span>
      <div class="tt-multi tt-system-actions tt-startup-actions">
        <button
          onclick={() => { cfgLaunchLogin = !cfgLaunchLogin; onSaveSettings(); }}
          class="tt-multi-btn" class:tt-multi-on={cfgLaunchLogin}>Launch at Login</button>
        <button
          onclick={() => { cfgShowSplash = !cfgShowSplash; onSaveSettings(); }}
          class="tt-multi-btn" class:tt-multi-on={cfgShowSplash}
          data-tip="Show the Turbo Talk splash window at startup">Show Splash</button>
      </div>
    </div>
    <div class="tt-row tt-row-field" data-tip="Reset settings and history, or check for a newer version">
      <div class="flex gap-2 w-full">
        <button
          onclick={onResetOpen}
          class="tt-btn tt-btn-danger-hover flex-1 justify-center"
        >
          Reset / Clear Caches
        </button>
        <div class="flex-1">
          <UpdateManager />
        </div>
      </div>
    </div>
  </div>

</div>
