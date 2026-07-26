/**
 * Launch a client with a clean profile, in one command.
 *
 * Bounce keeps a device key and a database per profile, and a device's identity
 * *is* that key — so testing a first-run flow, or pairing the same two clients
 * twice, means starting from an empty directory rather than reusing one.
 *
 * ```bash
 * npm run fresh                  # profile "a", no Tor
 * npm run fresh -- b             # a second client, so the two can talk
 * npm run fresh -- b --tor       # over Tor instead
 * npm run fresh -- a --keep      # relaunch without wiping
 * ```
 *
 * Each profile lives under the system temp directory, so nothing here touches
 * the real application data.
 */

import { spawn } from 'node:child_process';
import { existsSync, rmSync, unlinkSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const here = dirname(fileURLToPath(import.meta.url));
const projectRoot = join(here, '..');

const args = process.argv.slice(2);
const flags = new Set(args.filter((arg) => arg.startsWith('--')));
const name = args.find((arg) => !arg.startsWith('--')) ?? 'a';

if (!/^[a-z0-9_-]+$/i.test(name)) {
  console.error(`Invalid profile name ${JSON.stringify(name)} — use letters, digits, - or _.`);
  process.exit(1);
}

const useTor = flags.has('--tor');
const goCompatible = flags.has('--go-compat');
const keep = flags.has('--keep');

const dataDir = join(tmpdir(), 'bounce-dev', name);
const rendezvous = join(tmpdir(), 'bounce-dev-peers.json');

if (!keep && existsSync(dataDir)) {
  rmSync(dataDir, { recursive: true, force: true });
  console.log(`wiped   ${dataDir}`);
}

// Stale entries here are harmless — a dial to a dead port just fails — but
// clearing them keeps the file honest when starting over completely.
if (flags.has('--reset-peers') && existsSync(rendezvous)) {
  unlinkSync(rendezvous);
  console.log(`wiped   ${rendezvous}`);
}

console.log(`profile ${name} at ${dataDir}`);
console.log(`transport ${useTor ? 'tor' : 'tcp (no metadata protection)'}`);
if (goCompatible) {
  console.log('go-compat on: outbound handshakes use the Go format');
}

/** Run a command to completion, inheriting stdio. */
function run(command, commandArgs, env) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, commandArgs, {
      cwd: projectRoot,
      stdio: 'inherit',
      shell: process.platform === 'win32',
      env: { ...process.env, ...env },
    });
    child.on('error', reject);
    child.on('exit', (code) => resolve(code ?? 0));
  });
}

// Bundle first, so a source edit is picked up without a separate step. This
// does not run cargo: change Rust and you still need
// `cd ../rust && cargo build --release -p bounce-node`.
const built = await run('npm', ['run', 'build']);
if (built !== 0) {
  process.exit(built);
}

const code = await run('npx', ['electron', '.'], {
  BOUNCE_DATA_DIR: dataDir,
  BOUNCE_NO_TOR: useTor ? '0' : '1',
  BOUNCE_GO_COMPAT: goCompatible ? '1' : '0',
});

process.exit(code);
