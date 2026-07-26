/**
 * The main process's attachment adapter.
 *
 * This layer exists because the same shape is declared three times — in the
 * renderer, in the napi struct, and in between — and the translation is written
 * out field by field. That is exactly the kind of code that fails silently: a
 * field added to the other two and forgotten here is dropped without a type
 * error, because the source object simply has a property the target does not
 * mention.
 *
 * It happened. `path` was added for files too large to embed; the adapter kept
 * copying the other six fields; and a 800 MB attachment reached the engine with
 * neither its contents nor anywhere to read them from. Every layer either side
 * tested clean, because each one was correct.
 *
 * Bundle and run:
 *
 *   npx esbuild src/renderer/__tests__/attachments.test.ts --bundle \
 *     --platform=node --format=cjs --outfile=/tmp/attachments.test.cjs \
 *     && node --test /tmp/attachments.test.cjs
 */

import assert from 'node:assert/strict';
import test from 'node:test';

import {
  checkAttachments,
  toNative,
  type OutgoingAttachment,
} from '../../main/attachments';

function embedded(overrides: Partial<OutgoingAttachment> = {}): OutgoingAttachment {
  return {
    name: 'photo.png',
    data: new Uint8Array([1, 2, 3, 4]),
    isImage: true,
    width: 800,
    height: 600,
    blurHash: 'LEHV6nWB',
    ...overrides,
  };
}

function streamed(overrides: Partial<OutgoingAttachment> = {}): OutgoingAttachment {
  return {
    name: 'technocalyps.mkv',
    data: new Uint8Array(0),
    isImage: false,
    width: 0,
    height: 0,
    blurHash: '',
    path: '/Users/someone/technocalyps.mkv',
    ...overrides,
  };
}

/* --------------------------------------------------------------------------
 * Translation
 * -------------------------------------------------------------------------- */

test('every field the renderer sets survives the crossing', () => {
  // Asserted as a whole object rather than field by field, so a field added to
  // the renderer and forgotten here fails instead of going unnoticed.
  assert.deepEqual(toNative(embedded()), {
    name: 'photo.png',
    data: Buffer.from([1, 2, 3, 4]),
    isImage: true,
    width: 800,
    height: 600,
    blurHash: 'LEHV6nWB',
    path: '',
  });
});

test('the path of a streamed file reaches the engine', () => {
  // The regression itself. Without this the engine sees an attachment with no
  // bytes and no path, and can only report an empty file.
  const native = toNative(streamed());

  assert.equal(native.path, '/Users/someone/technocalyps.mkv');
  assert.equal(native.data.length, 0, 'a streamed file must not carry bytes');
});

test('an absent blur hash or path becomes an empty string, not undefined', () => {
  // The napi struct takes both as required strings on the other side.
  const native = toNative(embedded({ blurHash: undefined, path: undefined }));
  assert.equal(native.blurHash, '');
  assert.equal(native.path, '');
});

test('the bytes are wrapped, not copied', () => {
  // A copy would double the peak memory of a 20 MiB attachment for nothing.
  const source = embedded({ data: new Uint8Array([9, 8, 7]) });
  const native = toNative(source);

  assert.equal(native.data.buffer, source.data.buffer);
  assert.deepEqual([...native.data], [9, 8, 7]);
});

test('a view into a larger buffer keeps its own bounds', () => {
  // `Buffer.from(view.buffer)` alone would hand over the whole backing store.
  const backing = new Uint8Array([0, 0, 5, 6, 0, 0]);
  const view = backing.subarray(2, 4);

  assert.deepEqual([...toNative(embedded({ data: view })).data], [5, 6]);
});

/* --------------------------------------------------------------------------
 * Refusals
 * -------------------------------------------------------------------------- */

test('an attachment with neither contents nor a path is refused by name', () => {
  assert.throws(
    () => checkAttachments([streamed({ path: '' })], () => true),
    /technocalyps\.mkv.*no contents and no path/s,
  );
});

test('a path that does not resolve is refused before the engine sees it', () => {
  // Otherwise this surfaces much later, inside a hash loop, as an IO error
  // that never mentions which attachment it came from.
  assert.throws(
    () => checkAttachments([streamed()], () => false),
    /technocalyps\.mkv could not be read from/,
  );
});

test('a streamed file whose path resolves is allowed through', () => {
  assert.doesNotThrow(() => checkAttachments([streamed()], () => true));
});

test('an embedded file is never asked about the filesystem', () => {
  // Its bytes are already here; touching the disk would be both pointless and
  // a way to refuse a perfectly good pasted screenshot.
  let asked = false;
  checkAttachments([embedded()], () => {
    asked = true;
    return false;
  });
  assert.equal(asked, false);
});

test('an unnamed attachment still produces a readable refusal', () => {
  assert.throws(
    () => checkAttachments([streamed({ name: '', path: '' })], () => true),
    /an attachment arrived with no contents/,
  );
});

test('every attachment in a batch is checked, not just the first', () => {
  assert.throws(
    () => checkAttachments([embedded(), streamed({ path: '' })], () => true),
    /technocalyps\.mkv/,
  );
});
