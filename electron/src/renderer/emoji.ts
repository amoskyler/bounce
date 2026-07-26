/**
 * Emoji lookup, search, and `:shortcode:` completion.
 *
 * Kept apart from the components that use it because the interesting parts are
 * all decisions about text — where a shortcode starts, what counts as a match,
 * what the caret should do afterwards — and those are worth testing without a
 * DOM in the way.
 *
 * The behaviour follows Signal's: a shortcode has to start a word, two
 * characters are needed before anything is suggested, and typing the closing
 * colon on a name that exists replaces it there and then.
 */

import { EMOJI, type Emoji } from './emoji-data';

export { EMOJI, EMOJI_CATEGORIES, type Emoji } from './emoji-data';

/**
 * Characters allowed in a shortcode.
 *
 * `+` and `-` are in here for `:+1:` and `:-1:`, which are the two that would
 * be noticed immediately if they stopped working.
 */
const SHORTCODE_CHARACTERS = /^[a-z0-9_+-]*$/;

/**
 * How much of a name has to be typed before suggestions appear.
 *
 * One character matches several hundred emoji, which is a list nobody reads;
 * it also means a lone `:` at the end of a sentence opens a popup.
 */
export const MIN_QUERY_LENGTH = 2;

const byShortcode = new Map<string, Emoji>();
for (const emoji of EMOJI) {
  for (const shortcode of emoji.shortcodes) {
    // The generator guarantees these are unique, so first-wins is a belt-and-
    // braces guard rather than a real tie-break.
    if (!byShortcode.has(shortcode)) byShortcode.set(shortcode, emoji);
  }
}

/** The emoji a bare shortcode names, or undefined. */
export function emojiForShortcode(shortcode: string): Emoji | undefined {
  return byShortcode.get(shortcode.toLowerCase());
}

/**
 * Flatten the ways the same name gets written.
 *
 * Shortcodes join their words with underscores and labels use spaces, so
 * "thumbs up" and "thumbs_up" and "thumbs-up" are three spellings of one
 * search. Reducing all of them to the same thing means the box does not care
 * which one you reach for.
 */
function canonical(value: string): string {
  return value.toLowerCase().replace(/[\s_-]+/g, ' ').trim();
}

/** The canonicalised text each emoji is searched against, built once. */
const searchIndex = EMOJI.map((emoji) => ({
  shortcodes: emoji.shortcodes.map(canonical),
  label: canonical(emoji.label),
  keywords: emoji.keywords.map(canonical),
}));

/**
 * Rank an emoji against a query, lower being better, or null for no match.
 *
 * The tiers matter more than they look. Searching "he" should offer ❤️ before
 * 🚁 — "heart" starts with it, "helicopter" does too — and within a tier the
 * order falls back to Unicode's own, which puts the common ones first because
 * that is roughly how Unicode itself is arranged.
 */
function rank(entry: (typeof searchIndex)[number], query: string): number | null {
  let best: number | null = null;

  const consider = (score: number) => {
    if (best === null || score < best) best = score;
  };

  for (const shortcode of entry.shortcodes) {
    if (shortcode === query) return 0;
    if (shortcode.startsWith(query)) consider(1);
    else if (shortcode.includes(query)) consider(3);
  }

  if (entry.label === query) consider(1);
  else if (entry.label.startsWith(query)) consider(2);
  else if (entry.label.includes(query)) consider(4);

  for (const keyword of entry.keywords) {
    if (keyword === query) consider(3);
    else if (keyword.startsWith(query)) consider(5);
  }

  return best;
}

/**
 * Emoji matching a free-text query, best first.
 *
 * Used by both the picker's search box and the typeahead, so that the same
 * text always offers the same thing in the same order.
 */
export function searchEmoji(query: string, limit = Number.MAX_SAFE_INTEGER): Emoji[] {
  const needle = canonical(query);
  if (!needle) return [];

  const scored: { emoji: Emoji; score: number; index: number }[] = [];

  searchIndex.forEach((entry, index) => {
    const score = rank(entry, needle);
    if (score !== null) scored.push({ emoji: EMOJI[index], score, index });
  });

  scored.sort((a, b) => a.score - b.score || a.index - b.index);
  return scored.slice(0, limit).map((entry) => entry.emoji);
}

/** A `:name` being typed, located in the text. */
export interface ShortcodeQuery {
  /** Index of the opening colon. */
  start: number;
  /** Index just past the last character typed — always the caret. */
  end: number;
  /** What has been typed after the colon, lower-cased. */
  query: string;
}

/**
 * Find the shortcode being typed immediately before the caret.
 *
 * The colon has to open a word. Without that rule a timestamp — `10:30` — and
 * a URL — `https://` — would both open a suggestion list mid-sentence, and
 * emoji names are common enough words that the suggestions would look
 * deliberate.
 */
export function findShortcodeQuery(text: string, caret: number): ShortcodeQuery | null {
  // Scan back from the caret to the colon, refusing anything that cannot be
  // part of a name. This stops at the first space, so only the current word is
  // ever considered.
  let index = caret;
  while (index > 0) {
    const character = text[index - 1];
    if (character === ':') break;
    if (!SHORTCODE_CHARACTERS.test(character.toLowerCase())) return null;
    index -= 1;
  }

  if (index === 0) return null;

  const start = index - 1;
  const before = start === 0 ? '' : text[start - 1];
  if (before && !/\s/.test(before)) return null;

  const query = text.slice(index, caret).toLowerCase();
  if (query.length < MIN_QUERY_LENGTH) return null;

  return { start, end: caret, query };
}

/** The result of editing text: the new value and where the caret belongs. */
export interface TextEdit {
  text: string;
  caret: number;
}

/**
 * Replace the `:name:` that ends at the caret, if it names something.
 *
 * Called after each keystroke rather than only on `:` so that it also fires
 * when the closing colon arrives by paste, and returns null whenever there is
 * nothing to do, which is almost always.
 */
export function completeShortcodeAtCaret(text: string, caret: number): TextEdit | null {
  if (text[caret - 1] !== ':') return null;

  const inner = findShortcodeQuery(text, caret - 1);
  if (!inner) return null;

  const emoji = emojiForShortcode(inner.query);
  if (!emoji) return null;

  return {
    text: text.slice(0, inner.start) + emoji.char + text.slice(caret),
    caret: inner.start + emoji.char.length,
  };
}

/**
 * Put an emoji where the caret is, replacing a shortcode in progress.
 *
 * The trailing space is what makes the picker and the typeahead feel the same:
 * after either one you carry on typing words, not names.
 */
export function insertEmoji(
  text: string,
  caret: number,
  emoji: Emoji,
  options: { replace?: ShortcodeQuery } = {},
): TextEdit {
  const start = options.replace ? options.replace.start : caret;
  const end = options.replace ? options.replace.end : caret;

  const inserted = `${emoji.char} `;
  return {
    text: text.slice(0, start) + inserted + text.slice(end),
    caret: start + inserted.length,
  };
}
