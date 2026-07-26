/**
 * The composer's layout, the emoji picker, and the pane-width preference.
 *
 * The layout assertions look fussy, but the order of the controls is the whole
 * request: an emoji button outside the box on the left, the box, then the
 * attach button. Nothing else in the app would notice if a refactor put them
 * back inside the pill.
 *
 * Rendered with `react-dom/server`, so effects never run — only the first paint
 * is under test, which is where structure is observable.
 *
 * Bundle and run:
 *
 *   npx esbuild src/renderer/__tests__/composer.test.ts --bundle \
 *     --platform=node --format=cjs --loader:.css=empty \
 *     --outfile=/tmp/composer.test.cjs && node --test /tmp/composer.test.cjs
 */

import assert from 'node:assert/strict';
import test from 'node:test';

import * as React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

import { Composer } from '../Conversation';
import { EMBEDDED_FILE_LIMIT, stageFile } from '../Attachments';
import { EmojiPicker, EmojiSuggestions } from '../EmojiPicker';
import { emojiForShortcode } from '../emoji';
import {
  clampLeftPaneWidth,
  DEFAULT_LEFT_PANE_WIDTH,
  loadLeftPaneWidth,
  MAX_LEFT_PANE_WIDTH,
  MIN_LEFT_PANE_WIDTH,
  noteRecentEmoji,
  loadRecentEmoji,
  MAX_RECENT_EMOJI,
  saveLeftPaneWidth,
} from '../preferences';

function composer(draft = '') {
  return renderToStaticMarkup(
    React.createElement(Composer, {
      draft,
      onSend: () => {},
      onChange: () => {},
      onError: () => {},
    }),
  );
}

/* --------------------------------------------------------------------------
 * Layout
 * -------------------------------------------------------------------------- */

test('the emoji button is outside the input box, on its left', () => {
  const html = composer();

  const emoji = html.indexOf('aria-label="Emoji"');
  const wrapper = html.indexOf('composer__input-wrapper');
  const attach = html.indexOf('aria-label="Attach"');

  assert.ok(emoji > -1, 'no emoji button');
  assert.ok(attach > -1, 'no attach button');

  assert.ok(emoji < wrapper, 'the emoji button should come before the input');
  assert.ok(wrapper < attach, 'the attach button should come after the input');
});

test('there is no send button; Enter is the only way to send', () => {
  const html = composer('something to say');
  assert.ok(!html.includes('composer__send'), 'the send button is back');
  assert.ok(!html.includes('aria-label="Send"'), 'the send button is back');
});

test('the emoji button is not inside the pill', () => {
  const html = composer();

  // Everything from the wrapper to the textarea. The emoji button used to live
  // in here, which is what put a second control inside the rounded box.
  const wrapper = html.indexOf('composer__input-wrapper');
  const textarea = html.indexOf('<textarea');
  assert.ok(!html.slice(wrapper, textarea).includes('aria-label="Emoji"'));
});

test('the composer opens with the draft it was given', () => {
  assert.ok(composer('half a thought').includes('half a thought'));
});

test('the attach menu is closed until the button is pressed', () => {
  const html = composer();
  assert.ok(!html.includes('Photos &amp; Videos'));
  assert.ok(html.includes('aria-expanded="false"'));
});

/* --------------------------------------------------------------------------
 * Picker
 * -------------------------------------------------------------------------- */

test('the picker opens on a section of emoji with a tab strip', () => {
  const html = renderToStaticMarkup(
    React.createElement(EmojiPicker, { onChoose: () => {}, onDismiss: () => {} }),
  );

  assert.ok(html.includes('>Smileys &amp; People</div>'), 'first section is missing');
  assert.ok(html.includes('role="tablist"'), 'no tab strip');
  assert.ok(html.includes('😀'), 'no emoji rendered');

  // Every section has a tab, so the strip names all eight...
  assert.ok(html.includes('aria-label="Flags"'), 'no tab for the last section');

  // ...but only the ones near the viewport are mounted. Rendering all 1900
  // cells at once is what the offset arithmetic exists to avoid.
  const mounted = html.split('class="emoji-picker__section"').length - 1;
  assert.equal(mounted, 1, `${mounted} sections mounted`);
});

test('a suggestion list shows the emoji and its canonical name', () => {
  const html = renderToStaticMarkup(
    React.createElement(EmojiSuggestions, {
      query: 'tada',
      selected: 0,
      onChoose: () => {},
      onDismiss: () => {},
    }),
  );

  assert.ok(html.includes('🎉'));
  assert.ok(html.includes(':tada:'));
  assert.ok(html.includes('emoji-suggestion--selected'));
});

test('a suggestion list with no matches renders nothing at all', () => {
  const html = renderToStaticMarkup(
    React.createElement(EmojiSuggestions, {
      query: 'zzzznotathing',
      selected: 0,
      onChoose: () => {},
      onDismiss: () => {},
    }),
  );

  assert.equal(html, '');
});

/* --------------------------------------------------------------------------
 * Preferences
 * -------------------------------------------------------------------------- */

test('preferences fall back to defaults when storage is unavailable', () => {
  // There is no `localStorage` under the test runner, and reaching for it
  // throws rather than returning null. Every accessor has to survive that:
  // the pane still has to render.
  assert.equal(loadLeftPaneWidth(), DEFAULT_LEFT_PANE_WIDTH);
  assert.deepEqual(loadRecentEmoji(), []);
  assert.doesNotThrow(() => saveLeftPaneWidth(320));
  assert.doesNotThrow(() => noteRecentEmoji('🔥'));
});

