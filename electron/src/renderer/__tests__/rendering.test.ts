/**
 * Tests for the pure parts of message rendering.
 *
 * These are the functions that decide what a reader sees when the input came
 * from somebody else's client, so they are worth pinning down away from React:
 * the link rules are a security boundary, the truncation rule has to agree
 * with the Go client, and the sentences have to agree with it word for word.
 *
 * Bundle and run:
 *
 *   npx esbuild src/renderer/__tests__/rendering.test.ts --bundle \
 *     --platform=node --format=cjs --loader:.css=empty \
 *     --outfile=/tmp/rendering.test.cjs && node --test /tmp/rendering.test.cjs
 */

import assert from 'node:assert/strict';
import test from 'node:test';

import {
  MAX_RUNES,
  safeHref,
  splitLinks,
  trimTrailingPunctuation,
  truncateRunes,
  type MessageSegment,
} from '../MessageText';
import {
  describeSystemMessage,
  type DisplayNames,
  type SystemMessageView,
} from '../SystemMessage';

/* -------------------------------------------------------------------------
 * Links
 * ---------------------------------------------------------------------- */

/** The href of the single link in a body, or null if nothing was linked. */
function onlyLink(text: string): MessageSegment | null {
  const links = splitLinks(text).filter((segment) => segment.kind === 'link');
  return links.length === 1 ? links[0] : null;
}

test('an http url becomes a link and the surrounding prose stays text', () => {
  assert.deepEqual(splitLinks('see http://example.com now'), [
    { kind: 'text', text: 'see ' },
    { kind: 'link', text: 'http://example.com', href: 'http://example.com/' },
    { kind: 'text', text: ' now' },
  ]);
});

test('a body with no url is one text segment', () => {
  assert.deepEqual(splitLinks('nothing to click here'), [
    { kind: 'text', text: 'nothing to click here' },
  ]);
});

test('an empty body produces no segments', () => {
  assert.deepEqual(splitLinks(''), []);
});

test('every scheme other than http and https is left as plain text', () => {
  // The whole point of the exercise: a stranger's message must not be able to
  // put a local file, a script url, or a custom protocol handler behind a
  // click. Each of these would be a working link if the scheme check were
  // dropped.
  for (const hostile of [
    'file:///etc/passwd',
    'javascript:alert(1)',
    'data:text/html;base64,PHNjcmlwdD4=',
    'vbscript:msgbox(1)',
    'chrome://settings',
    'mailto:someone@example.com',
    'ftp://example.com/pub',
    'bounce://join/abc',
  ]) {
    assert.equal(onlyLink(hostile), null, hostile);
    assert.deepEqual(splitLinks(hostile), [{ kind: 'text', text: hostile }]);
  }
});

test('a hostile scheme wrapped around an http url only links the http part', () => {
  const segments = splitLinks('javascript:https://example.com');
  assert.deepEqual(segments, [
    { kind: 'text', text: 'javascript:' },
    { kind: 'link', text: 'https://example.com', href: 'https://example.com/' },
  ]);
});

test('a scheme inside a longer word is not a link', () => {
  // Otherwise "nothttps://x" and "shttp://x" would both linkify, which reads
  // as the client endorsing a url the author never wrote.
  assert.equal(onlyLink('nothttps://example.com'), null);
  assert.equal(onlyLink('xhttp://example.com'), null);
});

test('the sentence punctuation after a link is not part of it', () => {
  assert.equal(trimTrailingPunctuation('https://example.com/a.'), 'https://example.com/a');
  assert.equal(trimTrailingPunctuation('https://example.com/a,'), 'https://example.com/a');
  assert.equal(trimTrailingPunctuation('https://example.com/a?!'), 'https://example.com/a');

  assert.deepEqual(splitLinks('go to https://example.com/a.'), [
    { kind: 'text', text: 'go to ' },
    { kind: 'link', text: 'https://example.com/a', href: 'https://example.com/a' },
    { kind: 'text', text: '.' },
  ]);
});

test('parentheses inside a url survive but an unmatched closer does not', () => {
  const inside = 'https://en.wikipedia.org/wiki/Ruby_(gemstone)';
  assert.equal(trimTrailingPunctuation(inside), inside);
  assert.equal(trimTrailingPunctuation(`${inside})`), inside);

  const link = onlyLink(`(see ${inside})`);
  assert.deepEqual(link, { kind: 'link', text: inside, href: inside });
});

