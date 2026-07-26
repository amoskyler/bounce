/**
 * Build and package the whole application: Rust core, then Electron client.
 *
 * ```bash
 * npm run package                 # installer for this machine
 * npm run package -- --dir        # unpacked app only, much faster
 * npm run package -- --arch x64   # cross-architecture, same OS
 * npm run package -- --universal  # macOS arm64 + x86_64 in one bundle
 * ```
 *
 * ## Why there is no "build for every platform" option
 *
 * The engine is a compiled native module, so a package is only valid for the
 * platform and architecture it was compiled for. `electron-builder --mac --win
 * --linux` would happily wrap one machine's binary in three installers, and two
 * of them would fail to load the engine at launch — with no error until a user
 * ran them.
 *
 * So this script builds the Rust for the target it is packaging, and refuses to
 * package a target it cannot compile for. Producing all three means running it
 * on all three, or wiring up cross-linkers in CI.
 */

import { spawn } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readdirSync, statSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const projectRoot = join(here, '..');
const rustRoot = join(projectRoot, '..', 'rust');

const args = process.argv.slice(2);
const flags = new Set(args.filter((arg) => arg.startsWith('--')));

function flagValue(name, fallback) {
  const index = args.indexOf(`--${name}`);
  if (index !== -1 && args[index + 1] && !args[index + 1].startsWith('--')) {
    return args[index + 1];
  }
  return fallback;
}

const unpackedOnly = flags.has('--dir');
const universal = flags.has('--universal');
const targetArch = flagValue('arch', process.arch);

/** Rust target triples, by Node's platform and arch names. */
const RUST_TRIPLES = {
  'darwin:arm64': 'aarch64-apple-darwin',
  'darwin:x64': 'x86_64-apple-darwin',
  'win32:x64': 'x86_64-pc-windows-msvc',
  'win32:arm64': 'aarch64-pc-windows-msvc',
  'linux:x64': 'x86_64-unknown-linux-gnu',
  'linux:arm64': 'aarch64-unknown-linux-gnu',
};

/** What cargo names the shared library, by platform. */
const LIBRARY_NAMES = {
  darwin: 'libbounce_node.dylib',
  linux: 'libbounce_node.so',
  win32: 'bounce_node.dll',
};

const platform = process.platform;
const libraryName = LIBRARY_NAMES[platform];

if (!libraryName) {
  fail(`Unsupported platform: ${platform}`);
}

/**
 * Locate a Rust tool.
 *
 * rustup installs into `~/.cargo/bin` and puts it on PATH through the shell
 * profile, which `npm run` and most CI runners never source. Falling back to
 * the known location avoids a confusing "install Rust" error on a machine that
 * already has it.
 */
function rustTool(name) {
  const executable = process.platform === 'win32' ? `${name}.exe` : name;
  const cargoHome = process.env.CARGO_HOME ?? join(homedir(), '.cargo');
  const candidate = join(cargoHome, 'bin', executable);
  return existsSync(candidate) ? candidate : name;
}

const CARGO = rustTool('cargo');
const RUSTUP = rustTool('rustup');

function fail(message) {
  console.error(`\n✗ ${message}\n`);
  process.exit(1);
}

function step(message) {
  console.log(`\n▸ ${message}`);
}

/** Run a command, inheriting stdio, failing the script on a non-zero exit. */
function run(command, commandArgs, options = {}) {
  return new Promise((resolve) => {
    const child = spawn(command, commandArgs, {
      cwd: options.cwd ?? projectRoot,
      stdio: 'inherit',
      shell: process.platform === 'win32',
      env: { ...process.env, ...options.env },
    });
    child.on('error', (error) => fail(`${command} could not be run: ${error.message}`));
    child.on('exit', (code) => {
      if (code !== 0) {
        fail(`${command} ${commandArgs.join(' ')} exited with ${code}`);
      }
      resolve();
    });
  });
}

/** Capture a command's output, for probing the toolchain. */
function capture(command, commandArgs) {
  return new Promise((resolve) => {
    const child = spawn(command, commandArgs, {
      cwd: rustRoot,
      shell: process.platform === 'win32',
    });
    let output = '';
    child.stdout?.on('data', (chunk) => (output += chunk));
    child.stderr?.on('data', (chunk) => (output += chunk));
    child.on('error', () => resolve(null));
    child.on('exit', (code) => resolve(code === 0 ? output : null));
  });
}

