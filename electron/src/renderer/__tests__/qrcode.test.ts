/**
 * Tests for the QR encoder.
 *
 * A QR code is either right or unscannable, and nothing in between is visible
 * by eye, so the anchor here is the whole matrix of a real pairing code taken
 * from `github.com/piglig/go-qr` — the encoder the Fyne client already links
 * against. If a version, a block layout, the Reed-Solomon remainder, the
 * interleave, the mask choice or a format bit drifts, this fixture stops
 * matching, and the two clients would have drawn different squares for the
 * same code.
 *
 * The fixture was generated with:
 *
 *   go_qr.EncodeText("<the code below>", go_qr.Medium)
 *
 * Bundle and run:
 *
 *   npx esbuild src/renderer/__tests__/qrcode.test.ts --bundle \
 *     --platform=node --format=cjs --outfile=/tmp/qrcode.test.cjs \
 *     && node --test /tmp/qrcode.test.cjs
 */

import assert from 'node:assert/strict';
import test from 'node:test';

import * as React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

import { QrCode } from '../icons';
import { encodeQr } from '../qrcode';

/** An address-and-secret pair in exactly the shape `create_pairing_code` emits. */
const PAIRING_CODE =
  'nkd7yqfgrp2vhtsbz4mxwaeuc6j3l5o8dgtx9qmnbvcrzp4wsyek:9f3c1a7b0e5d248690bcafde13572468';

/** What the Go encoder produces for it: version 6, error correction M. */
const EXPECTED = [
  '11111110011000111101010101100011101111111',
  '10000010011001110100110011110010001000001',
  '10111010111101101110111111011000001011101',
  '10111010111010110000010000110010101011101',
  '10111010100111101011101100001001101011101',
  '10000010100010001000111010110110001000001',
  '11111110101010101010101010101010101111111',
  '00000000100001110000011110001010100000000',
  '10111110001010011101101111101100101111100',
  '00011101100110111011111100101101111111101',
  '10011010110011001000110011010010111110110',
  '00111000000110111000110000010011110011001',
  '00101011001011011100100011101110010100100',
  '10010101000011000010011001110000110010011',
  '00110111000001111010100110011001011101100',
  '10110101110011110000011100010001100101000',
  '01010111110011111110001111011111100100111',
  '11011000100000110001100100100101001111001',
  '01101110010101100010001010010100011100110',
  '10001000110110111000100010100111010110011',
  '01001110101101000010111000010000100101111',
  '10111001000110100001100100101011111010011',
  '10000011101000110110110011011010110000100',
  '00001000111011010001010110111011100110000',
  '00001110011101010110001001100110010101100',
  '01010001001010011101111110001101101010101',
  '01101110011110110110111011110010001101110',
  '10111001010110011000010110001001111011000',
  '00111011010001110110001101110100101000100',
  '11010101100100011000111010011000010111101',
  '10010110010010110111010001100000110110010',
  '10111101011010000001111010001000000110011',
  '10100110001100101111001001001100111111111',
  '00000000111100111101110111100010100010011',
  '11111110010000011100010001110011101011100',
  '10000010101101000111111011011001100011011',
  '10111010111101101000110100110010111110100',
  '10111010101101100011111100001001100101000',
  '10111010110011000110101001011111011010000',
  '10000010010000000101111110010010100010010',
  '11111110101110111100001001111101010111100',
];

/** The matrix as one string per row, for a readable diff when it fails. */
function rows(text: string): string[] {
  return encodeQr(text).modules.map((row) => row.map((dark) => (dark ? '1' : '0')).join(''));
}

test('a pairing code encodes to the square the Go client draws', () => {
  assert.deepEqual(rows(PAIRING_CODE), EXPECTED);
});

test('the version is the smallest that holds the payload', () => {
  // Sizes are 4v+17, and these are the versions the Go encoder picks: level M
  // for the version choice, then raised as far as the same square allows.
  assert.equal(encodeQr('a').size, 21);
  assert.equal(encodeQr('a'.repeat(89)).size, 41);
  assert.equal(encodeQr('a'.repeat(96)).size, 41);
  assert.equal(encodeQr('a'.repeat(150)).size, 49);
  assert.equal(encodeQr('a'.repeat(300)).size, 69);
  assert.equal(encodeQr('a'.repeat(700)).size, 101);
});

