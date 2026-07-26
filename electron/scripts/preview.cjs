/**
 * Render the interface with fixture data and write a screenshot.
 *
 * Run with Electron as the entry point:
 *
 * ```text
 * npx electron scripts/preview.cjs out.png
 * BOUNCE_PREVIEW_THEME=dark npx electron scripts/preview.cjs dark.png
 * ```
 *
 * Capturing through `webContents.capturePage` rather than the window server
 * means this works without screen recording permission, and works headlessly.
 */

const { app, BrowserWindow } = require('electron');
const { writeFileSync } = require('node:fs');
const { join, resolve } = require('node:path');

const output = resolve(process.argv[2] || 'preview.png');
const theme = process.env.BOUNCE_PREVIEW_THEME || 'light';
// Which conversation row to open, by index in the rendered list.
const rowIndex = Number(process.env.BOUNCE_PREVIEW_ROW || '0');
// The renderer is sandboxed, so flags are templated into the injected script
// rather than read from `process` inside the page.
const showDetails = process.env.BOUNCE_PREVIEW_DETAILS === '1';

/**
 * A CSS selector to click once the conversation is open, for capturing
 * something that is only on screen while a popover is up.
 *
 *     BOUNCE_PREVIEW_CLICK='[aria-label="Emoji"]' npx electron scripts/preview.cjs out.png
 */
const clickSelector = process.env.BOUNCE_PREVIEW_CLICK || '';

/** Text to type into the composer before capturing, e.g. to open a typeahead. */
const typeText = process.env.BOUNCE_PREVIEW_TYPE || '';

/** Never leave a stuck Electron process behind. */
const failsafe = setTimeout(() => {
  console.error('preview timed out');
  process.exit(1);
}, 30_000);

