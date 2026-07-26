/**
 * Emoji lookup, search, and `:shortcode:` completion.
 *
 * The cases that matter are the ones where a colon is *not* the start of an
 * emoji name — a timestamp, a URL, a bare colon at the end of a sentence — and
 * the ones where the generated table has to agree with Signal about what a
 * given name means. Both are silent failures otherwise: the first inserts an
 * emoji nobody asked for, the second inserts the wrong one.
 */

import { strict as assert } from 'node:assert';
import { test } from 'node:test';

import {
  completeShortcodeAtCaret,
  EMOJI,
  EMOJI_CATEGORIES,
  emojiForShortcode,
  findShortcodeQuery,
  insertEmoji,
  MIN_QUERY_LENGTH,
  searchEmoji,
} from '../emoji';

/* --------------------------------------------------------------------------
 * The table
 * -------------------------------------------------------------------------- */

test('the table is populated and internally consistent', () => {
  assert.ok(EMOJI.length > 1500, `only ${EMOJI.length} emoji`);

  for (const emoji of EMOJI) {
    assert.ok(emoji.char.length > 0, `${emoji.label} has no character`);
    assert.ok(emoji.label.length > 0, 'an emoji has no label');
    assert.ok(emoji.shortcodes.length > 0, `${emoji.label} has no shortcode`);
    assert.ok(
      emoji.category >= 0 && emoji.category < EMOJI_CATEGORIES.length,
      `${emoji.label} is in category ${emoji.category}`,
    );
  }
});

test('every shortcode names exactly one emoji', () => {
  // Two emoji claiming `:train:` would make what you get depend on iteration
  // order, which is the kind of thing that changes under a data refresh and is
  // never noticed.
  const owners = new Map<string, string>();

  for (const emoji of EMOJI) {
    for (const shortcode of emoji.shortcodes) {
      const existing = owners.get(shortcode);
      assert.equal(
        existing,
        undefined,
        `:${shortcode}: is claimed by both ${existing} and ${emoji.char}`,
      );
      owners.set(shortcode, emoji.char);
    }
  }
});

test('shortcodes agree with Signal on the names people actually type', () => {
  // Signal is built on emoji-datasource, whose naming the generator takes as
  // authoritative. These are the ones where the other sets disagree, so a
  // change in precedence would show up here first.
  const expected: [string, string][] = [
    ['smile', '😄'],
    ['+1', '👍️'],
    ['thumbsup', '👍️'],
    ['heart', '❤️'],
    ['joy', '😂'],
    ['fire', '🔥'],
    ['tada', '🎉'],
    ['rocket', '🚀'],
    ['100', '💯'],
    ['train', '🚋'],
    ['train2', '🚆'],
    ['point_up', '☝️'],
    ['point_up_2', '👆️'],
    ['dog', '🐶'],
  ];

  for (const [shortcode, character] of expected) {
    assert.equal(emojiForShortcode(shortcode)?.char, character, `:${shortcode}:`);
  }
});

test('shortcode lookup is case insensitive and rejects unknown names', () => {
  assert.equal(emojiForShortcode('SMILE')?.char, '😄');
  assert.equal(emojiForShortcode('not_an_emoji_name'), undefined);
  assert.equal(emojiForShortcode(''), undefined);
});

/* --------------------------------------------------------------------------
 * Search
 * -------------------------------------------------------------------------- */

test('search puts an exact shortcode first', () => {
  assert.equal(searchEmoji('fire')[0].char, '🔥');
  assert.equal(searchEmoji('heart')[0].char, '❤️');
});

test('search matches prefixes, labels and keywords', () => {
  const prefix = searchEmoji('grinn').map((emoji) => emoji.char);
  assert.ok(prefix.includes('😀'), 'grinning face is missing');

  // "pizza" is the label, not a shortcode prefix of anything else.
  assert.equal(searchEmoji('pizza')[0].char, '🍕');

  // "lmao" is a keyword on 😂 and appears in no label.
  assert.ok(searchEmoji('lmao').some((emoji) => emoji.char === '😂'));
});

test('the separator between words does not matter', () => {
  // "thumbs up" is the label, "thumbsup" a shortcode, and neither spelling
  // should be the one you had to guess.
  for (const query of ['thumbs up', 'thumbs_up', 'thumbs-up', 'thumbsup']) {
    assert.ok(
      searchEmoji(query).some((emoji) => emoji.char === '👍️'),
      `"${query}" did not find it`,
    );
  }
});

