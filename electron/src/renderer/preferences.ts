/**
 * Small per-window preferences: sidebar width, recently used emoji.
 *
 * Deliberately *not* engine settings. Those are signed, stored, and synced to
 * your other devices, which is right for "keep messages for 30 days" and wrong
 * for how wide a pane is on this particular screen — a laptop and a desktop
 * disagreeing about that is correct behaviour, not drift.
 *
 * Every access is guarded. `localStorage` throws rather than returning null
 * when storage is unavailable or the quota is full, and none of these are
 * worth failing a render over.
 */

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // A preference that cannot be saved is still usable for this session.
  }
}

/* --------------------------------------------------------------------------
 * Sidebar width
 * -------------------------------------------------------------------------- */

const WIDTH_KEY = 'bounce.leftPaneWidth';

/*
 * The limits are duplicated as `min-width`/`max-width` in the stylesheet so
 * that a pane rendered before this module has run is still the right size.
 * Change one, change the other.
 */

/**
 * Narrower than this and the header runs out of room: an avatar, a search
 * box and three buttons stop fitting on one line.
 */
export const MIN_LEFT_PANE_WIDTH = 240;

/** Past this the sidebar starts to look like the main event. */
export const MAX_LEFT_PANE_WIDTH = 520;

export const DEFAULT_LEFT_PANE_WIDTH = 300;

/** Hold a width inside the range the layout can actually render. */
export function clampLeftPaneWidth(width: number): number {
  if (!Number.isFinite(width)) return DEFAULT_LEFT_PANE_WIDTH;
  return Math.min(MAX_LEFT_PANE_WIDTH, Math.max(MIN_LEFT_PANE_WIDTH, Math.round(width)));
}

export function loadLeftPaneWidth(): number {
  const stored = read(WIDTH_KEY);
  if (stored === null) return DEFAULT_LEFT_PANE_WIDTH;
  const width = Number.parseInt(stored, 10);
  // A stored width from a build with different limits gets clamped rather than
  // discarded, so the pane lands near where it was left.
  return Number.isNaN(width) ? DEFAULT_LEFT_PANE_WIDTH : clampLeftPaneWidth(width);
}

export function saveLeftPaneWidth(width: number): void {
  write(WIDTH_KEY, String(clampLeftPaneWidth(width)));
}

/* --------------------------------------------------------------------------
 * Recently used emoji
 * -------------------------------------------------------------------------- */

const RECENT_KEY = 'bounce.recentEmoji';
const PREFERRED_REACTIONS_KEY = 'bounce.preferredReactions';

/**
 * Signal's default six, in Signal's order.
 *
 * Heart first because it is far and away the commonest, and the two thumbs
 * adjacent so agreeing and disagreeing are one place on the strip.
 */
export const DEFAULT_REACTIONS = ['❤️', '👍', '👎', '😂', '😮', '😢'] as const;

/** One row in the picker, so the section never pushes the grid down. */
export const MAX_RECENT_EMOJI = 8;

/**
 * The characters last picked, most recent first.
 *
 * Stored as characters rather than shortcodes so that an emoji dropped from a
 * future data refresh still shows up here instead of vanishing.
 */
export function loadRecentEmoji(): string[] {
  const stored = read(RECENT_KEY);
  if (!stored) return [];

  try {
    const parsed: unknown = JSON.parse(stored);
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((entry): entry is string => typeof entry === 'string')
      .slice(0, MAX_RECENT_EMOJI);
  } catch {
    return [];
  }
}

/**
 * The six emoji on the reaction strip.
 *
 * Seeded from what you actually reach for, then topped up from Signal's
 * defaults so the strip is always six wide. A strip that grew as you used it
 * would move the buttons under the pointer, which is worse than showing an
 * emoji you have not picked yet.
 */
export function loadPreferredReactions(): string[] {
  const stored = read(PREFERRED_REACTIONS_KEY);
  const saved = (() => {
    if (!stored) return [];
    try {
      const parsed: unknown = JSON.parse(stored);
      return Array.isArray(parsed)
        ? parsed.filter((entry): entry is string => typeof entry === 'string')
        : [];
    } catch {
      return [];
    }
  })();

  const filled = [...saved];
  for (const emoji of DEFAULT_REACTIONS) {
    if (filled.length >= DEFAULT_REACTIONS.length) break;
    if (!filled.includes(emoji)) filled.push(emoji);
  }
  return filled.slice(0, DEFAULT_REACTIONS.length);
}

/** Note a reaction, moving it to the front of the strip. */
export function notePreferredReaction(character: string): string[] {
  const next = [
    character,
    ...loadPreferredReactions().filter((entry) => entry !== character),
  ].slice(0, DEFAULT_REACTIONS.length);
  write(PREFERRED_REACTIONS_KEY, JSON.stringify(next));
  return next;
}

/** Record a use, moving it to the front. Returns the new list. */
export function noteRecentEmoji(character: string): string[] {
  const next = [character, ...loadRecentEmoji().filter((entry) => entry !== character)].slice(
    0,
    MAX_RECENT_EMOJI,
  );
  write(RECENT_KEY, JSON.stringify(next));
  return next;
}
