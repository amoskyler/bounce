/**
 * Tests for the two lists the shell keeps: the conversations that are open and
 * the contacts who exist.
 *
 * Collapsing the pair is the failure these pin down. Go adds every user to
 * `ui.users` and only builds a thread when `State.Open` is set
 * (`ui/ui.go:459-465`), so a twenty-person group costs twenty contacts and no
 * conversations; the port used to produce twenty sidebar rows and no route
 * back to anybody it was not showing. The ordering rule is here too, because
 * a draft has to hold its thread in place (`ui/thread.go:47-62`) or the
 * "Draft:" prefix ends up at the bottom of the list where nobody looks.
 *
 * Bundle and run:
 *
 *   npx esbuild src/renderer/__tests__/contacts.test.ts --bundle \
 *     --platform=node --format=cjs --loader:.css=empty \
 *     --outfile=/tmp/contacts.test.cjs && node --test /tmp/contacts.test.cjs
 */

import assert from 'node:assert/strict';
import test from 'node:test';

import {
  contacts,
  conversations,
  filterContacts,
  initialState,
  reducer,
  type State,
} from '../state';
import type { Group, InitialState, Message, User } from '../../preload';

/* -------------------------------------------------------------------------
 * Fixtures
 * ---------------------------------------------------------------------- */

const ME = 'me';

function user(id: string, overrides: Partial<User> = {}): User {
  return {
    id,
    name: id,
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
    // False is the interesting default: it is what a user met through a group
    // carries (`chat/user.go:395-402`, `frames/identity.rs:285`).
    openDm: false,
    notes: '',
    readReceiptsOverridden: false,
    readReceiptsEnabled: true,
    typingIndicatorsOverridden: false,
    typingIndicatorsEnabled: true,
    online: false,
    ...overrides,
  };
}

function group(id: string, overrides: Partial<Group> = {}): Group {
  return {
    id,
    name: id,
    images: [],
    members: [ME],
    admins: [ME],
    invites: [],
    createdBy: ME,
    createdAt: 0,
    lastActivity: 0,
    lastOpened: 0,
    mutedUntil: 0,
    retention: 0,
    restrictPosting: false,
    restrictGroupEdits: false,
    restrictUserManagement: false,
    ...overrides,
  };
}

function message(thread: string, writtenAt: number): Message {
  return {
    id: `${thread}-${writtenAt}`,
    thread,
    author: thread,
    text: 'hello',
    writtenAt,
    expiresAt: 0,
    seen: true,
    undeliverable: false,
    deliveredTo: [],
    readBy: [],
    attachments: [],
    outgoing: false,
  };
}

/** A loaded state holding the given users and groups. */
function stateWith(overrides: Partial<State> = {}): State {
  return {
    ...initialState,
    loaded: true,
    profile: user(ME, { name: 'Me' }),
    ...overrides,
  };
}

function names(list: Array<{ name: string }>): string[] {
  return list.map((entry) => entry.name);
}

function ids(list: Array<{ id: string }>): string[] {
  return list.map((entry) => entry.id);
}

/* -------------------------------------------------------------------------
 * The sidebar holds open conversations only
 * ---------------------------------------------------------------------- */

test('a contact met through a group is not a conversation', () => {
  const state = stateWith({
    users: { ada: user('ada'), grace: user('grace'), alan: user('alan') },
  });

  // Only the note-to-self row, which is always there.
  assert.deepEqual(ids(conversations(state)), [ME]);
});

test('a contact with an open conversation gets a sidebar row', () => {
  const state = stateWith({
    users: { ada: user('ada', { openDm: true }), grace: user('grace') },
  });

  assert.deepEqual(ids(conversations(state)).sort(), ['ada', ME].sort());
});

test('a closed conversation leaves the sidebar even if it has history', () => {
  // This used to assert the opposite. History outranked the flag because
  // `open_dm` had no producer and every contact carried a false one — but that
  // made closing a no-op for anyone you had ever messaged, which is the entire
  // point of closing. A one-time migration now opens everyone with history,
  // and the engine reopens a conversation when a message arrives, so a false
  // flag here is a deliberate choice rather than a missing value.
  const state = stateWith({
    users: { ada: user('ada', { openDm: false }) },
    messagesByThread: { ada: [message('ada', 100)] },
  });

  assert.deepEqual(ids(conversations(state)), [ME]);
});

