/**
 * Generate `src/renderer/emoji-data.ts` from emojibase.
 *
 * Run with `npm run build:emoji`. The output is committed, so a normal build
 * needs neither this script nor the `emojibase-data` package — which is why
 * that package is a devDependency and never reaches the shipped bundle.
 *
 * Shortcodes come from the `iamcal` set first. That is the naming used by
 * `emoji-datasource`, which is what Signal itself is built on, so `:smile:`
 * means the same emoji here as it does there. GitHub's set is folded in next
 * because it is what people have in their fingers, and emojibase's own set
 * last so that emoji too new for either list are still typeable.
 */

import { writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const here = dirname(fileURLToPath(import.meta.url));

const compact = require('emojibase-data/en/compact.json');

/** Most authoritative first. See the note at the top of the file. */
const shortcodeSets = [
  require('emojibase-data/en/shortcodes/iamcal.json'),
  require('emojibase-data/en/shortcodes/github.json'),
  require('emojibase-data/en/shortcodes/emojibase.json'),
];

/**
 * Our categories, in the order Signal shows its tabs.
 *
 * Emojibase splits faces from bodies; Signal shows them as one section, so the
 * two groups map onto a single category here. Group 2 ("component" — the skin
 * tone and hair modifiers) has no entry: those are pieces of other emoji, not
 * things you can send on their own.
 */
const CATEGORIES = [
  { name: 'Smileys & People', groups: [0, 1] },
  { name: 'Animals & Nature', groups: [3] },
  { name: 'Food & Drink', groups: [4] },
  { name: 'Activities', groups: [6] },
  { name: 'Travel & Places', groups: [5] },
  { name: 'Objects', groups: [7] },
  { name: 'Symbols', groups: [8] },
  { name: 'Flags', groups: [9] },
];

const categoryOfGroup = new Map();
CATEGORIES.forEach((category, index) => {
  for (const group of category.groups) categoryOfGroup.set(group, index);
});

/**
 * Assign each shortcode to exactly one emoji.
 *
 * The sets disagree, and not by accident: emojibase reuses several of iamcal's
 * names for different emoji, so `train` is 🚋 in one and 🚆 in the other. Left
 * unresolved, whichever emoji happened to sort first would win, and `:train:`
 * would quietly stop meaning what it means in Signal.
 *
 * So a name is claimed by the most authoritative set that uses it, and a lower
 * set cannot take it back. Names are stored bare — the colons are punctuation
 * around the name, not part of it.
 */
function claimShortcodes(emojiList) {
  /** shortcode -> hexcode that owns it. */
  const owner = new Map();
  /** hexcode -> shortcodes, in the order they were claimed. */
  const claimed = new Map();

  for (const set of shortcodeSets) {
    for (const emoji of emojiList) {
      const value = set[emoji.hexcode];
      if (!value) continue;

      for (const code of Array.isArray(value) ? value : [value]) {
        if (owner.has(code)) continue;
        owner.set(code, emoji.hexcode);

        const codes = claimed.get(emoji.hexcode);
        if (codes) codes.push(code);
        else claimed.set(emoji.hexcode, [code]);
      }
    }
  }

  return claimed;
}

/**
 * A word is worth storing only if it is not already reachable.
 *
 * Search matches shortcodes and the label as well as these, so a tag that
 * repeats a word from either is dead weight in a file that ships to every
 * user.
 */
function usefulKeywords(emoji, shortcodes) {
  const already = new Set();
  for (const code of shortcodes) for (const word of code.split(/[_-]/)) already.add(word);
  for (const word of emoji.label.toLowerCase().split(/[^a-z0-9+]+/)) already.add(word);

  const keywords = [];
  for (const tag of emoji.tags ?? []) {
    const word = tag.toLowerCase();
    // Tab and newline are the record separators, and a space separates words
    // within a field; a tag carrying any of them would corrupt the parse.
    if (/[\s]/.test(word)) continue;
    if (already.has(word) || keywords.includes(word)) continue;
    keywords.push(word);
  }
  return keywords;
}

// Only emoji that will actually be shown are eligible to claim a name, so a
// skin tone modifier cannot sit on a shortcode that no picker entry can use.
const included = compact.filter((emoji) => categoryOfGroup.has(emoji.group));
const shortcodesByHexcode = claimShortcodes(included);

const rows = [];
let skippedNoShortcode = 0;

for (const emoji of included) {
  const category = categoryOfGroup.get(emoji.group);
  const shortcodes = shortcodesByHexcode.get(emoji.hexcode);

  if (!shortcodes || shortcodes.length === 0) {
    // Nothing left to type it by, which means the typeahead could never
    // produce it and the picker would show a nameless cell.
    skippedNoShortcode += 1;
    continue;
  }

  rows.push({
    order: emoji.order ?? Number.MAX_SAFE_INTEGER,
    line: [
      emoji.unicode,
      emoji.label.toLowerCase(),
      String(category),
      shortcodes.join(' '),
      usefulKeywords(emoji, shortcodes).join(' '),
    ].join('\t'),
  });
}

// Sorted here rather than at load: the picker wants Unicode's own ordering,
// which groups visually similar emoji together, and doing it once at build
// time saves every client the sort.
rows.sort((a, b) => a.order - b.order);

const packed = rows.map((row) => row.line).join('\n');

// A backtick or a `${` in the data would end the template literal early. No
// emoji label contains either today, but a silent corruption on some future
// data refresh is not worth the risk of assuming it never will.
if (/[`\\]|\$\{/.test(packed)) {
  throw new Error('emoji data contains a character that cannot go in a template literal');
}

const output = `/**
 * The emoji table. GENERATED — do not edit; run \`npm run build:emoji\`.
 *
 * Packed into one string rather than written as ${rows.length} object literals
 * because the parsed form is what we want in memory anyway, and a megabyte of
 * JavaScript object syntax costs both bundle size and parse time to arrive at
 * the same place.
 *
 * Records are newline-separated. Fields are tab-separated, in order:
 * character, label, category index, shortcodes, keywords — the last two being
 * space-separated lists. Shortcodes are stored without their colons, and the
 * first one is the canonical name shown in the picker.
 */

/** One emoji, as the picker and the typeahead see it. */
export interface Emoji {
  /** The character itself, ready to insert. */
  readonly char: string;
  /** Human-readable name, e.g. "grinning face". */
  readonly label: string;
  /** Index into {@link EMOJI_CATEGORIES}. */
  readonly category: number;
  /** Names typeable between colons. Never empty; the first is canonical. */
  readonly shortcodes: readonly string[];
  /** Extra search terms that do not already appear in the label or shortcodes. */
  readonly keywords: readonly string[];
}

/** Picker sections, in display order. */
export const EMOJI_CATEGORIES: readonly string[] = ${JSON.stringify(
  CATEGORIES.map((category) => category.name),
)};

const PACKED = \`${packed}\`;

function unpack(): readonly Emoji[] {
  const emoji: Emoji[] = [];
  for (const line of PACKED.split('\\n')) {
    const [char, label, category, shortcodes, keywords] = line.split('\\t');
    emoji.push({
      char,
      label,
      category: Number(category),
      shortcodes: shortcodes.split(' '),
      keywords: keywords ? keywords.split(' ') : [],
    });
  }
  return emoji;
}

/** Every emoji we know, in Unicode's own order. */
export const EMOJI: readonly Emoji[] = unpack();
`;

const target = join(here, '..', 'src', 'renderer', 'emoji-data.ts');
writeFileSync(target, output);

console.log(
  `wrote ${rows.length} emoji to ${target} (${(output.length / 1024).toFixed(0)} KB), ` +
    `skipped ${skippedNoShortcode} with no shortcode`,
);
