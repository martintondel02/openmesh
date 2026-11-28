#!/usr/bin/env node
/**
 * Ensures the bundled AVIF encoder WASM exists before a web dev/build run.
 *
 * - If `apps/web/public/image/openmesh_image.wasm` already exists, do nothing (CI builds it
 *   explicitly via crates/openmesh-image-wasm/build-wasm.sh).
 * - Otherwise try to build it with the shared Rust script when cargo is available.
 * - Any failure is a warning, not a build failure: the web client falls back to canvas
 *   WebP/JPEG with the correct MIME, so a machine without Rust/wasm32 can still build/test
 *   the app.
 *
 * Set OPENMESH_SKIP_IMAGE_WASM=1 to skip the attempt entirely.
 */
import { existsSync, statSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const appRoot = resolve(scriptDir, '..');
const repoRoot = resolve(appRoot, '..', '..');
const output = resolve(appRoot, 'public', 'image', 'openmesh_image.wasm');
const buildScript = resolve(repoRoot, 'crates', 'openmesh-image-wasm', 'build-wasm.sh');

function describe() {
  try {
    const bytes = statSync(output).size;
    return `${(bytes / 1024 / 1024).toFixed(2)} MiB`;
  } catch {
    return 'unknown size';
  }
}

if (process.env.OPENMESH_SKIP_IMAGE_WASM === '1') {
  console.log('[image-wasm] skipped (OPENMESH_SKIP_IMAGE_WASM=1); the web client will fall back to WebP/JPEG.');
  process.exit(0);
}

if (existsSync(output)) {
  console.log(`[image-wasm] using existing ${output} (${describe()})`);
  process.exit(0);
}

const has = (command) => {
  const result = spawnSync(command, ['--version'], { stdio: 'ignore' });
  return result.status === 0;
};

if (!has('bash') || !has('cargo') || !existsSync(buildScript)) {
  console.warn(
    '[image-wasm] cargo/bash or the build script is unavailable; building the web app without the bundled AVIF encoder (WebP/JPEG fallback).',
  );
  process.exit(0);
}

console.log('[image-wasm] building the bundled AVIF encoder (first run can take a few minutes)…');
const result = spawnSync('bash', [buildScript], { cwd: repoRoot, stdio: 'inherit' });
if (result.status !== 0 || !existsSync(output)) {
  console.warn(
    '[image-wasm] build failed or produced no artifact; continuing without AVIF (WebP/JPEG fallback). Run `pnpm build:image-wasm` after fixing the Rust/wasm toolchain.',
  );
  process.exit(0);
}
console.log(`[image-wasm] ready: ${output} (${describe()})`);
