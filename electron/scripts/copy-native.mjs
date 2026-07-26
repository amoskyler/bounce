/**
 * Stage the compiled Rust core for Node's loader.
 *
 * Cargo emits a platform-named shared library (`libbounce_node.dylib`,
 * `libbounce_node.so`, `bounce_node.dll`). Node only treats a file as a native
 * addon if it ends in `.node`, so the artifact is copied under that name rather
 * than loaded in place.
 *
 * ## Why it unlinks and re-signs
 *
 * On macOS the kernel caches a mach-o's code signature by inode. Overwriting
 * the file in place leaves the cache pointing at bytes that no longer match,
 * and the next process to load it is killed outright — SIGKILL, no exception,
 * no message, exit code 137. It looks exactly like a crash in whatever you
 * changed last, which is a memorable afternoon.
 *
 * Removing the file first gives the copy a fresh inode, and an ad-hoc re-sign
 * makes the signature match the bytes that are actually there.
 */

import { execFileSync } from 'node:child_process';
import { access, copyFile, mkdir, rm } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const targetDirectory = join(here, '..', '..', 'rust', 'target');

const libraryName =
  process.platform === 'win32'
    ? 'bounce_node.dll'
    : process.platform === 'darwin'
      ? 'libbounce_node.dylib'
      : 'libbounce_node.so';

async function exists(path) {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

// Prefer a release build, fall back to debug so `cargo build` alone is enough
// during development.
const candidates = [
  join(targetDirectory, 'release', libraryName),
  join(targetDirectory, 'debug', libraryName),
];

let source = null;
for (const candidate of candidates) {
  if (await exists(candidate)) {
    source = candidate;
    break;
  }
}

if (!source) {
  console.error(
    `Could not find ${libraryName}. Build it first:\n` +
      '  cd rust && cargo build --release -p bounce-node',
  );
  process.exit(1);
}

const destination = join(here, '..', 'native', 'bounce.node');
await mkdir(dirname(destination), { recursive: true });

// Unlink rather than overwrite; see the note at the top of this file.
await rm(destination, { force: true });
await copyFile(source, destination);

if (process.platform === 'darwin') {
  try {
    execFileSync('codesign', ['--force', '--sign', '-', destination], { stdio: 'pipe' });
  } catch (error) {
    console.warn(
      'could not ad-hoc sign the native module; if Node exits with 137 on ' +
        `load, that is why: ${error.message}`,
    );
  }
}

console.log(`copied ${source} -> native/bounce.node`);
