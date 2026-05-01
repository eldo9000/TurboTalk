/**
 * initTheme — shared theme + accent initialisation for all Freedom Apps.
 *
 * Reads the LibreWin theme and accent from the Tauri backend, applies
 * them to the document, and wires up a live media-query listener so the
 * app tracks OS colour-scheme changes when the theme is set to "system".
 *
 * @param {Function} invoke — the Tauri `invoke` function from '@tauri-apps/api/core'
 * @returns {Function} cleanup — call on component destroy to remove the listener
 *
 * Usage in App.svelte:
 *   import { initTheme } from '@libre/ui/src/theme.js';
 *   onMount(() => initTheme(invoke));
 *
 * For apps with additional onMount logic:
 *   onMount(async () => {
 *     const themeCleanup = await initTheme(invoke);
 *     // ... extra setup ...
 *     return () => { themeCleanup?.(); // extra cleanup
 *   });
 */
export async function initTheme(invoke) {
  const theme = await invoke('get_theme');
  const mq = window.matchMedia('(prefers-color-scheme: dark)');
  document.documentElement.classList.toggle('dark',
    theme === 'dark' || (theme === 'system' && mq.matches));
  const accent = await invoke('get_accent');
  document.documentElement.style.setProperty('--accent', accent);

  // Live update: fires when KDE applies a new colour scheme (portal → WebKit).
  const handler = async (e) => {
    const t = await invoke('get_theme');
    if (t === 'system') document.documentElement.classList.toggle('dark', e.matches);
  };
  mq.addEventListener('change', handler);
  return () => mq.removeEventListener('change', handler);
}
