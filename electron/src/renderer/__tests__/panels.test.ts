/**
 * Tests for the details and settings panels.
 *
 * Two kinds. The pure ones pin the mappings that have to agree with the Fyne
 * client word for word — a tri-state override is three labels and two booleans,
 * and getting either wrong misreports a privacy setting. The rendering ones
 * pin what the controls are *seeded* with, because every bug in this area was a
 * control that displayed a default it had never been given: a notes box that
 * seeded from `''` and wrote that back, a retention select stuck on "Off".
 *
 * Rendered with `react-dom/server`, so effects never run and no bridge call is
 * made; only the first paint is under test, which is exactly where seeding is
 * observable.
 *
 * Bundle and run:
 *
 *   npx esbuild src/renderer/__tests__/panels.test.ts --bundle \
 *     --platform=node --format=cjs --loader:.css=empty \
 *     --outfile=/tmp/panels.test.cjs && node --test /tmp/panels.test.cjs
 */

import assert from 'node:assert/strict';
import test from 'node:test';

import * as React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

import {
  DetailsPanel,
  defaultOptionLabel,
  overrideSelection,
  overrideSetting,
} from '../DetailsPanel';
import { SettingsPanel, visibleContacts } from '../SettingsPanel';
import { initialState, reducer, type Conversation, type State } from '../state';
import type { User } from '../../preload';

/* -------------------------------------------------------------------------
 * Fixtures
 * ---------------------------------------------------------------------- */

function contact(overrides: Partial<User> = {}): User {
  return {
    id: 'user-1',
    name: 'Ada Lovelace',
    alias: '',
    images: [],
    blocked: false,
    accepted: true,
    introductionTime: 0,
    lastActivity: 0,
    lastOpened: 0,
    mutedUntil: 0,
    retention: 0,
    clearBefore: 0,
    openDm: true,
    notes: '',
    readReceiptsOverridden: false,
    readReceiptsEnabled: true,
    typingIndicatorsOverridden: false,
    typingIndicatorsEnabled: true,
    online: false,
    ...overrides,
  };
}

function conversationFor(user: User): Conversation {
  return {
    id: user.id,
    kind: 'direct',
    name: user.alias || user.name,
    memberCount: 0,
    lastActivity: 0,
    online: false,
    muted: false,
    invitationPending: false,
  };
}

/** The panel as it first paints for one contact. */
function renderPanel(user: User, state: Partial<State> = {}): string {
  const full: State = {
    ...initialState,
    profile: contact({ id: 'me', name: 'Grace Hopper' }),
    users: { [user.id]: user },
    ...state,
  };

  return renderToStaticMarkup(
    React.createElement(DetailsPanel, {
      conversation: conversationFor(user),
      state: full,
      onClose: () => {},
    }),
  );
}

/* -------------------------------------------------------------------------
 * Seeding
 * ---------------------------------------------------------------------- */

test('the notes box opens holding the stored note', () => {
  // The read path this had none of: the textarea seeded from a constant `''`,
  // and a blur wrote that constant back over the note.
  const markup = renderPanel(contact({ notes: 'met at the museum, plays cello' }));
  assert.match(markup, /met at the museum, plays cello/);
});

test('a contact with no note opens with an empty box', () => {
  const markup = renderPanel(contact({ notes: '' }));
  assert.match(markup, /<textarea[^>]*>\s*<\/textarea>/);
});

test("the retention select opens on the conversation's own retention", () => {
  // A week, set on another device. The select used to have no value to seed
  // from and reported "Off" — a claim about how long messages are kept that
  // was not merely stale but never true.
  const markup = renderPanel(contact({ retention: 7 * 24 * 60 * 60 }));
  assert.match(markup, /<option value="604800" selected="">1 week<\/option>/);
  assert.doesNotMatch(markup, /<option value="0" selected="">Off<\/option>/);
});

test('the overrides are offered, and behind a disclosure', () => {
  const markup = renderPanel(contact());
  // Go keeps both selects in an "Advanced Options" accordion rather than in
  // the body of the screen, so a conversation that overrides nothing offers
  // the heading and nothing else.
  assert.match(markup, /Advanced options/);
  assert.doesNotMatch(markup, /Read receipts/);
});

test('a conversation that overrides something opens showing what', () => {
  const markup = renderPanel(
    contact({
      readReceiptsOverridden: true,
      readReceiptsEnabled: false,
      typingIndicatorsOverridden: false,
      typingIndicatorsEnabled: true,
    }),
  );

  // Receipts are off for this contact alone; indicators follow the profile,
  // and the option that says so names what the profile currently does.
  assert.match(markup, /<option value="off" selected="">Off<\/option>/);
  assert.match(markup, /<option value="default" selected="">Default \(On\)<\/option>/);
  assert.doesNotMatch(markup, /<option value="on" selected="">/);
});

/* -------------------------------------------------------------------------
 * The tri-state
 * ---------------------------------------------------------------------- */