test('a query string is kept whole', () => {
  const url = 'https://example.com/search?q=a+b&page=2#top';
  assert.deepEqual(onlyLink(`look: ${url}`), { kind: 'link', text: url, href: url });
});

test('several links in one message are all found', () => {
  const segments = splitLinks('a https://one.example b http://two.example c');
  assert.equal(segments.filter((segment) => segment.kind === 'link').length, 2);
});

test('newlines around a link are preserved', () => {
  // The timeline renders with pre-wrap, so losing the newline here would
  // silently reflow somebody's carefully formatted message.
  assert.deepEqual(splitLinks('line one\nhttps://example.com\nline three'), [
    { kind: 'text', text: 'line one\n' },
    { kind: 'link', text: 'https://example.com', href: 'https://example.com/' },
    { kind: 'text', text: '\nline three' },
  ]);
});

test('a scheme with nothing after it is not a link', () => {
  assert.equal(safeHref('https://'), null);
  assert.equal(onlyLink('https://'), null);
});

test('safeHref accepts only http and https', () => {
  assert.equal(safeHref('http://example.com'), 'http://example.com/');
  assert.equal(safeHref('https://example.com/a'), 'https://example.com/a');
  assert.equal(safeHref('javascript:alert(1)'), null);
  assert.equal(safeHref('file:///etc/passwd'), null);
  assert.equal(safeHref('not a url at all'), null);
});

/* -------------------------------------------------------------------------
 * Truncation
 * ---------------------------------------------------------------------- */

test('a short message is returned untouched', () => {
  assert.deepEqual(truncateRunes('hello'), { body: 'hello', truncated: false });
});

test('a message of exactly the limit is not truncated', () => {
  // An off-by-one here shows a "Read more" toggle that reveals nothing.
  const text = 'a'.repeat(MAX_RUNES);
  assert.deepEqual(truncateRunes(text), { body: text, truncated: false });
});

test('a message one over the limit is truncated to the limit', () => {
  const text = 'a'.repeat(MAX_RUNES + 1);
  assert.deepEqual(truncateRunes(text), { body: 'a'.repeat(MAX_RUNES), truncated: true });
});

test('truncation counts scalar values so emoji are never cut in half', () => {
  // Every one of these is a surrogate pair, so a UTF-16 slice at the limit
  // would land inside the last one and leave a lone surrogate on screen.
  const text = '😀'.repeat(MAX_RUNES + 10);
  const { body, truncated } = truncateRunes(text);

  assert.equal(truncated, true);
  assert.equal([...body].length, MAX_RUNES);
  assert.equal(body, '😀'.repeat(MAX_RUNES));

  // The first mistake this guards against: a UTF-16 slice at the same limit
  // keeps only half the characters, because each one is two code units.
  assert.equal([...text.slice(0, MAX_RUNES)].length, MAX_RUNES / 2);
});

test('truncation never leaves a lone surrogate behind', () => {
  // The second mistake, shown at an odd limit where a UTF-16 slice lands
  // inside a surrogate pair and renders as a replacement character.
  const loneSurrogate = /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/;
  const text = '😀'.repeat(5);

  assert.equal(truncateRunes(text, 3).body, '😀'.repeat(3));
  assert.ok(!loneSurrogate.test(truncateRunes(text, 3).body), 'left a lone surrogate');
  assert.ok(loneSurrogate.test(text.slice(0, 3)), 'the naive slice was safe after all');
});

test('the limit is counted in scalar values, not bytes', () => {
  // Three bytes each in UTF-8; counting bytes would cut at a third of the
  // message.
  const text = '日'.repeat(MAX_RUNES);
  assert.equal(truncateRunes(text).truncated, false);
});

/* -------------------------------------------------------------------------
 * System sentences
 * ---------------------------------------------------------------------- */

const ADA = '11111111-1111-1111-1111-111111111111';
const GRACE = '22222222-2222-2222-2222-222222222222';
const ME = '33333333-3333-3333-3333-333333333333';
const THREAD = '44444444-4444-4444-4444-444444444444';

const NAMES: DisplayNames = {
  [ADA]: 'Ada',
  [GRACE]: 'Grace',
  [ME]: 'You',
};

function change(kind: string, extra: Partial<SystemMessageView> = {}): SystemMessageView {
  return {
    id: 'row',
    thread: THREAD,
    actor: ADA,
    kind,
    timestamp: 1_700_000_000,
    ...extra,
  };
}

function describe(kind: string, extra: Partial<SystemMessageView> = {}): string {
  return describeSystemMessage(change(kind, extra), NAMES);
}

