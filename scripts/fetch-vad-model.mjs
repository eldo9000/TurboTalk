#!/usr/bin/env node
// Download the whisper.cpp Silero VAD model used by whisper-server's --vad flag.
// Wired via package.json: `npm run fetch-vad-model`.
//
// The model is ~864 KB and is bundled into macOS release packages via
// tauri.macos.conf.json → bundle.resources.

import { createHash } from 'node:crypto';
import { createWriteStream, statSync } from 'node:fs';
import { mkdir, unlink } from 'node:fs/promises';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { pipeline } from 'node:stream/promises';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const dest = resolve(repoRoot, 'src-tauri/binaries/ggml-silero-v5.1.2.bin');

const URL =
  'https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin';

// Pin the upstream artifact so CI/dev refreshes are reproducible.
const EXPECTED_SHA256 =
  '29940d98d42b91fbd05ce489f3ecf7c72f0a42f027e4875919a28fb4c04ea2cf';

async function sha256File(path) {
  const { createReadStream } = await import('node:fs');
  return new Promise((resolveHash, reject) => {
    const hash = createHash('sha256');
    createReadStream(path)
      .on('data', (chunk) => hash.update(chunk))
      .on('error', reject)
      .on('end', () => resolveHash(hash.digest('hex')));
  });
}

async function fetchWithRetry(url, retries = 3) {
  const backoff = [2000, 8000, 30000];
  for (let attempt = 0; attempt <= retries; attempt++) {
    try {
      const res = await fetch(url, { signal: AbortSignal.timeout(30000) });
      if (res.ok) return res;
      if (res.status === 429 && attempt < retries) {
        console.warn(`[fetch-vad-model] HTTP 429 — retrying in ${backoff[attempt] / 1000}s (attempt ${attempt + 1})`);
        await new Promise(r => setTimeout(r, backoff[attempt]));
        continue;
      }
      throw new Error(`HTTP ${res.status}`);
    } catch (err) {
      if (attempt < retries && (err.cause?.code === 'UND_ERR_CONNECT_TIMEOUT' || err.message?.includes('fetch failed'))) {
        console.warn(`[fetch-vad-model] connect timeout — retrying in ${backoff[attempt] / 1000}s (attempt ${attempt + 1})`);
        await new Promise(r => setTimeout(r, backoff[attempt]));
        continue;
      }
      throw err;
    }
  }
}

async function main() {
  await mkdir(dirname(dest), { recursive: true });

  try {
    const st = statSync(dest);
    if (st.isFile() && st.size > 10_000) {
      const digest = await sha256File(dest);
      if (digest === EXPECTED_SHA256) {
        console.log(`[fetch-vad-model] already present and verified — ${dest}`);
        return;
      }
      console.log('[fetch-vad-model] existing file failed sha256 check — re-downloading');
    }
  } catch {
    // missing — download below
  }

  console.log(`[fetch-vad-model] downloading ${URL}`);
  const res = await fetchWithRetry(URL);
  if (!res.ok) {
    console.error(`[fetch-vad-model] HTTP ${res.status} fetching ${URL}`);
    process.exit(1);
  }

  const tmp = `${dest}.tmp`;
  await pipeline(res.body, createWriteStream(tmp));

  const size = statSync(tmp).size;
  if (size < 10_000) {
    console.error(`[fetch-vad-model] downloaded file too small (${size} bytes)`);
    await unlink(tmp);
    process.exit(1);
  }

  await unlink(dest).catch(() => {});
  const { rename } = await import('node:fs/promises');
  await rename(tmp, dest);

  const digest = await sha256File(dest);
  if (digest !== EXPECTED_SHA256) {
    console.error(`[fetch-vad-model] sha256 mismatch (got ${digest})`);
    await unlink(dest);
    process.exit(1);
  }
  console.log(`[fetch-vad-model] sha256 verified`);
  console.log(`[fetch-vad-model] done — ${size} bytes → ${dest}`);
}

main().catch((err) => {
  console.error('[fetch-vad-model]', err);
  process.exit(1);
});
