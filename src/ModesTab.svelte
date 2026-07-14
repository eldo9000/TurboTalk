<script>
  import { seg } from './lib/utils';

  let {
    cfgBackend, cfgCleanupMode, cfgVocabulary,
    cfgAntiVocabulary,
    cfgVadEnabled,
    cfgFormatPunct, cfgFormatLiteral,
    cfgFormatStripFillers, cfgFormatStripArtifacts, cfgFormatCapitalize,
    actions,
  } = $props();
</script>

<div class="flex-1 min-h-0 overflow-y-auto pb-4 bg-[var(--surface)]">
  <div class="tt-set">
    <div class="tt-section">
      <div class="subsection-hd"><span class="subsection-hd-title">Post-processing</span></div>
      <div class="tt-row tt-row-field">
        <div class="tt-seg tt-seg-wide">
          {#each [['off','Off'],['text_formatter','Text Formatter']] as [v, lbl], i}
            <button onclick={() => actions.setCleanupMode(v)} class={seg(cfgCleanupMode === v, i, 2)}>{lbl}</button>
          {/each}
        </div>
      </div>
      <div class="tt-row tt-row-col">
        <p class="tt-desc">
          {#if cfgCleanupMode === 'off'}
            Paste raw Whisper output — no formatting, no changes.
          {:else}
            Deterministic rule-based formatting: spoken punctuation, slash commands, @mentions, and smart capitalization. No network, no model — works offline instantly.
          {/if}
        </p>
      </div>
    </div>

    <div class="tt-section tt-section-last" class:tt-muted={cfgBackend !== 'whisper'}>
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
      <div class="subsection-hd"><span class="subsection-hd-title">Replacements</span></div>
      <div class="tt-row tt-row-col">
        <label for="anti-vocabulary" class="tt-lbl tt-lbl-fixed">Word list</label>
        <textarea
          id="anti-vocabulary"
          value={cfgAntiVocabulary}
          onchange={(e) => actions.setAntiVocabulary(e.currentTarget.value)}
          rows="3"
          placeholder={"groq = grok\nfluant = fluent\naptible"}
          class="tt-input tt-mono"
          spellcheck="false"
        ></textarea>
        <p class="tt-desc">Persistent ASR misspellings. One per line: <code class="tt-code">from = to</code> replaces a word, bare <code class="tt-code">word</code> removes it. Spaces around <code class="tt-code">=</code> optional. Case-insensitive, whole-word only.</p>
      </div>
    </div>
  </div>

  {#if cfgCleanupMode === 'text_formatter'}
    <div class="tt-set adv-panel-in" style="min-height:auto">
      <div class="tt-section tt-section-last">
        <div class="subsection-hd">
          <span class="subsection-hd-title">Formatting Rules</span>
        </div>
        <p class="tt-desc" style="padding: 4px 0 10px">
          Each stage can be toggled independently. All stages run in sequence:
        </p>
        <div class="tt-row tt-row-col tt-check-stack-list">
          {#each [
            ['punct',   cfgFormatPunct,   actions.setFormatPunct,   'Spoken punctuation',   '"type period" → "."  ·  "type comma" → ","'],
            ['literal', cfgFormatLiteral, actions.setFormatLiteral, 'Slash & mentions',     '"slash deploy" → "/deploy"  ·  "at sign Bob" → "@Bob"'],
            ['fillers', cfgFormatStripFillers, actions.setFormatStripFillers, 'Strip fillers', 'Removes um, uh, er, hmm'],
            ['artifacts', cfgFormatStripArtifacts, actions.setFormatStripArtifacts, 'Strip artifacts', 'Removes trailing " ." and "..." from silence segments'],
            ['caps',    cfgFormatCapitalize, actions.setFormatCapitalize, 'Capitalize',         'First letter of each utterance'],
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
      </div>
    </div>
  {/if}
</div>