app.whenReady().then(async () => {
  const window = new BrowserWindow({
    width: 1100,
    height: 760,
    show: false,
    backgroundColor: theme === 'dark' ? '#1b1b1b' : '#ffffff',
    webPreferences: {
      preload: join(__dirname, 'preview-preload.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
    },
  });

  window.webContents.on('console-message', (_event, _level, message) => {
    console.log('[renderer]', message);
  });

  await window.loadFile(join(__dirname, '..', 'dist', 'renderer', 'index.html'));

  // Pin the theme so the capture does not depend on the host's appearance.
  await window.webContents.executeJavaScript(
    `document.documentElement.dataset.theme = ${JSON.stringify(theme)};`,
  );

  // The conversation list renders once the fixture state resolves; poll briefly
  // for a row, then open it so the timeline is populated.
  const opened = await window.webContents.executeJavaScript(`
    (async () => {
      const deadline = Date.now() + 5000;
      while (Date.now() < deadline) {
        const rows = document.querySelectorAll('.conversation-row');
        if (rows.length > 0) {
          const row = rows[${rowIndex}] || rows[0];
          row.click();
          await new Promise((r) => setTimeout(r, 500));
          if (${showDetails}) {
            const info = document.querySelector('.conversation__identity');
            if (info) info.click();
            await new Promise((r) => setTimeout(r, 400));
          }
          return rows.length;
        }
        await new Promise((r) => setTimeout(r, 50));
      }
      return 0;
    })()
  `);

  console.log(`rendered ${opened} conversation rows`);

  // Typing goes through the native setter so React's onChange actually fires;
  // assigning `.value` on a controlled input is swallowed.
  if (typeText) {
    await window.webContents.executeJavaScript(`
      (async () => {
        const input = document.querySelector('.composer__input');
        if (!input) return 'no composer';
        input.focus();
        const setter = Object.getOwnPropertyDescriptor(
          window.HTMLTextAreaElement.prototype, 'value').set;
        setter.call(input, ${JSON.stringify(typeText)});
        input.dispatchEvent(new Event('input', { bubbles: true }));
        await new Promise((r) => setTimeout(r, 300));
        return 'typed';
      })()
    `);
  }

  if (clickSelector) {
    const clicked = await window.webContents.executeJavaScript(`
      (async () => {
        const target = document.querySelector(${JSON.stringify(clickSelector)});
        if (!target) return 'not found';
        target.click();
        await new Promise((r) => setTimeout(r, 400));
        return 'clicked';
      })()
    `);
    console.log(`${clickSelector}: ${clicked}`);
  }

  /*
   * Put a real file into the composer's picker, the way choosing one does.
   *
   * `DOM.setFileInputFiles` is the DevTools protocol call that browser
   * automation uses for uploads. It matters that it is this and not a
   * synthesised `File`: only a file the browser opened itself has a path on
   * disk, and the path is the whole point of the streaming attachment flow.
   */
  if (process.env.BOUNCE_PREVIEW_PICK_FILE) {
    const debug = window.webContents.debugger;
    debug.attach('1.3');

    const { root } = await debug.sendCommand('DOM.getDocument');
    const { nodeId } = await debug.sendCommand('DOM.querySelector', {
      nodeId: root.nodeId,
      selector: '.composer-area input[type=file]',
    });

    if (!nodeId) {
      console.log('pick: no file input found');
    } else {
      await debug.sendCommand('DOM.setFileInputFiles', {
        nodeId,
        files: [process.env.BOUNCE_PREVIEW_PICK_FILE],
      });
      console.log(`pick: set ${process.env.BOUNCE_PREVIEW_PICK_FILE}`);
      await new Promise((r) => setTimeout(r, 800));
    }

    debug.detach();
  }

  /*
   * A real right-click, delivered as an input event rather than a synthetic
   * DOM one.
   *
   * The difference matters: the browser sends `mousedown`, then `contextmenu`,
   * then `mouseup`, and React's own dispatch happens in the middle of that
   * sequence. A `dispatchEvent` of `contextmenu` alone skips the other two and
   * can pass while the real thing fails.
   *
   *     BOUNCE_PREVIEW_RIGHT_CLICK='760,620'
   */
  if (process.env.BOUNCE_PREVIEW_RIGHT_CLICK) {
    const [x, y] = process.env.BOUNCE_PREVIEW_RIGHT_CLICK.split(',').map(Number);
    const at = { x, y, button: 'right', clickCount: 1 };

    window.webContents.sendInputEvent({ type: 'mouseDown', ...at });
    window.webContents.sendInputEvent({ type: 'mouseUp', ...at });
    await new Promise((r) => setTimeout(r, 400));
    console.log(`right-clicked at ${x},${y}`);
  }

  /*
   * A real left click, as a full mousedown/mouseup pair.
   *
   *     BOUNCE_PREVIEW_CLICK_AT='910,600'
   *
   * Distinct from BOUNCE_PREVIEW_CLICK, which calls `.click()` on a selector
   * and so dispatches `click` with no `mousedown` in front of it — enough to
   * miss a handler that fires on the press rather than the release.
   */
  if (process.env.BOUNCE_PREVIEW_CLICK_AT) {
    const [x, y] = process.env.BOUNCE_PREVIEW_CLICK_AT.split(',').map(Number);
    const at = { x, y, button: 'left', clickCount: 1 };

    window.webContents.sendInputEvent({ type: 'mouseDown', ...at });
    window.webContents.sendInputEvent({ type: 'mouseUp', ...at });
    await new Promise((r) => setTimeout(r, 600));
    console.log(`clicked at ${x},${y}`);
  }

  /*
   * An expression evaluated in the page and logged, for the times a
   * screenshot shows that something is wrong but not by how much:
   *
   *     BOUNCE_PREVIEW_PROBE='document.querySelector(".left-pane").clientWidth'
   */
  if (process.env.BOUNCE_PREVIEW_PROBE) {
    const probed = await window.webContents.executeJavaScript(
      // Awaited, so a probe that has to drive the interface — click, wait for
      // a re-render, read the result — can be written as an async expression.
      `(async () => { try { return String(await (${process.env.BOUNCE_PREVIEW_PROBE})); }
                      catch (error) { return 'probe failed: ' + error.message; } })()`,
    );
    console.log(`probe: ${probed}`);
  }

  const image = await window.webContents.capturePage();
  writeFileSync(output, image.toPNG());
  console.log(`wrote ${output}`);

  clearTimeout(failsafe);
  app.quit();
});
