#!/usr/bin/env node
/**
 * Copies content/help/ into apps/web/public/help/ so demo/offline mode can serve
 * the bundled help corpus. Runs automatically before dev/build. The destination is
 * gitignored; content/help is the single source of truth.
 */
import { cp, mkdir, readdir, rm, stat } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const appRoot = resolve(scriptDir, '..');
const repoRoot = resolve(appRoot, '..', '..');
const sourceDir = resolve(repoRoot, 'content', 'help');
const targetDir = resolve(appRoot, 'public', 'help');

async function exists(path) {
  try {
    await stat(path);
    return true;
  } catch {
    return false;
  }
}

if (!(await exists(sourceDir))) {
  console.warn(`[copy-help] content/help not found at ${sourceDir}; the Help Center will use the embedded fallback.`);
  process.exit(0);
}

await rm(targetDir, { recursive: true, force: true });
await mkdir(targetDir, { recursive: true });
await cp(sourceDir, targetDir, { recursive: true });

const files = (await readdir(targetDir)).filter((name) => name.endsWith('.md') || name.endsWith('.json'));
console.log(`[copy-help] bundled ${files.length} help files into apps/web/public/help`);
