/**
 * Who is typing, in words.
 *
 * The engine has always said *who* — `TypingStarted` carries a user id, and the
 * reducer keeps the whole set per conversation. The interface then reduced it
 * to a count and drew three anonymous dots, which in a group of eight tells you
 * that somebody, somewhere, is composing something.
 *
 * ## Against the Go client
 *
 * `ui/typing_indicator.go` has two modes. In icon mode it draws an avatar per
 * typing user, capped at eight. In text mode it writes the display name of the
 * *most recently* seen typer followed by a colon — only ever one name, even
 * when several people are typing.
 *
 * This names them all, up to a limit, because "Ada" and "Ada and Bo" are
 * different facts and the count is the thing that was missing. The cap and the
 * ordering rule are Go's.
 *
 * In a one-to-one conversation there is exactly one person who could be typing,
 * so naming them says nothing the header does not. That case stays as bare
 * dots — a deliberate divergence from Go's text mode, which prints the name
 * there too.
 */

/** How many typists to name before falling back to a count. Go's `maxTypingUsers`. */
export const MAXIMUM_NAMED_TYPISTS = 8;

/** Display names by user id, as the caller already assembles them elsewhere. */
export type TypistNames = Readonly<Record<string, string | undefined>>;

/** What a name table has no entry for. */
const UNKNOWN = 'Someone';

/**
 * Join names the way English does: "a", "a and b", "a, b and c".
 *
 * Beyond the cap it becomes "a, b and 4 others", because a group of thirty
 * where everybody is replying at once should not push the timeline off screen.
 */
function joinNames(names: readonly string[], hidden: number): string {
  if (hidden > 0) {
    const others = hidden === 1 ? '1 other' : `${hidden} others`;
    return names.length === 0 ? others : `${names.join(', ')} and ${others}`;
  }

  if (names.length === 1) return names[0];
  if (names.length === 2) return `${names[0]} and ${names[1]}`;
  return `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
}

/**
 * The sentence shown beside the animated dots, or null for bare dots.
 *
 * `userIds` is the set from `state.typingByThread`, in the order the engine
 * reported them — most recent last, which is the order Go relies on when it
 * has room for only one name.
 */
export function typingLabel(
  userIds: readonly string[],
  names: TypistNames,
  options: { isGroup: boolean; selfId?: string | null } = { isGroup: true },
): string | null {
  // Our own device never reports itself, but a stale entry should not be able
  // to tell somebody they are typing to themselves.
  const typists = userIds.filter((id) => id !== options.selfId);
  if (typists.length === 0) return null;

  // Only one person can be typing at you in a one-to-one conversation, and
  // their name is already at the top of the window.
  if (!options.isGroup) return null;

  // Newest last, so the ones that survive the cap are the ones that just
  // started — the same choice Go makes when it shows a single name.
  const shown = typists.slice(-MAXIMUM_NAMED_TYPISTS);
  const hidden = typists.length - shown.length;

  const resolved = shown.map((id) => names[id] ?? UNKNOWN);
  const verb = typists.length === 1 ? 'is typing' : 'are typing';

  return `${joinNames(resolved, hidden)} ${verb}`;
}

/**
 * The avatars to draw beside the dots, newest last.
 *
 * Go's icon mode draws one per typist up to the same cap. Returning ids rather
 * than elements keeps this file free of JSX so it can be unit tested without a
 * DOM.
 */
export function typingAvatarIds(
  userIds: readonly string[],
  options: { selfId?: string | null } = {},
): string[] {
  return userIds
    .filter((id) => id !== options.selfId)
    .slice(-MAXIMUM_NAMED_TYPISTS);
}
