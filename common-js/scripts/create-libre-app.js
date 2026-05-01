#!/usr/bin/env node
/**
 * create-libre-app — scaffold a new Libre app inside apps/
 *
 * Usage (run from apps/):
 *   node common-js/scripts/create-libre-app.js <app-name>
 *
 * Example:
 *   node common-js/scripts/create-libre-app.js stack-lite
 *
 * Creates apps/<app-name>/ with:
 *   - Vite + Svelte + Tailwind frontend wired to @libre/ui
 *   - Tauri 2 backend with librewin-common dependency
 *   - get_theme / get_accent commands pre-wired
 *   - WindowFrame + Titlebar pre-wired in App.svelte
 *   - Icons copied from apps/ghost/src-tauri/icons/
 *   - apps/Cargo.toml workspace updated automatically
 *
 * After running: cd <app-name> && npm install && npm run tauri dev
 */

import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'fs';
import { dirname, join, resolve } from 'path';
import { fileURLToPath } from 'url';

// ── Argument parsing ──────────────────────────────────────────────────────────

const appName = process.argv[2];

if (!appName) {
  console.error('Usage: node create-libre-app.js <app-name>');
  console.error('Example: node create-libre-app.js stack-lite');
  process.exit(1);
}

if (!/^[a-z][a-z0-9-]*$/.test(appName)) {
  console.error(`Error: app name must be lowercase letters, digits, and hyphens (got: "${appName}")`);
  process.exit(1);
}

// ── Derived names ─────────────────────────────────────────────────────────────

const rustName    = appName.replace(/-/g, '_');               // my_app
const libName     = `${rustName}_lib`;                        // my_app_lib
const displayName = appName.split('-').map(w => w[0].toUpperCase() + w.slice(1)).join(' '); // My App
const identifier  = `io.librewin.${rustName}`;                // io.librewin.my_app

// ── Paths ─────────────────────────────────────────────────────────────────────

const __dir   = dirname(fileURLToPath(import.meta.url));
const appsDir = resolve(__dir, '../..');                      // apps/
const outDir  = join(appsDir, appName);

if (existsSync(outDir)) {
  console.error(`Error: directory already exists: ${outDir}`);
  process.exit(1);
}

// ── Helpers ───────────────────────────────────────────────────────────────────

function write(relPath, content) {
  const abs = join(outDir, relPath);
  mkdirSync(dirname(abs), { recursive: true });
  writeFileSync(abs, content, 'utf8');
  console.log(`  created  ${relPath}`);
}

function copyIcons() {
  const src = join(appsDir, 'ghost/src-tauri/icons');
  const dst = join(outDir, 'src-tauri/icons');
  if (existsSync(src)) {
    cpSync(src, dst, { recursive: true });
    const count = readdirSync(dst).length;
    console.log(`  copied   src-tauri/icons/ (${count} files from ghost)`);
  } else {
    mkdirSync(dst, { recursive: true });
    console.log(`  warning  src-tauri/icons/ not found — add icons manually`);
  }
}

function addToCargoWorkspace() {
  const cargoPath = join(appsDir, 'Cargo.toml');
  let cargo = readFileSync(cargoPath, 'utf8');
  const memberEntry = `    "${appName}/src-tauri",`;
  if (cargo.includes(memberEntry)) {
    console.log(`  skipped  Cargo.toml (already member)`);
    return;
  }
  // Insert before the closing bracket of members array
  cargo = cargo.replace(
    /(\s+"common",\n)(\])/,
    `$1${memberEntry}\n$2`
  );
  writeFileSync(cargoPath, cargo, 'utf8');
  console.log(`  updated  Cargo.toml (added ${appName}/src-tauri)`);
}

// ── Template files ────────────────────────────────────────────────────────────

console.log(`\nCreating Libre app: ${displayName}`);
console.log(`  output:  ${outDir}\n`);

// package.json
write('package.json', JSON.stringify({
  name: appName,
  private: true,
  version: '0.1.0',
  type: 'module',
  scripts: {
    dev: 'vite',
    build: 'vite build',
    preview: 'vite preview',
    tauri: 'tauri',
    test: 'vitest run',
  },
  dependencies: {
    '@fontsource/geist': '^5.2.8',
    '@libre/ui': '*',
    '@tauri-apps/api': '^2',
  },
  devDependencies: {
    '@sveltejs/vite-plugin-svelte': '^7',
    '@tailwindcss/vite': '^4',
    '@tauri-apps/cli': '^2',
    'svelte': '^5',
    'tailwindcss': '^4',
    'vite': '^8',
    'vitest': '^3.0.0',
    'jsdom': '^26.0.0',
  },
}, null, 2) + '\n');