test('a width is held inside what the layout can render', () => {
  assert.equal(clampLeftPaneWidth(10), MIN_LEFT_PANE_WIDTH);
  assert.equal(clampLeftPaneWidth(9999), MAX_LEFT_PANE_WIDTH);
  assert.equal(clampLeftPaneWidth(300.6), 301);
  assert.equal(clampLeftPaneWidth(Number.NaN), DEFAULT_LEFT_PANE_WIDTH);
  assert.equal(clampLeftPaneWidth(Number.POSITIVE_INFINITY), DEFAULT_LEFT_PANE_WIDTH);
});

test('the default width sits inside its own limits', () => {
  assert.ok(DEFAULT_LEFT_PANE_WIDTH >= MIN_LEFT_PANE_WIDTH);
  assert.ok(DEFAULT_LEFT_PANE_WIDTH <= MAX_LEFT_PANE_WIDTH);
});

test('recent emoji move to the front without duplicating', () => {
  // A real store, so the list logic is exercised rather than the fallback.
  const store = new Map<string, string>();
  (globalThis as { localStorage?: unknown }).localStorage = {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => void store.set(key, value),
  };

  try {
    assert.deepEqual(noteRecentEmoji('🔥'), ['🔥']);
    assert.deepEqual(noteRecentEmoji('🎉'), ['🎉', '🔥']);
    assert.deepEqual(noteRecentEmoji('🔥'), ['🔥', '🎉']);

    for (let index = 0; index < MAX_RECENT_EMOJI + 4; index += 1) {
      noteRecentEmoji(`x${index}`);
    }
    assert.equal(loadRecentEmoji().length, MAX_RECENT_EMOJI);
  } finally {
    delete (globalThis as { localStorage?: unknown }).localStorage;
  }
});

test('a corrupt recents list is ignored rather than thrown on', () => {
  const store = new Map<string, string>([['bounce.recentEmoji', '{not json']]);
  (globalThis as { localStorage?: unknown }).localStorage = {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => void store.set(key, value),
  };

  try {
    assert.deepEqual(loadRecentEmoji(), []);
  } finally {
    delete (globalThis as { localStorage?: unknown }).localStorage;
  }
});

test('a stored width outside the current limits is clamped, not discarded', () => {
  const store = new Map<string, string>([['bounce.leftPaneWidth', '9999']]);
  (globalThis as { localStorage?: unknown }).localStorage = {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => void store.set(key, value),
  };

  try {
    assert.equal(loadLeftPaneWidth(), MAX_LEFT_PANE_WIDTH);
    store.set('bounce.leftPaneWidth', 'nonsense');
    assert.equal(loadLeftPaneWidth(), DEFAULT_LEFT_PANE_WIDTH);
  } finally {
    delete (globalThis as { localStorage?: unknown }).localStorage;
  }
});

test('the picker and the typeahead agree about what a name means', () => {
  // Both go through `emojiForShortcode`, so this is really a guard against a
  // future shortcut that looks the name up some other way.
  assert.equal(emojiForShortcode('tada')?.char, '🎉');
});

/* --------------------------------------------------------------------------
 * Large files
 * -------------------------------------------------------------------------- */

/** A `File` of a given size without allocating one, which at 20 MiB matters. */
function fakeFile(name: string, size: number, type: string): File {
  return {
    name,
    size,
    type,
    arrayBuffer: async () => new ArrayBuffer(Math.min(size, 1024)),
  } as unknown as File;
}

test('a file past the embedding limit is staged by path, not read into memory', async () => {
  // The limit is not a cap on what may be sent. Reading a two-gigabyte file
  // only to refuse it is the behaviour this replaces.
  const staged = await stageFile(
    fakeFile('recording.mov', EMBEDDED_FILE_LIMIT + 1, 'video/quicktime'),
    () => '/Users/someone/recording.mov',
  );

  assert.ok(!('error' in staged), 'the file was refused');
  assert.equal(staged.path, '/Users/someone/recording.mov');
  assert.equal(staged.bytes.length, 0, 'the bytes were read after all');
  assert.equal(staged.name, 'recording.mov');
  assert.equal(staged.size, EMBEDDED_FILE_LIMIT + 1);
});

test('a large item with no file on disk is still refused', async () => {
  // A pasted screenshot is a blob that never existed as a file, so there is
  // nothing to stream from and the limit genuinely applies.
  const staged = await stageFile(fakeFile('', EMBEDDED_FILE_LIMIT + 1, 'image/png'), () => '');

  assert.ok('error' in staged);
  assert.match(staged.error, /no file on disk/);
});

test('a file inside the limit is still embedded', async () => {
  // The path is available either way; size is what decides, not availability.
  const staged = await stageFile(fakeFile('note.txt', 12, 'text/plain'), () => '/tmp/note.txt');

  assert.ok(!('error' in staged));
  assert.equal(staged.path, undefined, 'a small file should not be streamed');
  assert.equal(staged.bytes.length, 12);
});

test('exactly at the limit is embedded, one byte over is streamed', async () => {
  const at = await stageFile(fakeFile('a', EMBEDDED_FILE_LIMIT, 'application/octet-stream'), () => '/tmp/a');
  const over = await stageFile(fakeFile('b', EMBEDDED_FILE_LIMIT + 1, 'application/octet-stream'), () => '/tmp/b');

  assert.ok(!('error' in at) && at.path === undefined);
  assert.ok(!('error' in over) && over.path === '/tmp/b');
});
