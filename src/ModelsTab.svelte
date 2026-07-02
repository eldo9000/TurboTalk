<script>
  import { KNOWN_FILENAMES, altModelVariant, altModelActive } from './lib/catalog';
  import { seg } from './lib/utils';

  const ENGINE_OPTIONS = [
    ['parakeet', 'Parakeet'],
    ['whisper', 'Whisper'],
  ];

  const RECOMMENDED_MODEL = {
    name: 'ggml-large-v3-turbo',
    tier: 'Recommended',
    size: '1.6 GB',
    description: 'multilingual · best accuracy',
    url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin',
  };

  const MODEL_CATALOG = [
    {
      name: 'ggml-large-v3-turbo-q5_0',
      tier: 'Small',
      size: '574 MB',
      description: 'low RAM, english only, not bad',
      url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin',
    },
    {
      name: 'ggml-large-v3',
      tier: 'Large',
      size: '3.1 GB',
      description: 'high accuracy, high RAM, slow',
      url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin',
    },
  ];

  let {
    cfgBackend, cfgModels, cfgModel, downloadProgress, deletingModels,
    altModels, newModelPath, modelConfigured, cfgBackendVariant,
    actions,
  } = $props();

  const customPath = $derived(
    cfgModels.find(p => !KNOWN_FILENAMES.some(fn => p.endsWith(fn))) ?? ''
  );
</script>