test('search honours the limit and returns nothing for an empty query', () => {
  assert.equal(searchEmoji('face', 5).length, 5);
  assert.deepEqual(searchEmoji(''), []);
  assert.deepEqual(searchEmoji('   '), []);
  assert.deepEqual(searchEmoji('zzzzzznotathing'), []);
});

/* --------------------------------------------------------------------------
 * Locating a shortcode in progress
 * -------------------------------------------------------------------------- */

test('a shortcode is found once enough of it has been typed', () => {
  const text = 'hello :sm';
  const found = findShortcodeQuery(text, text.length);
  assert.deepEqual(found, { start: 6, end: 9, query: 'sm' });
});

test('one character is not enough to suggest on', () => {
  assert.equal(MIN_QUERY_LENGTH, 2);
  assert.equal(findShortcodeQuery('hi :s', 5), null);
  assert.equal(findShortcodeQuery('hi :', 4), null);
});

test('the colon has to start a word', () => {
  // The cases this rule exists for: a timestamp and a URL both contain a colon
  // followed by characters that look exactly like an emoji name.
  assert.equal(findShortcodeQuery('meet at 10:30', 13), null);
  assert.equal(findShortcodeQuery('https://example.com', 19), null);
  assert.equal(findShortcodeQuery('ratio 3:45', 10), null);

  // But one at the very start of the box is fine.
  assert.deepEqual(findShortcodeQuery(':sm', 3), { start: 0, end: 3, query: 'sm' });

  // As is one after a newline.
  assert.deepEqual(findShortcodeQuery('one\n:sm', 7), { start: 4, end: 7, query: 'sm' });
});

test('a space ends the search, so only the current word is considered', () => {
  assert.equal(findShortcodeQuery(':smile is nice', 14), null);
});

test('the caret has to be at the end of the name', () => {
  // Caret parked back at the colon: there is nothing before it to complete.
  assert.equal(findShortcodeQuery('hello :smile', 7), null);

  // Caret in the middle completes only what is to its left.
  assert.deepEqual(findShortcodeQuery('hello :smile', 10), {
    start: 6,
    end: 10,
    query: 'smi',
  });
});

/* --------------------------------------------------------------------------
 * Completion
 * -------------------------------------------------------------------------- */

test('a closing colon on a known name replaces it', () => {
  const text = 'well done :tada:';
  const edit = completeShortcodeAtCaret(text, text.length);
  assert.deepEqual(edit, { text: 'well done 🎉', caret: 'well done 🎉'.length });
});

test('completion works mid-message and leaves the tail alone', () => {
  const text = 'a :fire: b';
  // Caret just past the closing colon, not at the end of the text.
  const edit = completeShortcodeAtCaret(text, 8);
  // The caret lands past the emoji, which is two UTF-16 units wide.
  assert.deepEqual(edit, { text: 'a 🔥 b', caret: 2 + '🔥'.length });
});

test('an unknown name is left as typed', () => {
  assert.equal(completeShortcodeAtCaret('hi :nope_not_real:', 18), null);
});

test('completion never fires without a closing colon', () => {
  assert.equal(completeShortcodeAtCaret('hi :tada', 8), null);
});

test('completion ignores a colon that does not start a word', () => {
  assert.equal(completeShortcodeAtCaret('at 10:30:', 9), null);
});

/* --------------------------------------------------------------------------
 * Insertion
 * -------------------------------------------------------------------------- */

test('inserting at the caret adds a trailing space', () => {
  const fire = emojiForShortcode('fire')!;
  const edit = insertEmoji('hey', 3, fire);
  assert.deepEqual(edit, { text: 'hey🔥 ', caret: 'hey🔥 '.length });
  assert.equal(edit.text.slice(edit.caret), '');
});

test('inserting over a shortcode in progress replaces the whole thing', () => {
  const text = 'hello :sm';
  const query = findShortcodeQuery(text, text.length)!;
  const smile = emojiForShortcode('smile')!;

  assert.deepEqual(insertEmoji(text, text.length, smile, { replace: query }), {
    text: 'hello 😄 ',
    caret: 'hello 😄 '.length,
  });
});

test('inserting mid-text keeps what comes after it', () => {
  const fire = emojiForShortcode('fire')!;
  const edit = insertEmoji('ab', 1, fire);
  assert.equal(edit.text, 'a🔥 b');
  assert.equal(edit.text.slice(edit.caret), 'b');
});
