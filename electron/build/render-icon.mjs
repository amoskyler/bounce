/**
 * Render an SVG to a transparent PNG at a given size, using Electron.
 *
 *   electron build/render-icon.mjs <source.svg> <out.png> <size>
 *
 * Exists because `qlmanage -t` composites its thumbnails onto an opaque
 * background, which turns a transparent mark into a white tile. A
 * `BrowserWindow` with `transparent: true` and a page that sets no background
 * keeps the alpha channel intact.
 */

import { app, BrowserWindow } from 'electron';
import { readFileSync, writeFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

const [source, output, sizeArgument] = process.argv.slice(2);
const size = Number(sizeArgument);

if (!source || !output || !Number.isFinite(size)) {
  console.error('usage: electron render-icon.mjs <source.svg> <out.png> <size>');
  process.exit(1);
}

app.disableHardwareAcceleration();

app.whenReady().then(async () => {
  const window = new BrowserWindow({
    width: size,
    height: size,
    show: false,
    frame: false,
    transparent: true,
    // No background of its own, or the transparency is lost before the SVG is
    // even drawn.
    backgroundColor: '#00000000',
    webPreferences: { offscreen: true, sandbox: true },
  });

  const svg = readFileSync(source, 'utf8');
  const page = `<!doctype html><meta charset="utf-8">
    <style>
      html, body { margin: 0; padding: 0; background: transparent; }
      svg { display: block; width: ${size}px; height: ${size}px; }
    </style>
    ${svg}`;

  await window.loadURL(`data:text/html;charset=utf-8,${encodeURIComponent(page)}`);
  // One frame, so the SVG has certainly been laid out and painted.
  await new Promise((resolve) => setTimeout(resolve, 120));

  const captured = await window.webContents.capturePage({ x: 0, y: 0, width: size, height: size });

  // On a Retina display `capturePage` hands back physical pixels, so a 16pt
  // window yields a 32px image. Resizing to the requested size normalises that
  // — and on such a display the render was supersampled first, which is a
  // better small icon than one drawn straight at 16px.
  const scaled =
    captured.getSize().width === size ? captured : captured.resize({ width: size, height: size, quality: 'best' });

  writeFileSync(output, scaled.toPNG());

  app.quit();
});

// Never leave a headless Electron behind if something goes wrong.
setTimeout(() => process.exit(1), 30_000);

void pathToFileURL;
