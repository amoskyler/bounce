/**
 * Build the platform icon files from `icon.svg`.
 *
 *   node build/make-icons.mjs
 *
 * Writes `icon.icns` (macOS), `icon.ico` (Windows) and `icon.png` (Linux, and
 * the window icon on every platform) beside the source. electron-builder picks
 * them up by name out of `directories.buildResources`.
 *
 * Rendering goes through Electron rather than `qlmanage`. `qlmanage -t` makes
 * a Quick Look *thumbnail*, and a thumbnail is composited onto an opaque
 * background — so every size came out as the logo on a white square, which is
 * exactly what the dock then showed. Nothing in the output hints at it; the
 * file is a valid RGBA PNG whose alpha happens to be 255 everywhere.
 *
 * A `BrowserWindow` with `transparent: true` renders the same SVG through the
 * same WebKit and keeps the alpha, which is what makes the icon read as a mark
 * rather than a tile. `iconutil` packs the .icns; the .ico is assembled here,
 * being a directory of PNGs behind a 22-byte header that Windows has read
 * since Vista.
 *
 * Each size is rendered from the SVG at that size rather than resampled from
 * the largest one, so small icons are drawn small rather than being a shrunken
 * 1024.
 */

import { execFileSync } from 'node:child_process';
import * as zlibSync from 'node:zlib';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));

/** Electron's binary, which is a devDependency of this package. */
function electronBinary() {
  return path.join(here, '..', 'node_modules', '.bin', 'electron');
}

/**
 * Fail loudly if a render came back opaque.
 *
 * Decodes just the first scanline, which is enough: the artwork is inset, so
 * the top-left pixel is background in every size.
 */
function assertTransparentCorner(png, size) {
  let offset = 8;
  let idat = Buffer.alloc(0);

  while (offset < png.length) {
    const length = png.readUInt32BE(offset);
    const type = png.toString('ascii', offset + 4, offset + 8);
    if (type === 'IDAT') idat = Buffer.concat([idat, png.subarray(offset + 8, offset + 8 + length)]);
    if (type === 'IEND') break;
    offset += 12 + length;
  }

  // Filter byte, then RGBA — so the first pixel's alpha is at index 4.
  const raw = zlibSync.inflateSync(idat);
  if (raw[4] !== 0) {
    throw new Error(
      `the ${size}px render has an opaque corner (alpha ${raw[4]}). ` +
        'Something flattened it onto a background; the dock will show a square.',
    );
  }
}
const source = path.join(here, 'icon.svg');

/** The sizes macOS asks for, as (pixels, iconset name) pairs. */
const ICONSET = [
  [16, 'icon_16x16.png'],
  [32, 'icon_16x16@2x.png'],
  [32, 'icon_32x32.png'],
  [64, 'icon_32x32@2x.png'],
  [128, 'icon_128x128.png'],
  [256, 'icon_128x128@2x.png'],
  [256, 'icon_256x256.png'],
  [512, 'icon_256x256@2x.png'],
  [512, 'icon_512x512.png'],
  [1024, 'icon_512x512@2x.png'],
];

/** The sizes Windows shells pick between. */
const ICO_SIZES = [16, 24, 32, 48, 64, 128, 256];

const work = mkdtempSync(path.join(tmpdir(), 'bounce-icons-'));

try {
  const png = new Map();
  for (const size of new Set([...ICONSET.map(([pixels]) => pixels), ...ICO_SIZES])) {
    png.set(size, render(size));
  }

  // macOS: an .iconset directory of the named sizes, packed by iconutil.
  const iconset = path.join(work, 'icon.iconset');
  mkdirSync(iconset);
  for (const [size, name] of ICONSET) {
    writeFileSync(path.join(iconset, name), png.get(size));
  }
  execFileSync('iconutil', ['-c', 'icns', iconset, '-o', path.join(here, 'icon.icns')]);

  writeFileSync(path.join(here, 'icon.ico'), buildIco(ICO_SIZES.map((size) => png.get(size))));
  writeFileSync(path.join(here, 'icon.png'), png.get(1024));

  console.log('wrote icon.icns, icon.ico and icon.png');
} finally {
  rmSync(work, { recursive: true, force: true });
}

/** Rasterise the SVG at one size, checking that it came back square. */
function render(size) {
  const out = path.join(work, String(size));
  mkdirSync(out, { recursive: true });

  execFileSync(
    electronBinary(),
    [path.join(here, 'render-icon.mjs'), source, path.join(out, 'icon.png'), String(size)],
    { stdio: 'ignore', env: { ...process.env, ELECTRON_RUN_AS_NODE: '' } },
  );

  const bytes = readFileSync(path.join(out, 'icon.png'));
  const width = bytes.readUInt32BE(16);
  const height = bytes.readUInt32BE(20);
  if (width !== size || height !== size) {
    throw new Error(`rendered ${size}px as ${width}x${height}`);
  }

  // The whole point of not using qlmanage. A corner pixel that is opaque means
  // the render was flattened onto a background, and the icon will show as a
  // tile in the dock however good the artwork is.
  assertTransparentCorner(bytes, size);
  return bytes;
}

/**
 * Pack PNGs into an .ico.
 *
 * A six-byte header, then a sixteen-byte directory entry per image, then the
 * images themselves. A dimension of 256 is written as 0, which is how the
 * one-byte fields reach it.
 */
function buildIco(images) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0); // reserved
  header.writeUInt16LE(1, 2); // 1 is an icon, 2 would be a cursor
  header.writeUInt16LE(images.length, 4);

  const directory = Buffer.alloc(16 * images.length);
  let offset = header.length + directory.length;

  images.forEach((bytes, index) => {
    const size = bytes.readUInt32BE(16);
    const entry = index * 16;
    directory.writeUInt8(size >= 256 ? 0 : size, entry);
    directory.writeUInt8(size >= 256 ? 0 : size, entry + 1);
    directory.writeUInt8(0, entry + 2); // palette size: none, this is truecolour
    directory.writeUInt8(0, entry + 3); // reserved
    directory.writeUInt16LE(1, entry + 4); // colour planes
    directory.writeUInt16LE(32, entry + 6); // bits per pixel
    directory.writeUInt32LE(bytes.length, entry + 8);
    directory.writeUInt32LE(offset, entry + 12);
    offset += bytes.length;
  });

  return Buffer.concat([header, directory, ...images]);
}