test('every symbol is square and sized 4v+17', () => {
  for (const length of [1, 20, 89, 400]) {
    const matrix = encodeQr('x'.repeat(length));
    assert.equal(matrix.modules.length, matrix.size);
    for (const row of matrix.modules) assert.equal(row.length, matrix.size);
    assert.equal((matrix.size - 17) % 4, 0);
  }
});

test('the three finder patterns are where a reader looks for them', () => {
  const matrix = encodeQr(PAIRING_CODE);
  const last = matrix.size - 7;

  for (const [ox, oy] of [
    [0, 0],
    [last, 0],
    [0, last],
  ]) {
    for (let dy = 0; dy < 7; dy += 1) {
      for (let dx = 0; dx < 7; dx += 1) {
        // A finder is a 7x7 ring around a 3x3 core: dark except for the ring
        // of light modules one in from the edge.
        const edge = Math.max(Math.abs(dx - 3), Math.abs(dy - 3));
        assert.equal(
          matrix.modules[oy + dy][ox + dx],
          edge !== 2,
          `finder at ${ox},${oy} wrong at ${dx},${dy}`,
        );
      }
    }
  }
});

test('the timing patterns alternate across the whole symbol', () => {
  const matrix = encodeQr(PAIRING_CODE);
  for (let i = 8; i < matrix.size - 8; i += 1) {
    assert.equal(matrix.modules[6][i], i % 2 === 0, `row timing at ${i}`);
    assert.equal(matrix.modules[i][6], i % 2 === 0, `column timing at ${i}`);
  }
});

test('a payload no version can hold is refused rather than truncated', () => {
  // Version 40 at level M holds 2,331 bytes.
  assert.doesNotThrow(() => encodeQr('a'.repeat(2331)));
  assert.throws(() => encodeQr('a'.repeat(2332)), /too long/);
});

test('multi-byte text is measured in utf-8 bytes, not characters', () => {
  // Two-byte characters, so this fills the same square as 178 ascii ones.
  assert.equal(encodeQr('é'.repeat(89)).size, encodeQr('a'.repeat(178)).size);
});

/* -------------------------------------------------------------------------
 * Drawing it
 * ---------------------------------------------------------------------- */

function draw(text: string): string {
  return renderToStaticMarkup(React.createElement(QrCode, { text, size: 180 }));
}

test('the drawn symbol carries the quiet zone a reader needs', () => {
  // Four modules of light space on every side, around a 41 module symbol.
  assert.match(draw(PAIRING_CODE), /viewBox="0 0 49 49"/);
  assert.match(draw(PAIRING_CODE), /width="180" height="180"/);
});

test('the modules are drawn dark on light, whatever the theme is', () => {
  const svg = draw(PAIRING_CODE);
  assert.match(svg, /<rect[^>]+fill="#ffffff"/);
  assert.match(svg, /<path[^>]+fill="#000000"/);
});

test('a row of the drawn path matches the row of the matrix', () => {
  const svg = draw(PAIRING_CODE);
  const path = /<path d="([^"]+)"/.exec(svg)?.[1] ?? '';

  // The first row of a symbol opens with a finder pattern: seven dark modules
  // from the left edge, which is one run, drawn four modules in from both.
  assert.ok(path.startsWith('M4 4h7v1h-7z'), path.slice(0, 40));

  // Every dark module is covered exactly once: the runs sum to the count.
  const drawn = [...path.matchAll(/h(\d+)v1/g)].reduce(
    (total, match) => total + Number(match[1]),
    0,
  );
  const dark = encodeQr(PAIRING_CODE).modules.flat().filter(Boolean).length;
  assert.equal(drawn, dark);
});

test('an empty string draws nothing rather than an empty square', () => {
  assert.equal(draw(''), '');
});