test('an open conversation is a sidebar row whether or not it has history', () => {
  const withHistory = stateWith({
    users: { ada: user('ada', { openDm: true }) },
    messagesByThread: { ada: [message('ada', 100)] },
  });
  const empty = stateWith({ users: { bo: user('bo', { openDm: true }) } });

  assert.deepEqual(ids(conversations(withHistory)).sort(), ['ada', ME].sort());
  assert.deepEqual(ids(conversations(empty)).sort(), ['bo', ME].sort());
});

test('a blocked contact is never a sidebar row, even with history', () => {
  const state = stateWith({
    users: { ada: user('ada', { blocked: true, openDm: true }) },
    messagesByThread: { ada: [message('ada', 100)] },
  });

  assert.deepEqual(ids(conversations(state)), [ME]);
});

test('the profile never yields a second row of its own', () => {
  // A user table that happens to carry this profile must not produce a row
  // beside the note-to-self one — same key, same list.
  const state = stateWith({
    users: { [ME]: user(ME, { openDm: true }) },
  });

  assert.deepEqual(ids(conversations(state)), [ME]);
});

test('groups are listed regardless of any open flag', () => {
  const state = stateWith({
    users: { ada: user('ada') },
    groups: { walkers: group('walkers', { name: 'Walkers' }) },
  });

  assert.deepEqual(ids(conversations(state)).sort(), ['walkers', ME].sort());
});

/* -------------------------------------------------------------------------
 * The contact store holds everybody
 * ---------------------------------------------------------------------- */

test('contacts lists people the sidebar does not', () => {
  const state = stateWith({
    users: { ada: user('ada'), grace: user('grace', { openDm: true }) },
  });

  assert.deepEqual(names(contacts(state)), ['ada', 'grace']);
  assert.deepEqual(ids(conversations(state)).sort(), ['grace', ME].sort());
});

test('contacts are name-ordered and use the alias when there is one', () => {
  const state = stateWith({
    users: {
      zoe: user('zoe'),
      ada: user('ada', { alias: 'Ada L' }),
      grace: user('grace'),
    },
  });

  assert.deepEqual(names(contacts(state)), ['Ada L', 'grace', 'zoe']);
});

test('contacts never include the profile itself', () => {
  // Inviting yourself to a new group costs a signed frame and a status line
  // reading "invited you to the group"; this is the picker's source.
  const state = stateWith({
    users: { [ME]: user(ME, { name: 'Me' }), ada: user('ada') },
  });

  assert.deepEqual(names(contacts(state)), ['ada']);
});

test('blocked contacts are hidden until asked for', () => {
  const state = stateWith({
    users: { ada: user('ada'), mallory: user('mallory', { blocked: true }) },
  });

  assert.deepEqual(names(contacts(state)), ['ada']);
  assert.deepEqual(names(contacts(state, true)), ['ada', 'mallory']);
});

test('a contact knows whether the sidebar is showing it', () => {
  const state = stateWith({
    users: {
      open: user('open', { openDm: true }),
      shut: user('shut', { openDm: false }),
      chatty: user('chatty', { openDm: true }),
    },
    messagesByThread: { chatty: [message('chatty', 10)] },
  });

  const byId = new Map(contacts(state).map((entry) => [entry.id, entry]));
  assert.equal(byId.get('open')?.open, true);
  assert.equal(byId.get('shut')?.open, false);
  assert.equal(byId.get('chatty')?.open, true);

  // Hiding is offered wherever the row is showing. It used to be withheld from
  // a conversation with history, because history pinned it to the sidebar and
  // the control would have done nothing; the flag decides now, so closing one
  // with messages in it works like any other.
  assert.equal(byId.get('open')?.hideable, true);
  assert.equal(byId.get('shut')?.hideable, false);
  assert.equal(byId.get('chatty')?.hideable, true);
});

test('the contact search matches on the displayed name', () => {
  const list = contacts(
    stateWith({
      users: { ada: user('ada', { alias: 'Ada Lovelace' }), grace: user('grace') },
    }),
  );

  assert.deepEqual(names(filterContacts(list, 'love')), ['Ada Lovelace']);
  assert.deepEqual(names(filterContacts(list, '  ')), ['Ada Lovelace', 'grace']);
  assert.deepEqual(names(filterContacts(list, 'nobody')), []);
});

/* -------------------------------------------------------------------------
 * A draft holds its thread in place
 * ---------------------------------------------------------------------- */

