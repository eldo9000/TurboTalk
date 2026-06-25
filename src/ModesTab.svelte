<script>
  import { PROMPT_PRESETS } from './lib/prompts';
  import { seg } from './lib/utils';

  let {
    cfgBackend, cfgCleanupMode, cfgStripFillers, cfgAppendPeriod,
    cfgStripArtifacts, cfgOllamaUrl, cfgLlmModel, cfgVocabulary,
    cfgAntiVocabulary, cfgClassifierPrompt, activePresetId,
    cfgVadEnabled, ollamaReachable, ollamaModelPresent,
    ollamaModelPartial, ollamaPullState,
    actions,
  } = $props();

  function promptActive(p) {
    return cfgClassifierPrompt === p.prompt || activePresetId === p.id;
  }
</script>

<div class="flex-1 min-h-0 overflow-y-auto pb-4 bg-[var(--surface)]">
  <div class="tt-set" style={cfgCleanupMode === 'chaperone' ? 'min-height:auto' : ''}>
    <div class="tt-section">
      <div class="subsection-hd"><span class="subsection-hd-title">Post-processing</span></div>
      <div class="tt-row tt-row-field">
        <div class="tt-seg tt-seg-wide">
          {#each [['off','Off'],['regex','Simple'],['chaperone','Advanced']] as [v, lbl], i}
            <button onclick={() => actions.setCleanupMode(v)} class={seg(cfgCleanupMode === v, i, 3)}>{lbl}</button>
          {/each}
        </div>
      </div>
      <div class="tt-row tt-row-col">
        <p class="tt-desc">
          {#if cfgCleanupMode === 'off'}
            Paste raw Whisper output — no formatting, no changes.
          {:else if cfgCleanupMode === 'regex'}
            Capitalizes the first letter. Fast, deterministic, works offline.
          {:else}
            Routes transcript through a local Ollama model for intent-aware formatting. Sends transcript to your local Ollama server (localhost only — no internet).
          {/if}
        </p>
      </div>

      {#if cfgCleanupMode !== 'off'}
        <div class="tt-row tt-row-col tt-check-stack-list">
          {#each [
            ['strip_fillers',   cfgStripFillers,   actions.setStripFillers,   'Strip filler words',      'Removes um, uh, er, hmm.'],
            ['append_period',   cfgAppendPeriod,   actions.setAppendPeriod,   'Append period',           'Adds a period if no punctuation present.'],
            ['strip_artifacts', cfgStripArtifacts, actions.setStripArtifacts, 'Strip Whisper artifacts', 'Removes trailing " ." and "..." on silence.'],
          ] as [key, val, setter, label, desc]}
            <label class="tt-check-row tt-check-row-stacked">
              <input
                type="checkbox"
                class="cb-native"
                checked={val}
                onchange={() => setter(!val)}
              />
              <div class="tt-check-stack">
                <span class="tt-check-lbl tt-check-lbl-strong">{label}</span>
                <p class="tt-check-desc">{desc}</p>
              </div>
            </label>
          {/each}
        </div>
      {/if}
    </div>

    <div class="tt-section {cfgCleanupMode === 'chaperone' ? '' : 'tt-section-last'}" class:tt-muted={cfgBackend !== 'whisper'}>
      <div class="subsection-hd"><span class="subsection-hd-title">Whisper</span></div>
      <div class="tt-row tt-row-field" data-tip="Skip silent regions before transcription — prevents hallucination on silence and speeds up long recordings">
        <span class="tt-lbl">Silence Filter</span>
        <div class="tt-multi">
          <button
            onclick={() => actions.setVadEnabled(!cfgVadEnabled)}
            class="tt-multi-btn" class:tt-multi-on={cfgVadEnabled}
            disabled={cfgBackend !== 'whisper'}
            data-tip="Silero VAD pre-filter — when on, whisper-server skips silent regions before transcribing">Skip silent regions (VAD)</button>
        </div>
      </div>
      <div class="tt-row tt-row-col">
        <label for="custom-vocabulary" class="tt-lbl tt-lbl-fixed">Custom vocabulary</label>
        <textarea
          id="custom-vocabulary"
          value={cfgVocabulary}
          onchange={(e) => actions.setVocabulary(e.currentTarget.value)}
          rows="4"
          placeholder={"One word or phrase per line…\nTurbo Talk\nOllama\ggml-base"}
          class="tt-input tt-mono"
          disabled={cfgBackend !== 'whisper'}
          spellcheck="false"
        ></textarea>
        <p class="tt-desc">Domain terms Whisper tends to mishear. Applied as <code class="tt-code">--prompt</code> bias every transcription.</p>
      </div>
      {#if cfgBackend !== 'whisper'}
        <p class="tt-yellow">Silence Filter and Custom vocabulary require the Whisper backend. Switch to Whisper in Models → Transcription Engine.</p>
      {/if}
    </div>

    <div class="tt-section tt-section-last">
      <div class="subsection-hd"><span class="subsection-hd-title">Corrections</span></div>
      <div class="tt-row tt-row-col">
        <label for="anti-vocabulary" class="tt-lbl tt-lbl-fixed">Anti-vocabulary</label>
        <textarea
          id="anti-vocabulary"
          value={cfgAntiVocabulary}
          onchange={(e) => actions.setAntiVocabulary(e.currentTarget.value)}
          rows="3"
          placeholder={"One per line. Bare word = removed; from→to = replaced.\ngroq→grok\nfluant→fluent"}
          class="tt-input tt-mono"
          spellcheck="false"
        ></textarea>
        <p class="tt-desc">Words to fix or remove after transcription — works with any backend. Use <code class="tt-code">word→replacement</code> to replace, or just <code class="tt-code">word</code> to drop it entirely.</p>
      </div>
    </div>
  </div>

  {#if cfgCleanupMode === 'chaperone'}
    <div class="tt-set adv-panel-in" style="min-height:auto">
      <div class="tt-section">
        <div class="subsection-hd">
          <span class="subsection-hd-title">Setup</span>
          {#if ollamaReachable && ollamaModelPresent}
            <span class="tt-status-ready">Ready</span>
          {/if}
        </div>

        {#if ollamaReachable === null}
          <div class="tt-row tt-row-action">
            <div class="tt-row-info">
              <span class="tt-check-lbl tt-check-lbl-strong">Checking Ollama…</span>
            </div>
            <button onclick={actions.refreshOllamaSetup} class="tt-btn">Refresh</button>
          </div>
        {:else if ollamaReachable === false}
          <div class="tt-row tt-row-action">
            <div class="tt-row-info">
              <span class="tt-check-lbl tt-check-lbl-strong">Ollama not running</span>
              <p class="tt-check-desc">Start the Ollama app, then click Refresh. Or install it if you haven't yet.</p>
            </div>
            <div class="flex flex-col gap-1.5 items-end">
              <button onclick={actions.refreshOllamaSetup} class="tt-btn">Refresh</button>
              <button onclick={actions.installOllama} class="tt-btn" style="font-size:10px;opacity:0.7">Install Ollama</button>
            </div>
          </div>
        {:else if ollamaReachable === true && !ollamaModelPresent}
          <div class="tt-row tt-row-action">
            <div class="tt-row-info">
              <span class="tt-check-lbl tt-check-lbl-strong">ollama reachable · classifier model missing</span>
              <p class="tt-check-desc">{cfgLlmModel || 'llama3.2:3b'} — not yet pulled</p>
              {#if ollamaPullState.inFlight}
                <div class="tt-progress-row">
                  <div class="tt-progress-track">
                    <div class="tt-progress-fill" style="width:{ollamaPullState.pct}%"></div>
                  </div>
                  <span class="tt-progress-pct">{ollamaPullState.pct}%</span>
                </div>
                {#if ollamaPullState.status}
                  <p class="tt-check-desc tt-truncate">{ollamaPullState.status}</p>
                {/if}
              {/if}
              {#if ollamaPullState.error}
                <p class="tt-check-desc" style="color:var(--error,#f87171)">{ollamaPullState.error}</p>
              {/if}
            </div>
            <button onclick={actions.startOllamaPull} disabled={ollamaPullState.inFlight} class="tt-btn">
              {ollamaPullState.inFlight ? '↓ …' : 'Download (~2GB)'}
            </button>
          </div>
        {:else if ollamaReachable === true && ollamaModelPresent}
          <div class="tt-row tt-row-action">
            <div class="tt-row-info">
              {#if ollamaModelPartial}
                <span class="tt-check-lbl tt-check-lbl-strong" style="color:var(--error,#f87171)">Incomplete download detected</span>
                <p class="tt-check-desc">The previous download was interrupted. Re-pull to fix.</p>
              {:else}
                <span class="tt-check-lbl tt-check-lbl-strong">Model present</span>
                <p class="tt-check-desc">Re-pull if the model is behaving incorrectly.</p>
              {/if}
              {#if ollamaPullState.inFlight}
                <div class="tt-progress-row">
                  <div class="tt-progress-track">
                    <div class="tt-progress-fill" style="width:{ollamaPullState.pct}%"></div>
                  </div>
                  <span class="tt-progress-pct">{ollamaPullState.pct}%</span>
                </div>
                {#if ollamaPullState.status}
                  <p class="tt-check-desc tt-truncate">{ollamaPullState.status}</p>
                {/if}
              {/if}
              {#if ollamaPullState.error}
                <p class="tt-check-desc" style="color:var(--error,#f87171)">{ollamaPullState.error}</p>
              {/if}
            </div>
            <button onclick={actions.startOllamaPull} disabled={ollamaPullState.inFlight} class="tt-btn" class:tt-btn-danger-hover={ollamaModelPartial && !ollamaPullState.inFlight}>
              {ollamaPullState.inFlight ? '↓ …' : ollamaModelPartial ? 'Fix Download' : 'Re-pull'}
            </button>
          </div>
        {/if}
      </div>

      <div class="tt-section">
        <div class="subsection-hd"><span class="subsection-hd-title">Ollama</span></div>
        <div class="tt-row tt-row-col">
          <label for="ollama-url" class="tt-lbl tt-lbl-fixed">URL</label>
          <input
            id="ollama-url"
            value={cfgOllamaUrl}
            onchange={(e) => actions.setOllamaUrl(e.currentTarget.value)}
            class="tt-input"
            spellcheck="false"
          />
        </div>
        <div class="tt-row tt-row-col">
          <label for="classifier-model" class="tt-lbl tt-lbl-fixed">Classifier model</label>
          <input
            id="classifier-model"
            value={cfgLlmModel}
            onchange={(e) => actions.setLlmModel(e.currentTarget.value)}
            placeholder="llama3.2:3b"
            class="tt-input"
            spellcheck="false"
          />
          <p class="tt-desc">Run <code class="tt-code">ollama pull llama3.2:3b</code> to fetch.</p>
        </div>
      </div>

      <div class="tt-section tt-section-last">
        <div class="subsection-hd"><span class="subsection-hd-title">Classifier prompt</span></div>
        <div class="tt-row tt-row-col">
          <div class="tt-multi tt-multi-wrap">
            {#each PROMPT_PRESETS as p (p.id)}
              <button
                onclick={() => actions.applyPreset(p)}
                class="tt-multi-btn"
                class:tt-multi-on={promptActive(p)}
              >{p.label}</button>
            {/each}
          </div>
          <textarea
            id="classifier-prompt"
            value={cfgClassifierPrompt}
            onchange={(e) => actions.setClassifierPrompt(e.currentTarget.value)}
            rows="10"
            class="tt-input tt-mono"
            spellcheck="false"
          ></textarea>
          <div class="tt-inline-foot">
            <p class="tt-desc"><code class="tt-code">{'{text}'}</code> replaced with transcript.</p>
            <button
              onclick={actions.resetClassifierPrompt}
              class="tt-reset-btn"
            >Reset</button>
          </div>
        </div>
      </div>
    </div>
  {/if}
</div>
