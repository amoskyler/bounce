/**
 * Tests for what an avatar draws.
 *
 * A contact's `images` have always reached the renderer and were always thrown
 * away; the point of these is that the photo is now used, that the *newest* one
 * is the one used — Go walks the list from the end for the same reason
 * (`ui/default_image.go:84-100`) — and that everything else still falls back to
 * initials, including a file whose bytes have not arrived.
 *
 * Rendered with `react-dom/server`, so effects never run and no bridge call is
 * made from the component itself; the cache is filled first, which is the state
 * the component finds itself in on the render after the bytes land.
 *
 * Bundle and run:
 *
 *   npx esbuild src/renderer/__tests__/avatar.test.ts --bundle \
 *     --platform=node --format=cjs --loader:.css=empty \
 *     --outfile=/tmp/avatar.test.cjs && node --test /tmp/avatar.test.cjs
 */

import assert from 'node:assert/strict';
import test from 'node:test';

import * as React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

import { Avatar } from '../Avatar';
import { fileUrl } from '../attachment-data';

const ID = '2b0f6a1e-2f4a-4b7e-9a1b-3c5d7e9f0a11';

/**
 * Stand in for the main process and for the renderer's blob URLs.
 *
 * Object URLs are the one thing about this path that node has no answer for,
 * so each file gets a name of its own and the assertions read the file id back
 * out of the URL.
 */
function stubBridge(): void {
  const global = globalThis as unknown as {
    window: unknown;
    URL: { createObjectURL: (blob: unknown) => string; revokeObjectURL: (url: string) => void };
  };

  let pending = '';
  global.window = {
    bounce: {
      fileData: (fileId: string) => {
        pending = fileId;
        return Promise.resolve(new Uint8Array([0x89, 0x50, 0x4e, 0x47]));
      },
    },
  };
  global.URL.createObjectURL = () => `blob:${pending}`;
  global.URL.revokeObjectURL = () => {};
}

function render(images?: string[]): string {
  return renderToStaticMarkup(
    React.createElement(Avatar, { id: ID, name: 'Ada Lovelace', images, size: 48 }),
  );
}

test('a contact with no images gets initials', () => {
  const html = render();
  assert.match(html, /avatar__initials">AL</);
  assert.doesNotMatch(html, /<img/);
});

test('an empty image list gets initials', () => {
  assert.match(render([]), /avatar__initials">AL</);
});

test('an image whose bytes have not arrived falls back to initials', () => {
  // Nothing has been fetched for this id, so there is no object URL for it and
  // the circle is the same one a contact with no photo gets.
  const html = render(['1a2b3c4d-0000-4000-8000-000000000001']);
  assert.match(html, /avatar__initials">AL</);
  assert.doesNotMatch(html, /<img/);
});

test('a fetched image is drawn instead of the initials', async () => {
  stubBridge();
  const fileId = '1a2b3c4d-0000-4000-8000-00000000000a';
  await fileUrl(fileId);

  const html = render([fileId]);
  assert.match(html, new RegExp(`<img[^>]*src="blob:${fileId}"`));
  assert.doesNotMatch(html, /avatar__initials/);
});

test('the newest image wins when a contact has changed their photo', async () => {
  stubBridge();
  const older = '1a2b3c4d-0000-4000-8000-00000000000b';
  const newer = '1a2b3c4d-0000-4000-8000-00000000000c';
  await fileUrl(older);
  await fileUrl(newer);

  const html = render([older, newer]);
  assert.match(html, new RegExp(`src="blob:${newer}"`));
  assert.doesNotMatch(html, new RegExp(`src="blob:${older}"`));
});

test('the tint stays under the image, for photos with transparency', async () => {
  stubBridge();
  const fileId = '1a2b3c4d-0000-4000-8000-00000000000d';
  await fileUrl(fileId);

  // The wrapper keeps its palette background whether or not there is a photo,
  // and the same one either way: the colour is derived from the id.
  const withPhoto = render([fileId]);
  const background = /background:([^;"]+)/.exec(render())?.[1];
  assert.ok(background, 'the initials circle has no background to compare against');
  assert.ok(withPhoto.includes(`background:${background}`));
});
