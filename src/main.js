import './app.css';
import { mount } from 'svelte';
import { getCurrentWindow } from '@tauri-apps/api/window';

const label = getCurrentWindow().label;

if (label === 'overlay') {
  document.documentElement.style.background = 'transparent';
  document.body.style.background = 'transparent';
  const appEl = document.getElementById('app');
  if (appEl) appEl.style.background = 'transparent';
}

const Component = label === 'overlay'
  ? (await import('./Overlay.svelte')).default
  : (await import('./App.svelte')).default;

mount(Component, { target: document.getElementById('app') });