test('each fixed sentence matches the Fyne clients wording', () => {
  // These strings are the contract with ui/thread_item.go. If one of them has
  // to change, it changes in both clients or the two disagree about history.
  assert.equal(describe('groupCreated'), 'Ada created the group');
  assert.equal(describe('groupImageChanged'), 'Ada changed the group image');
  assert.equal(describe('userLeft'), 'Ada left the group');
  assert.equal(describe('inviteAccepted'), 'Ada accepted the invite');
  assert.equal(describe('inviteRejected'), 'Ada rejected the invite');
  assert.equal(describe('userManagementRestricted'), 'Ada restricted user management');
  assert.equal(describe('userManagementUnrestricted'), 'Ada unrestricted user management');
  assert.equal(describe('groupEditsRestricted'), 'Ada restricted group edits');
  assert.equal(describe('groupEditsUnrestricted'), 'Ada unrestricted group edits');
  assert.equal(describe('postingRestricted'), 'Ada restricted posting');
  assert.equal(describe('postingUnrestricted'), 'Ada unrestricted posting');
  assert.equal(describe('groupBlocked'), 'Ada blocked the group');
  assert.equal(describe('historyCleared'), 'Ada cleared the chat history');
});

test('the sentences that carry a value read the value back', () => {
  assert.equal(
    describe('groupRenamed', { value: 'Lunch plans' }),
    'Ada changed the group name to Lunch plans',
  );
  assert.equal(
    describe('retentionChanged', { value: '1 week' }),
    'Ada changed the message retention to 1 week',
  );
});

test('the sentences about another user name that user', () => {
  assert.equal(describe('userInvited', { subject: GRACE }), 'Ada invited Grace to the group');
  assert.equal(describe('userRemoved', { subject: GRACE }), 'Ada removed Grace from the group');
  assert.equal(describe('adminPromoted', { subject: GRACE }), 'Ada made Grace an admin');
  assert.equal(describe('adminDemoted', { subject: GRACE }), 'Ada removed Grace as an admin');
  assert.equal(
    describe('inviteRevoked', { subject: GRACE }),
    'Ada removed the invite for Grace',
  );
});

test('an invitee we have no record of is named by the text the core supplied', () => {
  // An invitation can name somebody this device has never seen, which is why
  // the subject falls through to a literal rather than becoming "Someone".
  assert.equal(
    describe('userInvited', { subject: 'Hopper' }),
    'Ada invited Hopper to the group',
  );
});

test('a user the name table does not know is Someone', () => {
  const stranger = '55555555-5555-5555-5555-555555555555';
  assert.equal(
    describeSystemMessage(change('groupCreated', { actor: stranger }), NAMES),
    'Someone created the group',
  );
});

test('the local user is You in every sentence', () => {
  assert.equal(describe('groupCreated', { actor: ME }), 'You created the group');
  assert.equal(
    describe('userInvited', { actor: ME, subject: GRACE }),
    'You invited Grace to the group',
  );
  assert.equal(describe('historyCleared', { actor: ME }), 'You cleared the chat history');
});

test('removing yourself reads as leaving', () => {
  // The core has a kind of its own for this, but a removal whose subject is
  // its actor is the same event, and "Ada removed Ada from the group" is not
  // a sentence anyone should have to read.
  assert.equal(describe('userRemoved', { subject: ADA }), 'Ada left the group');
  assert.equal(describe('userRemoved', { actor: ME, subject: ME }), 'You left the group');
});

test('a rename is phrased around the name the user had before', () => {
  assert.equal(
    describe('userRenamed', { subject: 'Ada L', value: 'Ada Lovelace' }),
    'Ada L changed their name to Ada Lovelace',
  );
  assert.equal(
    describe('userRenamed', { actor: ME, value: 'Hayden' }),
    'You changed your name to Hayden',
  );
});

test('a rename with no previous name falls back to the current one', () => {
  assert.equal(
    describe('userRenamed', { value: 'Ada Lovelace' }),
    'Ada changed their name to Ada Lovelace',
  );
});

test('a profile image change is phrased around the user, not prefixed', () => {
  assert.equal(describe('userImageChanged'), 'Ada changed their profile image');
  assert.equal(describe('userImageChanged', { actor: ME }), 'You changed your profile image');
});

test('an unrecognised kind still produces a sentence', () => {
  // A core newer than this build will send kinds it has never heard of, and a
  // vague row is better than a blank one or a thrown error.
  assert.equal(describe('somethingNewerThanThisBuild'), 'Ada updated the conversation');
});
