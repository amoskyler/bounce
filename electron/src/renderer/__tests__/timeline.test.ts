/**
 * Tests for where a conversation opens.
 *
 * Go opens a thread at the last thing the reader read rather than at its newest
 * message (`ui/chat_history.go:534-551`), and which row that is comes down to
 * one walk over the merged list of bubbles and status rows. The walk is the
 * part worth pinning down: the scrolling around it is DOM measurement, but
 * getting "unread" wrong means opening the thread in the wrong place, or
 * marking the wrong thing read.
 *
 * Bundle and run:
 *
 *   npx esbuild src/renderer/__tests__/timeline.test.ts --bundle \
 *     --platform=node --format=cjs --loader:.css=empty \
 *     --outfile=/tmp/timeline.test.cjs && node --test /tmp/timeline.test.cjs
 */

import assert from 'node:assert/strict';
import test from 'node:test';

import { firstUnreadIndex, type Entry } from '../Conversation';
import type { Message, SystemMessage } from '../../preload';

let sequence = 0;

/** A message row. `seen` is what the engine reports, not what is on screen. */
function message(options: { outgoing?: boolean; seen?: boolean } = {}): Entry {
  sequence += 1;
  const value: Message = {
    id: `m${sequence}`,
    thread: 't',
    author: options.outgoing ? 'me' : 'them',
    text: 'hello',
    writtenAt: sequence,
    expiresAt: 0,
    seen: options.seen ?? false,
    undeliverable: false,
    deliveredTo: [],
    readBy: [],
    attachments: [],
    outgoing: options.outgoing ?? false,
  };
  return { kind: 'message', at: value.writtenAt, id: value.id, message: value };
}

/** A status row: a rename, a departure. Never something to be read. */
function status(): Entry {
  sequence += 1;
  const value: SystemMessage = {
    id: `s${sequence}`,
    thread: 't',
    actor: 'them',
    kind: 'groupRenamed',
    value: 'Lunch',
    timestamp: sequence,
  };
  return { kind: 'system', at: value.timestamp, id: value.id, system: value };
}

test('a thread with nothing in it has no unread row', () => {
  assert.equal(firstUnreadIndex([]), -1);
});

test('a fully read thread has no unread row', () => {
  assert.equal(firstUnreadIndex([message({ seen: true }), message({ seen: true })]), -1);
});

test('the first unseen incoming message is the one', () => {
  const entries = [message({ seen: true }), message({ seen: true }), message(), message()];
  assert.equal(firstUnreadIndex(entries), 2);
});

test('an unsent outgoing message is not unread', () => {
  // `seen` is about the reader, and one's own message is never waiting to be
  // read; the whole thread here is caught up.
  assert.equal(firstUnreadIndex([message({ outgoing: true }), message({ outgoing: true })]), -1);
});

test('a status row is skipped rather than counted', () => {
  const entries = [message({ seen: true }), status(), status(), message()];
  assert.equal(firstUnreadIndex(entries), 3);
});

test('an unread message at the very top is index zero, not the row above it', () => {
  // The caller scrolls to the row above; from index 0 there is none, which is
  // the case Go handles with ScrollToTop.
  assert.equal(firstUnreadIndex([message(), message({ seen: true })]), 0);
});

test('an outgoing message after the unread one does not move the answer', () => {
  const entries = [
    message({ seen: true }),
    message(),
    message({ outgoing: true }),
    message(),
  ];
  assert.equal(firstUnreadIndex(entries), 1);
});

test('a thread of nothing but status rows opens at the end', () => {
  assert.equal(firstUnreadIndex([status(), status()]), -1);
});
