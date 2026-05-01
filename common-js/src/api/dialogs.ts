/**
 * @libre/ui — File dialog wrappers
 *
 * Typed wrappers around Tauri's invoke-based file dialog commands.
 * All apps that need file open/save dialogs should import from here
 * instead of calling invoke() directly — keeps the command names in
 * one place and provides TypeScript types for the results.
 *
 * Requirements: the Tauri backend must expose these commands:
 *   open_file_dialog()          → string | null
 *   save_file_dialog(default_name: string) → string | null
 *
 * Usage:
 *   import { openFileDialog, saveFileDialog } from '@libre/ui/src/api/dialogs.ts';
 *   const path = await openFileDialog();
 *   const dest = await saveFileDialog('untitled.md');
 */

import { invoke } from '@tauri-apps/api/core';

export interface OpenFileOptions {
  /** Hint for the dialog title. Not all backends respect this. */
  title?: string;
}

export interface SaveFileOptions {
  /** Default filename shown in the save dialog. */
  defaultName?: string;
}

/**
 * Open a single-file picker dialog.
 * Returns the selected file path, or null if the user cancelled.
 */
export async function openFileDialog(_options?: OpenFileOptions): Promise<string | null> {
  return invoke<string | null>('open_file_dialog');
}

/**
 * Open a save-file dialog.
 * Returns the chosen destination path, or null if the user cancelled.
 */
export async function saveFileDialog(options?: SaveFileOptions): Promise<string | null> {
  return invoke<string | null>('save_file_dialog', {
    defaultName: options?.defaultName ?? '',
  });
}
