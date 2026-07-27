/**
 * Which rung of the delivery ladder a message is on.
 *
 * The ordering is the whole of it, and it is the part that goes wrong: read
 * implies delivered, and a message that has been read but whose acknowledgement
 * never arrived — the counterparty's client sent a receipt before its device
 * ack made it back — must still show read rather than falling to sending.
 */

import assert from 'node:assert/strict';
import test from 'node:test';

import { DELIVERY_LABELS, deliveryState, type Deliverable } from '../delivery';

function message(overrides: Partial<Deliverable> = {}): Deliverable {
  return {
    outgoing: true,
    undeliverable: false,
    deliveredTo: [],
    readBy: [],
    ...overrides,
  };
}

test('an unacknowledged message is still sending', () => {
  assert.equal(deliveryState(message()), 'sending');
});

test('an acknowledgement from any device is delivery', () => {
  assert.equal(deliveryState(message({ deliveredTo: ['ada'] })), 'delivered');
});

test('a read receipt outranks delivery', () => {
  assert.equal(
    deliveryState(message({ deliveredTo: ['ada'], readBy: ['ada'] })),
    'read',
  );
});

test('a receipt that overtook its own acknowledgement still reads as read', () => {
  // Two different channels with no ordering between them: the receipt is a
  // signed frame the person's client chose to send, the acknowledgement is
  // automatic. Deriving read from delivered would drop this to sending.
  assert.equal(deliveryState(message({ readBy: ['ada'] })), 'read');
});

test('undeliverable outranks everything', () => {
  // The flag is only ever set after weeks of total non-delivery, so if it is
  // set alongside a receipt the receipt is the stale fact, not the flag.
  assert.equal(
    deliveryState(message({ undeliverable: true, deliveredTo: ['ada'], readBy: ['ada'] })),
    'undeliverable',
  );
});

test('one delivery in a group is enough', () => {
  // Signal ticks a group message off when it reaches anybody, not everybody;
  // the info panel is where "who exactly" is answered.
  assert.equal(
    deliveryState(message({ deliveredTo: ['ada', 'grace'] })),
    'delivered',
  );
});

test('every state has a label', () => {
  const states = ['sending', 'sent', 'delivered', 'read', 'undeliverable'] as const;
  for (const state of states) {
    assert.equal(typeof DELIVERY_LABELS[state], 'string');
    assert.ok(DELIVERY_LABELS[state].length > 0, state);
  }
});