// -------------------------------------------------------------------------

const targets = universal
  ? ['arm64', 'x64']
  : [targetArch];

if (universal && platform !== 'darwin') {
  fail('--universal only applies to macOS.');
}

step('Checking the toolchain');

const cargoVersion = await capture(CARGO, ['--version']);
if (!cargoVersion) {
  fail('cargo is not on PATH. Install Rust from https://rustup.rs and retry.');
}
console.log(`  ${cargoVersion.trim()}`);
console.log(`  node ${process.version}`);
console.log(`  target ${platform}/${targets.join(' + ')}`);

// Confirm every Rust target is installed before spending time on a build that
// would fail late.
const installed = (await capture(RUSTUP, ['target', 'list', '--installed'])) ?? '';
const hostTriple = RUST_TRIPLES[`${platform}:${process.arch}`];

for (const arch of targets) {
  const triple = RUST_TRIPLES[`${platform}:${arch}`];
  if (!triple) {
    fail(`No Rust target known for ${platform}/${arch}.`);
  }
  if (triple !== hostTriple && !installed.includes(triple)) {
    fail(
      `Rust target ${triple} is not installed. Add it with:\n` +
        `    rustup target add ${triple}\n` +
        '  Cross-compiling also needs a linker for that target.',
    );
  }
}

// -------------------------------------------------------------------------

step('Building the Rust core');

const builtLibraries = [];

for (const arch of targets) {
  const triple = RUST_TRIPLES[`${platform}:${arch}`];
  const crossCompiling = triple !== hostTriple;

  console.log(`  ${triple}${crossCompiling ? ' (cross)' : ''}`);

  await run(
    CARGO,
    [
      'build',
      '--release',
      '-p',
      'bounce-node',
      ...(crossCompiling ? ['--target', triple] : []),
    ],
    { cwd: rustRoot },
  );

  const output = crossCompiling
    ? join(rustRoot, 'target', triple, 'release', libraryName)
    : join(rustRoot, 'target', 'release', libraryName);

  if (!existsSync(output)) {
    fail(`cargo reported success but ${output} is missing.`);
  }
  builtLibraries.push({ arch, path: output });
}

// -------------------------------------------------------------------------

step('Staging the native module');

const nativeDir = join(projectRoot, 'native');
mkdirSync(nativeDir, { recursive: true });
const staged = join(nativeDir, 'bounce.node');

if (builtLibraries.length === 1) {
  copyFileSync(builtLibraries[0].path, staged);
} else {
  // Fuse the two macOS slices into one binary, so a single bundle runs on both
  // Apple silicon and Intel.
  await run('lipo', [
    '-create',
    ...builtLibraries.map((library) => library.path),
    '-output',
    staged,
  ]);
}

const stagedSize = statSync(staged).size;
console.log(`  native/bounce.node — ${(stagedSize / 1024 / 1024).toFixed(1)} MB`);

// -------------------------------------------------------------------------

step('Type checking');
await run('npx', ['tsc', '--noEmit']);

step('Bundling the client');
await run('npm', ['run', 'build:main']);
await run('npm', ['run', 'build:renderer']);

// -------------------------------------------------------------------------

step('Packaging');

const builderArgs = [`--${platform === 'win32' ? 'win' : platform === 'darwin' ? 'mac' : 'linux'}`];

if (unpackedOnly) {
  builderArgs.push('--dir');
}
if (universal) {
  builderArgs.push('--universal');
} else if (targetArch !== process.arch) {
  builderArgs.push(`--${targetArch}`);
}

await run('npx', ['electron-builder', ...builderArgs]);

// -------------------------------------------------------------------------

step('Done');

const releaseDir = join(projectRoot, 'release');
if (existsSync(releaseDir)) {
  for (const entry of readdirSync(releaseDir)) {
    const full = join(releaseDir, entry);
    const info = statSync(full);
    const size = info.isDirectory() ? '' : ` — ${(info.size / 1024 / 1024).toFixed(1)} MB`;
    console.log(`  ${relative(projectRoot, full)}${size}`);
  }
}

console.log(
  '\n  This package runs on ' +
    `${platform}/${targets.join(' + ')} only — the engine is compiled, not portable.\n`,
);
