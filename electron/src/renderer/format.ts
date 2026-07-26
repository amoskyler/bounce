/**
 * Date and text formatting, following Signal's conventions.
 */

const SECOND = 1000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** Whether two instants fall on the same calendar day, in local time. */
function isSameDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

/**
 * The timestamp shown on a conversation row.
 *
 * Today shows a clock time, the past week a weekday, and anything older a
 * date — so the column stays narrow while remaining unambiguous.
 */
export function conversationTimestamp(unixSeconds: number, now = new Date()): string {
  if (!unixSeconds) return '';

  const when = new Date(unixSeconds * 1000);

  if (isSameDay(when, now)) {
    return when.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
  }

  const yesterday = new Date(now.getTime() - DAY);
  if (isSameDay(when, yesterday)) {
    return 'Yesterday';
  }

  if (now.getTime() - when.getTime() < 7 * DAY) {
    return when.toLocaleDateString(undefined, { weekday: 'short' });
  }

  const sameYear = when.getFullYear() === now.getFullYear();
  return when.toLocaleDateString(undefined, {
    month: 'numeric',
    day: 'numeric',
    ...(sameYear ? {} : { year: '2-digit' }),
  });
}

/** The clock time shown inside a message bubble. */
export function messageTimestamp(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toLocaleTimeString(undefined, {
    hour: 'numeric',
    minute: '2-digit',
  });
}

/** The separator shown between days in the timeline. */
export function dateSeparator(unixSeconds: number, now = new Date()): string {
  const when = new Date(unixSeconds * 1000);

  if (isSameDay(when, now)) return 'Today';
  if (isSameDay(when, new Date(now.getTime() - DAY))) return 'Yesterday';

  if (now.getTime() - when.getTime() < 7 * DAY) {
    return when.toLocaleDateString(undefined, { weekday: 'long' });
  }

  const sameYear = when.getFullYear() === now.getFullYear();
  return when.toLocaleDateString(undefined, {
    weekday: 'short',
    month: 'short',
    day: 'numeric',
    ...(sameYear ? {} : { year: 'numeric' }),
  });
}

/** Whether a date separator belongs between two consecutive messages. */
export function needsDateSeparator(previous: number | null, current: number): boolean {
  if (previous === null) return true;
  return !isSameDay(new Date(previous * 1000), new Date(current * 1000));
}

/**
 * Whether two consecutive messages should be visually grouped.
 *
 * Signal groups a run from the same author within a few minutes into one
 * cluster with a single tail, so a burst of messages reads as one turn.
 */
export function shouldGroupWith(
  previous: { author: string; writtenAt: number } | null,
  current: { author: string; writtenAt: number },
): boolean {
  if (!previous) return false;
  if (previous.author !== current.author) return false;
  return (current.writtenAt - previous.writtenAt) * SECOND < 3 * MINUTE;
}

/** A one-line preview of a message for the conversation list. */
export function snippet(text: string, attachmentCount: number): string {
  const collapsed = text.replace(/\s+/g, ' ').trim();
  if (collapsed) return collapsed;

  if (attachmentCount > 1) return `${attachmentCount} attachments`;
  if (attachmentCount === 1) return 'Attachment';
  return '';
}

/** A human-readable file size. */
export function fileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ['KB', 'MB', 'GB'];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}

/** Truncate an onion address for display, keeping both ends recognisable. */
export function shortAddress(address: string): string {
  if (address.length <= 20) return address;
  return `${address.slice(0, 8)}…${address.slice(-6)}`;
}
