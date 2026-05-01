import './app.css';
import { mount } from 'svelte';
import { getCurrentWindow } from '@tauri-apps/api/window';

const label = getCurrentWindow().label;
const Component = label === 'overlay'
  ? (await import('./Overlay.svelte')).default
  : (await import('./App.svelte')).default;

mount(Component, { target: document.getElementById('app') });
