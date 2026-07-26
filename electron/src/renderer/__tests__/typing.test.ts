/**
 * Naming who is typing.
 *
 * The bug this pins down was not missing data — the engine always sent a user
 * id and the reducer always kept it. The view reduced the set to `length` and
 * drew anonymous dots, so in a group of eight you learned only that somebody
 * was composing something.
 */

import assert from 'node:assert/strict';
import test from 'node:test';

import { MAXIMUM_NAMED_TYPISTS, typingAvatarIds, typingLabel } from '../typing';

const NAMES = {
  ada: 'Ada',
  bo: 'Bo',
  cy: 'Cy',
  dee: 'Dee',
  me: 'You',
};

const group = { isGroup: true, selfId: 'me' };

test('nobody typing produces no label', () => {
  assert.equal(typingLabel([], NAMES, group), null);
});

test('one person in a group is named', () => {
  assert.equal(typingLabel(['ada'], NAMES, group), 'Ada is typing');
});

test('two people read as a pair, and the verb agrees', () => {
  assert.equal(typingLabel(['ada', 'bo'], NAMES, group), 'Ada and Bo are typing');
});

test('three or more use a serial comma-free list', () => {
  assert.equal(
    typingLabel(['ada', 'bo', 'cy'], NAMES, group),
    'Ada, Bo and Cy are typing',
  );
});

test('a one-to-one conversation shows dots alone', () => {
  // Only one person can be typing at you, and their name is already in the
  // header — repeating it there is noise.
  assert.equal(typingLabel(['ada'], NAMES, { isGroup: false, selfId: 'me' }), null);
});

test('our own device is never reported back at us', () => {
  assert.equal(typingLabel(['me'], NAMES, group), null);
  assert.equal(typingLabel(['me', 'ada'], NAMES, group), 'Ada is typing');
});

test('a user the name table does not know still counts', () => {
  // Better a vague sentence than dropping a typist, which is the bug again.
  assert.equal(typingLabel(['ghost'], NAMES, group), 'Someone is typing');
});

test('beyond the cap the remainder becomes a count', () => {
  const many = Array.from({ length: MAXIMUM_NAMED_TYPISTS + 3 }, (_, i) => `u${i}`);
  const label = typingLabel(many, {}, group);

  assert.ok(label !== null);
  assert.ok(label.endsWith('and 3 others are typing'), label);
  // Exactly the cap is named, no more.
  assert.equal(label.split(', ').length, MAXIMUM_NAMED_TYPISTS);
});

test('one over the cap says "1 other", not "1 others"', () => {
  const many = Array.from({ length: MAXIMUM_NAMED_TYPISTS + 1 }, (_, i) => `u${i}`);
  assert.ok(typingLabel(many, {}, group)?.includes('and 1 other are typing'));
});

test('the newest typists are the ones kept', () => {
  // The engine reports oldest first, and Go shows the most recent when it has
  // room for only one. Truncating from the front keeps the same choice.
  const many = [...Array.from({ length: MAXIMUM_NAMED_TYPISTS }, (_, i) => `old${i}`), 'ada'];
  assert.ok(typingLabel(many, NAMES, group)?.includes('Ada'));
});

test('avatars follow the same set and cap', () => {
  assert.deepEqual(typingAvatarIds(['ada', 'bo'], { selfId: 'me' }), ['ada', 'bo']);
  assert.deepEqual(typingAvatarIds(['me', 'ada'], { selfId: 'me' }), ['ada']);

  const many = Array.from({ length: MAXIMUM_NAMED_TYPISTS + 5 }, (_, i) => `u${i}`);
  assert.equal(typingAvatarIds(many).length, MAXIMUM_NAMED_TYPISTS);
});
