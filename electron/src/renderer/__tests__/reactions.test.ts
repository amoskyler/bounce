/**
 * Folding a reaction event into a message's pills.
 *
 * The engine groups reactions when it builds a snapshot; this is the same
 * grouping applied incrementally, as events arrive. The two have to agree —
 * if they do not, the bug only appears after a restart, when the timeline is
 * built the other way.
 */

import assert from 'node:assert/strict';
import test from 'node:test';

import { withReaction } from '../state';
import type { Reaction } from '../../preload';

const ME = 'me';

test('a first reaction creates its pill', () => {
  const next = withReaction([], 'ada', '👍', ME);
  assert.deepEqual(next, [{ emoji: '👍', users: ['ada'], mine: false }]);
});

test('our own reaction is marked as ours', () => {
  const next = withReaction([], ME, '👍', ME);
  assert.equal(next[0].mine, true);
});

test('a second person joins an existing pill', () => {
  const first = withReaction([], 'ada', '👍', ME);
  const next = withReaction(first, 'grace', '👍', ME);

  assert.equal(next.length, 1);
  assert.deepEqual(next[0].users, ['ada', 'grace']);
});

test('changing your mind moves you rather than counting you twice', () => {
  // One reaction per person. Without the removal the same actor ends up under
  // two emoji, and the counts add up to more people than are in the room.
  let reactions: Reaction[] = withReaction([], 'ada', '👍', ME);
  reactions = withReaction(reactions, 'grace', '👍', ME);
  reactions = withReaction(reactions, 'ada', '🎉', ME);

  assert.deepEqual(reactions, [
    { emoji: '👍', users: ['grace'], mine: false },
    { emoji: '🎉', users: ['ada'], mine: false },
  ]);
});

test('withdrawing removes the person, and the pill once it is empty', () => {
  let reactions: Reaction[] = withReaction([], 'ada', '👍', ME);
  reactions = withReaction(reactions, 'grace', '👍', ME);

  reactions = withReaction(reactions, 'ada', '', ME);
  assert.deepEqual(reactions, [{ emoji: '👍', users: ['grace'], mine: false }]);

  reactions = withReaction(reactions, 'grace', '', ME);
  assert.deepEqual(reactions, []);
});

test('withdrawing our own clears the mine flag', () => {
  let reactions: Reaction[] = withReaction([], ME, '👍', ME);
  reactions = withReaction(reactions, 'ada', '👍', ME);
  assert.equal(reactions[0].mine, true);

  reactions = withReaction(reactions, ME, '', ME);
  assert.equal(reactions[0].mine, false, 'the pill is no longer ours');
  assert.deepEqual(reactions[0].users, ['ada']);
});

test('pills keep first-use order when somebody joins a later one', () => {
  // An established reaction jumping position because a third person agreed with
  // a different one is movement under the pointer for no reason.
  let reactions: Reaction[] = withReaction([], 'ada', '👍', ME);
  reactions = withReaction(reactions, 'grace', '🎉', ME);
  reactions = withReaction(reactions, 'alan', '🎉', ME);

  assert.deepEqual(
    reactions.map((entry) => entry.emoji),
    ['👍', '🎉'],
  );
});

test('the input is not mutated', () => {
  // The reducer hands a message's own array straight in, and React compares by
  // identity — mutating it would apply the change while telling React nothing
  // happened.
  const before: Reaction[] = [{ emoji: '👍', users: ['ada'], mine: false }];
  const snapshot = JSON.parse(JSON.stringify(before));

  withReaction(before, 'grace', '👍', ME);
  assert.deepEqual(before, snapshot);
});
