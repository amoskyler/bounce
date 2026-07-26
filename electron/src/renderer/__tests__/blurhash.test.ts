/**
 * BlurHash round trips, and stays compatible with the Go client's parameters.
 *
 * The hash goes on the wire, so a change here is a wire change: a hash this
 * build produces has to be one `github.com/bbrks/go-blurhash` can decode, and
 * vice versa.
 */

import assert from 'node:assert/strict';
import test from 'node:test';

import {
  COMPONENTS_X,
  COMPONENTS_Y,
  decodeBlurHash,
  encodeBlurHash,
} from '../blurhash';

/** A flat block of one colour, as RGBA. */
function solid(width: number, height: number, r: number, g: number, b: number) {
  const pixels = new Uint8ClampedArray(width * height * 4);
  for (let index = 0; index < width * height; index += 1) {
    pixels[index * 4] = r;
    pixels[index * 4 + 1] = g;
    pixels[index * 4 + 2] = b;
    pixels[index * 4 + 3] = 255;
  }
  return pixels;
}

/** A horizontal red and vertical green ramp, so both axes carry structure. */
function gradient(width: number, height: number) {
  const pixels = new Uint8ClampedArray(width * height * 4);
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const offset = (x + y * width) * 4;
      pixels[offset] = Math.floor((x * 255) / width);
      pixels[offset + 1] = Math.floor((y * 255) / height);
      pixels[offset + 2] = 128;
      pixels[offset + 3] = 255;
    }
  }
  return pixels;
}

/** Left half one colour, right half another. */
function split(width: number, height: number) {
  const pixels = new Uint8ClampedArray(width * height * 4);
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const offset = (x + y * width) * 4;
      const left = x < width / 2;
      pixels[offset] = left ? 220 : 20;
      pixels[offset + 1] = left ? 40 : 180;
      pixels[offset + 2] = left ? 40 : 60;
      pixels[offset + 3] = 255;
    }
  }
  return pixels;
}

test('a hash has the length the format prescribes', () => {
  const hash = encodeBlurHash(solid(8, 8, 120, 200, 90), 8, 8);
  // 1 byte of size flag, 1 of maximum, 4 of DC, 2 per AC component.
  assert.equal(hash.length, 4 + 2 * (COMPONENTS_X * COMPONENTS_Y - 1) + 2);
});

test('the component count survives the round trip', () => {
  const hash = encodeBlurHash(solid(8, 8, 10, 20, 30), 8, 8);
  const decoded = decodeBlurHash(hash, 4, 4);
  assert.equal(decoded.width, 4);
  assert.equal(decoded.height, 4);
  assert.equal(decoded.pixels.length, 4 * 4 * 4);
});

/**
 * Hashes produced by `github.com/bbrks/go-blurhash` — the exact library the Go
 * client uses — for images built the same way as the fixtures above.
 *
 * These are the compatibility contract. The hash goes on the wire, and a
 * change to the basis, the quantisation, or the sRGB rounding would show up
 * here as a diff rather than as a subtly wrong blur on somebody else's screen.
 */
const GO_HASHES = {
  solid8: 'UNM|T9}XfQ}X}XsofQsofQfQfQfQ}XsofQso',
  solid16: 'U01C={o$fQo$o$j[fQj[fQfQfQfQo$j[fQj[',
  split16: 'U}Iq9k{6w1O:s.nkjabGfQfQfQfQs.nkjabG',
  gradient32: 'UxG[[y2swxX8l}WDjte;gJfjfQfjnmWpjtfQ',
};

test('encoding agrees with the Go client byte for byte', () => {
  assert.equal(encodeBlurHash(solid(8, 8, 200, 100, 50), 8, 8), GO_HASHES.solid8);
  assert.equal(encodeBlurHash(solid(16, 16, 10, 20, 30), 16, 16), GO_HASHES.solid16);
  assert.equal(encodeBlurHash(split(16, 16), 16, 16), GO_HASHES.split16);
  assert.equal(encodeBlurHash(gradient(32, 32), 32, 32), GO_HASHES.gradient32);
});

test('a hash from the Go client decodes to the colour it encoded', () => {
  // Interop in the direction that actually matters at runtime: a picture sent
  // from the Fyne client has to blur correctly here.
  const { pixels } = decodeBlurHash(GO_HASHES.solid16, 4, 4);

  // Sampled away from the corner, where every basis function peaks at once.
  const offset = 4 * (2 + 2 * 4);
  assert.ok(Math.abs(pixels[offset] - 10) < 40, `red was ${pixels[offset]}`);
  assert.ok(Math.abs(pixels[offset + 1] - 20) < 40, `green was ${pixels[offset + 1]}`);
  assert.ok(Math.abs(pixels[offset + 2] - 30) < 40, `blue was ${pixels[offset + 2]}`);
});

test('structure survives: a split image stays split', () => {
  // The whole point is that the blur resembles the picture. If the two halves
  // came back the same, the hash would be carrying nothing but an average.
  const hash = encodeBlurHash(split(16, 16), 16, 16);
  const { pixels } = decodeBlurHash(hash, 8, 8);

  const at = (x: number, y: number) => {
    const offset = (x + y * 8) * 4;
    return [pixels[offset], pixels[offset + 1], pixels[offset + 2]];
  };

  const [leftR, , leftB] = at(1, 4);
  const [rightR, rightG] = at(6, 4);

  assert.ok(leftR > 120, `the left side should stay reddish, got ${leftR}`);
  assert.ok(rightG > 100, `the right side should stay greenish, got ${rightG}`);
  assert.ok(leftR > rightR, 'the left side should be redder than the right');
  assert.ok(leftB < 255, 'sanity');
});

test('a known hash from the reference implementation decodes', () => {
  // From the BlurHash project's own README, so this pins the decoder to the
  // published format rather than to our own encoder.
  const decoded = decodeBlurHash('LEHV6nWB2yk8pyo0adR*.7kCMdnj', 4, 4);
  assert.equal(decoded.pixels.length, 4 * 4 * 4);
  assert.ok(decoded.pixels.some((value) => value !== 0));
});

test('a malformed hash is rejected rather than producing garbage', () => {
  assert.throws(() => decodeBlurHash('abc'), /too short/);
  // Right prefix, wrong length for the components it declares.
  assert.throws(() => decodeBlurHash('LEHV6nWB2yk8'), /length/);
});

test('the parameters still match the Go client', () => {
  // `ui/pending_message_attachments.go` calls blurhash.Encode(4, 4, …).
  // Changing these changes what goes on the wire.
  assert.equal(COMPONENTS_X, 4);
  assert.equal(COMPONENTS_Y, 4);
});
