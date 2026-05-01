/**
 * @libre/ui — Typed Tauri command wrappers
 *
 * A thin typed layer over Tauri's invoke() for commands available in
 * every Libre app. Each app extends this with its own command wrappers
 * rather than calling invoke() with raw string names.
 *
 * Usage:
 *   import { getTheme, getAccent, call } from '@libre/ui/src/api/commands.ts';
 *
 *   // Core commands (available in all apps)
 *   const theme = await getTheme();   // 'dark' | 'light' | 'system'
 *   const accent = await getAccent(); // '#0066cc'
 *
 *   // App-specific commands — use the generic call() wrapper
 *   const files = await call<FileEntry[]>('list_dir', { path: '/home/user' });
 */

import { invoke } from '@tauri-apps/api/core';

/**
 * Generic typed invoke wrapper. Use this as the base for all app-specific
 * command calls to keep command names in one typed file per app.
 *
 * @param cmd  — Tauri command name (snake_case, must match #[tauri::command] fn)
 * @param args — Command arguments object (field names match Rust param names)
 */
export function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(cmd, args);
}

// ── Core commands — available in every Libre app ──────────────────────────────

/** Read the LibreWin theme setting. Returns 'dark' | 'light' | 'system'. */
export function getTheme(): Promise<string> {
  return call<string>('get_theme');
}

/** Read the LibreWin accent colour. Returns a hex string e.g. '#0066cc'. */
export function getAccent(): Promise<string> {
  return call<string>('get_accent');
}