// index.html
write('index.html', `<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>${displayName}</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.js"></script>
  </body>
</html>
`);

// vite.config.js
write('vite.config.js', `import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  plugins: [tailwindcss(), svelte()],
  clearScreen: false,
  server: {
    port: 1421,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: {
    target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
    minify: process.env.TAURI_ENV_DEBUG ? false : 'oxc',
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});
`);

// svelte.config.js
write('svelte.config.js', `import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

export default {
  preprocess: vitePreprocess(),
};
`);

// src/main.js
write('src/main.js', `import './app.css';
import App from './App.svelte';
import { mount } from 'svelte';

const app = mount(App, {
  target: document.getElementById('app'),
});

export default app;
`);

// src/app.css
write('src/app.css', `@import "tailwindcss";
@import "@fontsource/geist/latin.css";
@import "@libre/ui/src/tokens.css";
@variant dark (&:where(.dark, .dark *));
`);

// src/App.svelte
write('src/App.svelte', `<script>
  import { WindowFrame, Titlebar } from '@libre/ui';
</script>

<WindowFrame>
  <Titlebar>
    <div class="flex items-center gap-2 pl-3" data-tauri-drag-region>
      <span class="text-[13px] font-medium text-[var(--text-primary)]">${displayName}</span>
    </div>
  </Titlebar>

  <main class="flex-1 flex items-center justify-center" role="main">
    <p class="text-[var(--text-secondary)] text-sm">Hello from ${displayName}</p>
  </main>
</WindowFrame>
`);

// src-tauri/Cargo.toml
write('src-tauri/Cargo.toml', `[package]
name = "${rustName}"
version = "0.1.0"
description = "${displayName} — LibreWin app"
authors = ["LibreWin"]
edition = "2021"
rust-version = "1.77"

[lib]
name = "${libName}"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri           = { workspace = true }
serde           = { workspace = true }
serde_json      = { workspace = true }
librewin-common = { workspace = true }
`);

// src-tauri/build.rs
write('src-tauri/build.rs', `fn main() {
    tauri_build::build()
}
`);

// src-tauri/src/main.rs
write('src-tauri/src/main.rs', `// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    ${libName}::run();
}
`);

// src-tauri/src/lib.rs
write('src-tauri/src/lib.rs', `/// Read the LibreWin theme preference.
#[tauri::command]
fn get_theme() -> String { librewin_common::get_theme() }

/// Read the LibreWin accent colour.
#[tauri::command]
fn get_accent() -> String { librewin_common::get_accent() }

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_theme,
            get_accent,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ${appName}");
}
`);

// src-tauri/tauri.conf.json
write('src-tauri/tauri.conf.json', JSON.stringify({
  productName: displayName,
  version: '0.1.0',
  identifier,
  build: {
    frontendDist: '../dist',
    devUrl: 'http://localhost:1421',
    beforeDevCommand: 'npm run dev',
    beforeBuildCommand: 'npm run build',
  },
  app: {
    windows: [{
      label: 'main',
      title: displayName,
      width: 1024,
      height: 768,
      minWidth: 640,
      minHeight: 480,
      decorations: false,
      resizable: true,
      center: true,
      transparent: true,
    }],
    security: { csp: null },
  },
  bundle: {
    active: true,
    targets: 'all',
    linux: {
      deb: { depends: ['xdg-utils'] },
      rpm: { depends: ['xdg-utils'] },
    },
    icon: [
      'icons/32x32.png',
      'icons/128x128.png',
      'icons/128x128@2x.png',
      'icons/icon.png',
    ],
  },
}, null, 2) + '\n');

// src-tauri/capabilities/default.json
write('src-tauri/capabilities/default.json', JSON.stringify({
  $schema: '../gen/schemas/desktop-schema.json',
  identifier: 'default',
  description: `Default capabilities for ${displayName} main window`,
  windows: ['main'],
  permissions: [
    'core:default',
    'core:window:allow-minimize',
    'core:window:allow-toggle-maximize',
    'core:window:allow-close',
  ],
}, null, 2) + '\n');

// ── Post-create steps ─────────────────────────────────────────────────────────

copyIcons();
addToCargoWorkspace();

console.log(`
✓ ${displayName} created at apps/${appName}/

Next steps:
  cd ${appName}
  npm install
  npm run tauri dev

To add Tauri commands, edit src-tauri/src/lib.rs and register them in
the invoke_handler! macro. The get_theme and get_accent commands are
pre-wired — the frontend reads them automatically via WindowFrame.
`);
