<script>
  import { getCurrentWindow } from '@tauri-apps/api/window';

  /**
   * Titlebar — shared window chrome for all Libre apps.
   *
   * Renders the top resize strip, drag region, app content slot, and
   * Windows-style minimize / maximize / close controls.
   *
   * Props:
   *   height  — Tailwind height class for the bar. Default: 'h-8'.
   *             Use 'h-11' for apps with richer titlebar content (e.g. Ghost).
   *
   * Usage:
   *   <Titlebar>
   *     <!-- your icon + title or nav controls here -->
   *   </Titlebar>
   */

  /**
   * onclose — optional custom close handler. If provided, called instead of
   *   appWindow.close(). Use this when the app needs to confirm unsaved state
   *   (e.g. Stack checks for unsaved tabs) before closing the window.
   */
  let { height = 'h-8', onclose, children } = $props();

  const appWindow = getCurrentWindow();
</script>

<!-- Top resize strip — invisible 4px hit-target so dragging the very top edge resizes -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="absolute top-[8px] left-0 right-0 h-[4px] z-50 cursor-n-resize"
  onmousedown={(e) => { e.preventDefault(); appWindow.startResizeDragging('North'); }}
></div>

<!-- Titlebar -->
<div
  data-tauri-drag-region
  class="{height} bg-[var(--titlebar-bg)] border-b border-[var(--border)]
         flex items-center shrink-0 select-none"
>
  <!-- App-specific content (icon, title, nav controls, etc.) -->
  <div class="flex items-center flex-1 min-w-0 h-full" data-tauri-drag-region>
    {@render children?.()}
  </div>

  <!-- Windows-style window controls -->
  <div class="flex items-center shrink-0 h-full">
    <button
      onclick={() => appWindow.minimize()}
      class="w-11 h-full flex items-center justify-center text-gray-500 dark:text-gray-400
             hover:bg-gray-200 dark:hover:bg-gray-700 transition-colors text-[16px] leading-none"
      title="Minimize"
      aria-label="Minimize"
    >─</button>
    <button
      onclick={() => appWindow.toggleMaximize()}
      class="w-11 h-full flex items-center justify-center text-gray-500 dark:text-gray-400
             hover:bg-gray-200 dark:hover:bg-gray-700 transition-colors text-[13px]"
      title="Maximize"
      aria-label="Maximize"
    >□</button>
    <button
      onclick={() => onclose ? onclose() : appWindow.close()}
      class="w-11 h-full flex items-center justify-center text-gray-500 dark:text-gray-400
             hover:bg-red-500 hover:text-white transition-colors text-[18px]"
      title="Close"
      aria-label="Close"
    >×</button>
  </div>
</div>
