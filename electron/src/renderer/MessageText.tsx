/**
 * Message bodies.
 *
 * A message is text somebody else wrote, so everything here treats it as
 * hostile input: links are recognised by pattern but only ever rendered after
 * the parsed URL is confirmed to be http or https, and the long-message cut is
 * made on Unicode scalar values rather than UTF-16 code units so a surrogate
 * pair is never split into two replacement characters.
 */

import * as React from 'react';

import './message-text.css';

/**
 * How much of a message is shown before the "Read more" toggle appears.
 *
 * The Fyne client uses `maxRunes = 500` for the same purpose; keeping the two
 * equal means a message that reads as truncated on one client reads the same
 * way on the other.
 */
export const MAX_RUNES = 500;

/** A run of a message body: either plain text or a link that may be followed. */
export type MessageSegment =
  | { kind: 'text'; text: string }
  | { kind: 'link'; text: string; href: string };

/**
 * Candidate links.
 *
 * Only http and https are matched. A `file://` or `javascript:` URL from a
 * stranger is an attack rather than a convenience, and the cheapest way to
 * never render one is to never recognise it in the first place.
 *
 * The lookbehind stops a scheme being found inside a longer word, so
 * "nothttps://example.com" is left alone. Quotes, angle brackets and
 * backslashes end a match because they are far more likely to be markup or
 * prose around the link than part of it.
 */
const LINK_PATTERN = /(?<![\w@.-])https?:\/\/[^\s<>"'`\\]+/gi;

/** Characters that end a sentence rather than a URL. */
const TRAILING_PUNCTUATION = '.,;:!?';

/** Whether every closing parenthesis in a string has an opener before it. */
function parenthesesAreBalanced(text: string): boolean {
  let depth = 0;
  for (const character of text) {
    if (character === '(') depth += 1;
    else if (character === ')') {
      depth -= 1;
      if (depth < 0) return false;
    }
  }
  return depth === 0;
}

/**
 * Drop the punctuation a writer put after a link rather than inside it.
 *
 * "See https://example.com/a." ends in a full stop that belongs to the
 * sentence, and "(https://en.wikipedia.org/wiki/Ruby_(gemstone))" ends in a
 * parenthesis that belongs to the aside — but the inner pair belongs to the
 * URL, so parentheses are only dropped when they are unmatched. This mirrors
 * the Fyne client's `hyperlinkify`.
 */
export function trimTrailingPunctuation(candidate: string): string {
  let end = candidate.length;

  while (end > 0) {
    const character = candidate[end - 1];

    if (TRAILING_PUNCTUATION.includes(character)) {
      end -= 1;
      continue;
    }

    if (character === ')' && !parenthesesAreBalanced(candidate.slice(0, end))) {
      end -= 1;
      continue;
    }

    break;
  }

  return candidate.slice(0, end);
}

/**
 * The URL to put in an `href`, or null if the candidate must stay plain text.
 *
 * `LINK_PATTERN` already refuses to match anything but http and https, so in
 * practice this only rejects malformed URLs. It re-checks the scheme anyway:
 * this is the single point where message text becomes something the browser
 * will navigate to, and it should be safe on its own terms rather than because
 * a regular expression two functions away happens to be written correctly.
 */
export function safeHref(candidate: string): string | null {
  let parsed: URL;
  try {
    parsed = new URL(candidate);
  } catch {
    return null;
  }

  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return null;
  return parsed.href;
}

/**
 * Split a message body into text and link runs.
 *
 * Newlines and runs of whitespace are preserved verbatim in the text
 * segments; the timeline renders them with `white-space: pre-wrap`.
 */
export function splitLinks(text: string): MessageSegment[] {
  const segments: MessageSegment[] = [];
  let consumed = 0;

  for (const match of text.matchAll(LINK_PATTERN)) {
    const start = match.index ?? 0;
    const candidate = trimTrailingPunctuation(match[0]);
    const href = safeHref(candidate);

    // Anything we decline to link stays inside the surrounding text run,
    // which the next flush picks up untouched.
    if (href === null) continue;

    if (start > consumed) {
      segments.push({ kind: 'text', text: text.slice(consumed, start) });
    }

    segments.push({ kind: 'link', text: candidate, href });
    consumed = start + candidate.length;
  }

  if (consumed < text.length) {
    segments.push({ kind: 'text', text: text.slice(consumed) });
  }

  return segments;
}

/**
 * Cut a message to `limit` Unicode scalar values.
 *
 * Spreading the string iterates code points, so a four-byte emoji counts once
 * and is never halved. Combining sequences and ZWJ emoji can still be divided,
 * which is what the Go client's rune count does too — matching it keeps the
 * cut in the same place on both clients.
 */
export function truncateRunes(
  text: string,
  limit: number = MAX_RUNES,
): { body: string; truncated: boolean } {
  const runes = [...text];
  if (runes.length <= limit) return { body: text, truncated: false };
  return { body: runes.slice(0, limit).join(''), truncated: true };
}

type MessageTextProps = {
  /** The message body, exactly as its author wrote it. */
  text: string;
};

/**
 * A message body, with links and a "Read more" toggle on long messages.
 *
 * The main process turns every renderer-initiated navigation into an external
 * one through `setWindowOpenHandler`, so a plain `target="_blank"` anchor
 * opens the reader's browser and the window itself never navigates. `noopener`
 * also denies the opened page a handle back to this one.
 */
export function MessageText({ text }: MessageTextProps) {
  const [expanded, setExpanded] = React.useState(false);

  const { body, truncated } = truncateRunes(text);
  const shown = truncated && !expanded ? `${body}…` : text;

  return (
    <>
      {splitLinks(shown).map((segment, index) =>
        segment.kind === 'link' ? (
          <a
            key={index}
            className="message-text__link"
            href={segment.href}
            target="_blank"
            rel="noreferrer noopener"
          >
            {segment.text}
          </a>
        ) : (
          <React.Fragment key={index}>{segment.text}</React.Fragment>
        ),
      )}

      {truncated && (
        <button
          type="button"
          className="message-text__read-more"
          onClick={() => setExpanded((open) => !open)}
        >
          {expanded ? 'Read less' : 'Read more'}
        </button>
      )}
    </>
  );
}
