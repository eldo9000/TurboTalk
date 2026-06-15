import './app.css';
import { mount } from 'svelte';
import { getCurrentWindow } from '@tauri-apps/api/window';

const label = getCurrentWindow().label;

if (label === 'splash' || label === 'overlay' || label === 'cursor-dot' || label === 'status') {
  // app.css sets a solid background on html/body/#app; override it inline so
  // transparent Tauri windows don't flash a white rectangle before Svelte mounts.
  document.documentElement.style.background = 'transparent';
  document.body.style.background = 'transparent';
  const appEl = document.getElementById('app');
  if (appEl) appEl.style.background = 'transparent';
}

const Component = label === 'overlay'
  ? (await import('./Overlay.svelte')).default
  : label === 'splash'
  ? (await import('./Splash.svelte')).default
  : label === 'cursor-dot'
  ? (await import('./CursorDot.svelte')).default
  : label === 'status'
  ? (await import('./Status.svelte')).default
  : (await import('./App.svelte')).default;

mount(Component, { target: document.getElementById('app') });