{#snippet modelRow(m)}
  {@const filename      = m.name + '.bin'}
  {@const installedPath = cfgModels.find(p => p.endsWith(filename))}
  {@const isInstalled   = !!installedPath}
  {@const isSelected    = isInstalled && cfgModel === installedPath}
  {@const isDownloading = m.name in downloadProgress}
  {@const pct           = downloadProgress[m.name] ?? 0}
  <div class="tt-model-row group">
    <div class="tt-row-info">
      <div class="tt-model-name-row">
        <span class="tt-tier-name">{m.tier}</span>
        <span class="tt-model-name-pill">{m.name}</span>
      </div>
      <span class="tt-model-desc" class:tt-warn={m.warn}>{m.description}</span>
    </div>
    <span class="tt-model-size">{m.size}</span>
    {#if isDownloading}
      <span class="tt-model-pct">{pct}%</span>
      <button onclick={() => actions.cancelDownload(m.name)} class="tt-btn tt-btn-danger">Cancel</button>
    {:else if !isInstalled}
      <button onclick={() => actions.startDownload(m)} class="tt-btn">Download</button>
    {:else if isSelected}
      {#if deletingModels.has(installedPath)}
        <button disabled class="tt-model-x tt-model-x-deleting"><span class="tt-model-spin"></span></button>
      {:else}
        <button onclick={() => actions.removeModel(installedPath)} title="Remove" class="tt-model-x">×</button>
      {/if}
      <button disabled class="tt-btn tt-btn-success">Selected</button>
    {:else}
      {#if deletingModels.has(installedPath)}
        <button disabled class="tt-model-x tt-model-x-deleting"><span class="tt-model-spin"></span></button>
      {:else}
        <button onclick={() => actions.removeModel(installedPath)} title="Remove" class="tt-model-x">×</button>
      {/if}
      <button onclick={() => actions.selectModel(installedPath)} class="tt-btn tt-btn-accent">Use</button>
    {/if}
  </div>
{/snippet}

{#snippet altModelActions(m, accent = false)}
  {@const isDownloading = m.id in downloadProgress}
  {@const pct           = downloadProgress[m.id] ?? 0}
  {@const isInstalled   = m.installed}
  {@const isActive      = altModelActive(m, cfgBackendVariant, cfgBackend)}
  {#if isDownloading}
    <span class="tt-model-pct" class:tt-model-pct-lg={accent}>{pct}%</span>
    <button onclick={() => actions.cancelAltDownload(m)} class="tt-btn" class:tt-btn-md={accent} class:tt-btn-danger={accent}>Cancel</button>
  {:else if !isInstalled}
    <button onclick={() => actions.startAltDownload(m)} class="tt-btn" class:tt-btn-md={accent} class:tt-btn-accent={accent}>Download</button>
  {:else if isActive}
    {#if deletingModels.has(m.id)}
      <button disabled class="tt-model-x tt-model-x-deleting" class:tt-model-x-lg={accent} class:tt-model-x-deleting-lg={accent}><span class="tt-model-spin"></span></button>
    {:else}
      <button onclick={() => actions.removeAltModel(m)} title="Remove" class="tt-model-x" class:tt-model-x-lg={accent}>×</button>
    {/if}
    <button disabled class="tt-btn" class:tt-btn-md={accent} class:tt-btn-success={accent}>Selected</button>
  {:else}
    {#if deletingModels.has(m.id)}
      <button disabled class="tt-model-x tt-model-x-deleting" class:tt-model-x-lg={accent} class:tt-model-x-deleting-lg={accent}><span class="tt-model-spin"></span></button>
    {:else}
      <button onclick={() => actions.removeAltModel(m)} title="Remove" class="tt-model-x" class:tt-model-x-lg={accent}>×</button>
    {/if}
    <button onclick={() => actions.selectAltModel(m)} class="tt-btn" class:tt-btn-md={accent} class:tt-btn-accent={!accent}>Use</button>
  {/if}
{/snippet}

{#snippet altModelRow(m, accent = false)}
  {@const isActive = altModelActive(m, cfgBackendVariant, cfgBackend)}
  {#if accent}
    <div class="tt-model-card group" class:tt-model-card-selected={isActive}>
      <div class="tt-model-card-hd">
        <span class="tt-model-star">★</span>
        <span class="tt-model-star-lbl">Recommended</span>
      </div>
      <div class="tt-model-card-body">
        <div class="tt-row-info">
          <div class="tt-model-name-row">
            <span class="tt-tier-name">{m.tier}</span>
            <span class="tt-model-name-pill">{m.label}</span>
          </div>
          <span class="tt-desc">{m.description}</span>
        </div>
        <span class="tt-model-size">{m.size}</span>
        {@render altModelActions(m, true)}
      </div>
    </div>
  {:else}
    <div class="tt-model-row group">
      <div class="tt-row-info">
        <div class="tt-model-name-row">
          <span class="tt-tier-name">{m.tier}</span>
          <span class="tt-model-name-pill">{m.label}</span>
        </div>
        <span class="tt-model-desc">{m.description}</span>
      </div>
      <span class="tt-model-size">{m.size}</span>
      {@render altModelActions(m, false)}
    </div>
  {/if}
{/snippet}

<div class="flex-1 min-h-0 overflow-y-auto bg-[var(--surface)]">
  <div class="tt-set">
  <div class="tt-section">
    <div class="subsection-hd"><span class="subsection-hd-title">Transcription Engine</span></div>
    <div class="tt-row tt-row-field" data-tip="Which local transcription engine to use. Download a model below after switching.">
      <div class="tt-seg tt-seg-wide">
        {#each ENGINE_OPTIONS as [v, lbl], i}
          <button onclick={() => actions.setTranscriptionEngine(v)} class={seg(cfgBackend === v, i, ENGINE_OPTIONS.length)}>{lbl}</button>
        {/each}
      </div>
    </div>
    {#if cfgBackend === 'parakeet'}
      <p class="px-3 pb-2 text-[10px] text-[var(--text-secondary)] leading-snug">Recommended default · English-only · fastest. Download the model below.</p>
    {:else}
      <p class="px-3 pb-2 text-[10px] text-[var(--text-secondary)] leading-snug">Multilingual · most accurate. Model managed below.</p>
    {/if}
  </div>

  {#if cfgBackend === 'whisper'}
    {@const rmFilename      = RECOMMENDED_MODEL.name + '.bin'}
    {@const rmInstalledPath = cfgModels.find(p => p.endsWith(rmFilename))}
    {@const rmIsInstalled   = !!rmInstalledPath}
    {@const rmIsSelected    = rmIsInstalled && cfgModel === rmInstalledPath}
    {@const rmIsDownloading = RECOMMENDED_MODEL.name in downloadProgress}
    {@const rmPct           = downloadProgress[RECOMMENDED_MODEL.name] ?? 0}
    <div class="tt-section">
      <div class="subsection-hd"><span class="subsection-hd-title">Recommended</span></div>
      <div class="tt-row tt-row-field">
        <div class="tt-model-card group" class:tt-model-card-selected={rmIsSelected}>
          <div class="tt-model-card-hd">
            <span class="tt-model-star">★</span>
            <span class="tt-model-star-lbl">Recommended</span>
          </div>
          <div class="tt-model-card-body">
            <div class="tt-row-info">
              <div class="tt-model-name-row">
                <span class="tt-tier-name">{RECOMMENDED_MODEL.tier}</span>
                <span class="tt-model-name-pill">{RECOMMENDED_MODEL.name}</span>
              </div>
              <span class="tt-desc">{RECOMMENDED_MODEL.description}</span>
            </div>
            <span class="tt-model-size">{RECOMMENDED_MODEL.size}</span>
            {#if rmIsDownloading}
              <span class="tt-model-pct tt-model-pct-lg">{rmPct}%</span>
              <button onclick={() => actions.cancelDownload(RECOMMENDED_MODEL.name)} class="tt-btn tt-btn-md tt-btn-danger">Cancel</button>
            {:else if !rmIsInstalled}
              <button onclick={() => actions.startDownload(RECOMMENDED_MODEL)} class="tt-btn tt-btn-md tt-btn-accent">Download</button>
            {:else if rmIsSelected}
              {#if deletingModels.has(rmInstalledPath)}
                <button disabled class="tt-model-x tt-model-x-lg tt-model-x-deleting tt-model-x-deleting-lg"><span class="tt-model-spin"></span></button>
              {:else}
                <button onclick={() => actions.removeModel(rmInstalledPath)} title="Remove" class="tt-model-x tt-model-x-lg">×</button>
              {/if}
              <button disabled class="tt-btn tt-btn-md tt-btn-success">Selected</button>
            {:else}
              {#if deletingModels.has(rmInstalledPath)}
                <button disabled class="tt-model-x tt-model-x-lg tt-model-x-deleting tt-model-x-deleting-lg"><span class="tt-model-spin"></span></button>
              {:else}
                <button onclick={() => actions.removeModel(rmInstalledPath)} title="Remove" class="tt-model-x tt-model-x-lg">×</button>
              {/if}
              <button onclick={() => actions.selectModel(rmInstalledPath)} class="tt-btn tt-btn-md tt-btn-accent">Use</button>
            {/if}
          </div>
        </div>
      </div>
    </div>

    <div class="tt-section">
      <div class="subsection-hd"><span class="subsection-hd-title">Available</span></div>
      {#each MODEL_CATALOG as m}
        {@render modelRow(m)}
      {/each}
    </div>

    <div class="tt-section tt-section-last">
      <div class="subsection-hd"><span class="subsection-hd-title">Custom model</span></div>
      {#if customPath}
        <div class="tt-row tt-row-field">
          <div class="tt-custom-pill">
            <span class="tt-custom-name" title={customPath}>{customPath.split('/').at(-1)}</span>
            <span class="tt-custom-status">Connected</span>
            {#if deletingModels.has(customPath)}
              <button disabled class="tt-model-x tt-model-x-visible tt-model-x-deleting"><span class="tt-model-spin"></span></button>
            {:else}
              <button onclick={() => actions.removeModel(customPath)} title="Clear custom model" class="tt-model-x tt-model-x-visible">×</button>
            {/if}
          </div>
        </div>
      {:else}
        <div class="tt-row tt-row-field">
          <input
            value={newModelPath}
            oninput={(e) => actions.setNewModelPath(e.currentTarget.value)}
            onkeydown={(e) => e.key === 'Enter' && actions.setCustomModel(newModelPath)}
            placeholder="Paste path to .bin file…"
            class="tt-input"
            spellcheck="false"
          />
          <button onclick={actions.browseCustomModel} class="tt-btn">Browse</button>
        </div>
      {/if}
      {#if !modelConfigured}
        <div class="tt-row">
          <p class="tt-warn">No model selected — transcription will fail.</p>
        </div>
      {/if}
    </div>
  {:else}
    {#if altModels.length === 0}
      <div class="tt-section">
        <div class="subsection-hd"><span class="subsection-hd-title">Parakeet Models</span></div>
        <div class="tt-row"><p class="tt-desc">Loading…</p></div>
      </div>
    {:else}
      {@const recAltModel = altModels.find(m => m.recommended)}
      {@const altCatalog  = altModels.filter(m => !m.recommended)}
      {#if recAltModel}
        <div class="tt-section">
          <div class="subsection-hd"><span class="subsection-hd-title">Recommended</span></div>
          <div class="tt-row tt-row-field">
            {@render altModelRow(recAltModel, true)}
          </div>
        </div>
      {/if}
      {#if altCatalog.length > 0}
        <div class="tt-section">
          <div class="subsection-hd"><span class="subsection-hd-title">Available</span></div>
          {#each altCatalog as m}
            {@render altModelRow(m, false)}
          {/each}
        </div>
      {/if}
    {/if}

    <div class="tt-section tt-section-last">
      {#if !modelConfigured}
        <div class="tt-row">
          <p class="tt-warn">No model selected — transcription will fail.</p>
        </div>
      {/if}
      <div class="tt-row tt-row-col">
        <p class="tt-desc">Models are stored in <code class="tt-code">~/.config/turbotalk/models/{cfgBackend}/</code>.</p>
      </div>
    </div>
  {/if}
  </div>
</div>
