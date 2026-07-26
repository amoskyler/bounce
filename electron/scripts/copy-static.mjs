/**
 * Copy the renderer's static shell into the build output.
 *
 * esbuild emits `index.js` and `index.css`; the HTML that loads them is not
 * part of the bundle graph, so it is copied alongside.
 */

import { copyFile, mkdir } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const source = join(here, '..', 'src', 'renderer', 'index.html');
const destination = join(here, '..', 'dist', 'renderer', 'index.html');

await mkdir(dirname(destination), { recursive: true });
await copyFile(source, destination);

console.log('copied index.html -> dist/renderer/index.html');