test('a conversation with no override follows the profile', () => {
  assert.equal(overrideSelection(false, true), 'default');
  // The value byte is still there when the override byte is clear, and reading
  // it would show "Off" for a conversation that follows an "On" default.
  assert.equal(overrideSelection(false, false), 'default');
});

test('an override reads as the value it overrode with', () => {
  assert.equal(overrideSelection(true, true), 'on');
  assert.equal(overrideSelection(true, false), 'off');
});

test('null is what the bridge is told for "follow the default"', () => {
  assert.equal(overrideSetting('default'), null);
  assert.equal(overrideSetting('on'), true);
  assert.equal(overrideSetting('off'), false);
});

test('every selection survives a round trip through the bridge shape', () => {
  // What the engine stores is the pair of bytes, so a selection has to come
  // back as itself after being written and read.
  for (const [overridden, enabled] of [
    [false, true],
    [false, false],
    [true, true],
    [true, false],
  ] as const) {
    const selection = overrideSelection(overridden, enabled);
    const setting = overrideSetting(selection);
    const stored: [boolean, boolean] = [setting !== null, setting ?? true];
    assert.equal(overrideSelection(stored[0], stored[1]), selection);
  }
});

test('the first option says which default it follows', () => {
  // Go's wording exactly (`ui/settings_container.go:13-16`); a bare "Default"
  // would make choosing it a guess.
  assert.equal(defaultOptionLabel(true), 'Default (On)');
  assert.equal(defaultOptionLabel(false), 'Default (Off)');
});

/* -------------------------------------------------------------------------
 * The contact list
 * ---------------------------------------------------------------------- */

test('the settings panel says a blocked contact is there to be found', () => {
  // The recovery route: blocking removes the conversation, so the count and
  // the toggle are what tell someone their contact still exists at all.
  const blocked = contact({ id: 'a', name: 'Ada Lovelace', blocked: true });
  const markup = renderToStaticMarkup(
    React.createElement(SettingsPanel, {
      state: {
        ...initialState,
        profile: contact({ id: 'me', name: 'Grace Hopper' }),
        users: { [blocked.id]: blocked },
      },
      onClose: () => {},
    }),
  );

  assert.match(markup, /Contacts/);
  assert.match(markup, /Show blocked/);
  assert.match(markup, /1 blocked contact is hidden/);
  // Hidden until asked for, as Go's "Show Blocked" checkbox has it.
  assert.doesNotMatch(markup, /Ada Lovelace/);
});

test('blocked contacts are hidden until they are asked for', () => {
  const users = [contact({ id: 'a', name: 'Ada' }), contact({ id: 'b', name: 'Blocked', blocked: true })];

  assert.deepEqual(
    visibleContacts(users, 'me', false).map((user) => user.id),
    ['a'],
  );
  // The whole point of the list: blocking is only reversible from a row that
  // blocking cannot remove.
  assert.deepEqual(
    visibleContacts(users, 'me', true).map((user) => user.id),
    ['a', 'b'],
  );
});

test('the profile itself is a conversation but not a contact', () => {
  const users = [contact({ id: 'me', name: 'Grace Hopper' }), contact({ id: 'a', name: 'Ada' })];
  assert.deepEqual(
    visibleContacts(users, 'me', true).map((user) => user.id),
    ['a'],
  );
});

test('contacts are ordered by the name actually shown', () => {
  // `b` sorts under its alias, not under "Ada": the list has to be searchable
  // by the name the rest of the interface uses for someone.
  const users = [
    contact({ id: 'a', name: 'Zoe' }),
    contact({ id: 'b', name: 'Ada', alias: 'Zebra' }),
    contact({ id: 'c', name: 'Mary' }),
  ];
  assert.deepEqual(
    visibleContacts(users, 'me', false).map((user) => user.id),
    ['c', 'b', 'a'],
  );
});

test('an update about ourselves lands on the profile, not in the contact list', () => {
  // The engine emits `userUpdated` for the profile's own record when the name
  // or picture changes. Routing that into `users` left `profile` stale, so a
  // new picture showed up in every avatar drawn from `users` and not in the
  // settings panel, which reads `profile` — until a reload re-fetched the
  // snapshot and hid the bug.
  const me = { ...contact(), id: 'me', name: 'Ada', images: [] as string[] };
  const start = reducer(initialState, {
    type: 'loaded',
    address: 'addr',
    state: {
      profile: me,
      networkOnline: true,
      deviceRevoked: false,
      syncDevices: [],
      users: [],
      groups: [],
      messages: [],
      systemMessages: [],
      drafts: [],
    },
  });

  const updated = reducer(start, {
    type: 'engineEvent',
    event: { type: 'userUpdated', user: { ...me, images: ['picture-1'] } },
  });

  assert.deepEqual(updated.profile?.images, ['picture-1'], 'the profile must see it');
  assert.equal(updated.users.me, undefined, 'and we must not become our own contact');
});