test('a draft pins a quiet conversation to when it was last opened', () => {
  const state = stateWith({
    users: {
      quiet: user('quiet', { openDm: true, lastOpened: 500 }),
      busy: user('busy', { openDm: true }),
    },
    messagesByThread: {
      quiet: [message('quiet', 100)],
      busy: [message('busy', 400)],
    },
    drafts: { quiet: 'half a reply' },
  });

  // Without the rule `quiet` sorts on 100 and sinks below `busy`, displaying
  // "Draft:" from the bottom of the list.
  assert.deepEqual(ids(conversations(state)).slice(0, 2), ['quiet', 'busy']);
});

test('an empty draft does not pin anything', () => {
  const state = stateWith({
    users: {
      quiet: user('quiet', { openDm: true, lastOpened: 500 }),
      busy: user('busy', { openDm: true }),
    },
    messagesByThread: {
      quiet: [message('quiet', 100)],
      busy: [message('busy', 400)],
    },
    drafts: { quiet: '' },
  });

  assert.deepEqual(ids(conversations(state)).slice(0, 2), ['busy', 'quiet']);
});

test('last-opened alone does not promote a thread', () => {
  // Go only reaches for the open time when there is a draft; otherwise reading
  // a conversation would reshuffle the list under the reader.
  const state = stateWith({
    users: {
      quiet: user('quiet', { openDm: true, lastOpened: 500 }),
      busy: user('busy', { openDm: true }),
    },
    messagesByThread: {
      quiet: [message('quiet', 100)],
      busy: [message('busy', 400)],
    },
  });

  assert.deepEqual(ids(conversations(state)).slice(0, 2), ['busy', 'quiet']);
});

test('a draft never drags a thread backwards', () => {
  // `lastOpened` older than the newest message must leave the order alone.
  const state = stateWith({
    users: {
      recent: user('recent', { openDm: true, lastOpened: 50 }),
      older: user('older', { openDm: true }),
    },
    messagesByThread: {
      recent: [message('recent', 400)],
      older: [message('older', 100)],
    },
    drafts: { recent: 'still typing' },
  });

  assert.deepEqual(ids(conversations(state)).slice(0, 2), ['recent', 'older']);
});

test('a group with a draft is pinned the same way', () => {
  const state = stateWith({
    groups: {
      walkers: group('walkers', { lastOpened: 500 }),
      cooks: group('cooks'),
    },
    messagesByThread: {
      walkers: [message('walkers', 100)],
      cooks: [message('cooks', 400)],
    },
    drafts: { walkers: 'where are we meeting' },
  });

  assert.deepEqual(ids(conversations(state)).slice(0, 2), ['walkers', 'cooks']);
});

/* -------------------------------------------------------------------------
 * Network and revocation reach the reducer
 * ---------------------------------------------------------------------- */

function snapshot(overrides: Partial<InitialState> = {}): InitialState {
  return {
    profile: user(ME, { name: 'Me' }),
    networkOnline: false,
    deviceRevoked: false,
    syncDevices: [],
    users: [],
    groups: [],
    messages: [],
    systemMessages: [],
    drafts: [],
    ...overrides,
  };
}

test('the shell opens on the starting state rather than on a lost connection', () => {
  assert.equal(initialState.networkStarting, true);
  assert.equal(initialState.networkOnline, false);

  // A snapshot that cannot yet say we are online leaves us starting: nothing
  // has been lost, so "reconnecting" would be a lie.
  const loading = reducer(initialState, {
    type: 'loaded',
    state: snapshot(),
    address: 'abc.onion',
  });
  assert.equal(loading.networkStarting, true);
});

test('a snapshot that is already online ends the starting state', () => {
  const loaded = reducer(initialState, {
    type: 'loaded',
    state: snapshot({ networkOnline: true }),
    address: 'abc.onion',
  });

  assert.equal(loaded.networkOnline, true);
  assert.equal(loaded.networkStarting, false);
});

test('an offline event turns starting into lost, and it never goes back', () => {
  const offline = reducer(initialState, {
    type: 'engineEvent',
    event: { type: 'networkOffline' },
  });
  assert.equal(offline.networkOnline, false);
  assert.equal(offline.networkStarting, false);

  const online = reducer(offline, {
    type: 'engineEvent',
    event: { type: 'networkOnline' },
  });
  assert.equal(online.networkOnline, true);
  assert.equal(online.networkStarting, false);
});

test('revocation survives the snapshot', () => {
  // It used to be dropped: the reducer copied eight fields and not this one.
  const loaded = reducer(initialState, {
    type: 'loaded',
    state: snapshot({ deviceRevoked: true }),
    address: 'abc.onion',
  });

  assert.equal(loaded.deviceRevoked, true);
});
