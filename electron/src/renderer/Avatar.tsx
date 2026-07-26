/**
 * Avatars.
 *
 * With no profile photo, Signal falls back to initials on a tinted background,
 * with the tint chosen deterministically from the contact's identifier so the
 * same person is always the same colour on every device.
 */

import * as React from 'react';

/**
 * Signal's avatar palette: twelve background/foreground pairs, each a muted
 * tint with a saturated companion, so initials stay legible in both themes.
 */
const PALETTE: ReadonlyArray<{ background: string; foreground: string }> = [
  { background: '#e3e3fe', foreground: '#3838f5' },
  { background: '#dde4f7', foreground: '#1251d3' },
  { background: '#d8e8f0', foreground: '#086da0' },
  { background: '#cde4cd', foreground: '#067906' },
  { background: '#eae0f8', foreground: '#7a3ede' },
  { background: '#f5e3fe', foreground: '#b814b8' },
  { background: '#f6d8ec', foreground: '#c625a4' },
  { background: '#f5d7d7', foreground: '#d00b0b' },
  { background: '#fef5d0', foreground: '#8f6600' },
  { background: '#eae6d5', foreground: '#6c6c13' },
  { background: '#dde4e6', foreground: '#077288' },
  { background: '#d2d2dc', foreground: '#3b3b45' },
];

/**
 * Pick a palette entry from an identifier.
 *
 * Any stable hash works; this one is FNV-1a, chosen because it is short,
 * dependency-free, and spreads adjacent UUIDs across different buckets.
 */
function paletteFor(identifier: string) {
  let hash = 0x811c9dc5;
  for (let i = 0; i < identifier.length; i += 1) {
    hash ^= identifier.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return PALETTE[hash % PALETTE.length];
}

/**
 * Up to two initials from a display name.
 *
 * Uses the first and last word so "Ada Byron Lovelace" reads as "AL", and
 * falls back to a single character for mononyms. Iterating with the spread
 * operator rather than by index keeps multi-byte characters intact.
 *
 * Parenthesised suffixes are dropped, so the note-to-self conversation
 * ("Hayden Parker (You)") still reads as "HP" rather than "H(".
 */
function initialsFor(name: string): string {
  const words = name
    .trim()
    .split(/\s+/)
    .filter((word) => word.length > 0 && !word.startsWith('('));

  if (words.length === 0) return '?';

  const first = [...words[0]][0] ?? '';
  if (words.length === 1) return first.toUpperCase();

  const last = [...words[words.length - 1]][0] ?? '';
  return (first + last).toUpperCase();
}

type AvatarProps = {
  /** Stable identifier, used to choose the colour. */
  id: string;
  /** Display name, used for the initials. */
  name: string;
  size?: number;
  /** Shows the presence dot when true. */
  online?: boolean;
  className?: string;
};

export function Avatar({ id, name, size = 48, online = false, className }: AvatarProps) {
  const { background, foreground } = paletteFor(id);

  return (
    <div
      className={className ? `avatar ${className}` : 'avatar'}
      style={
        {
          '--avatar-size': `${size}px`,
          background,
          color: foreground,
        } as React.CSSProperties
      }
      title={name}
    >
      <span className="avatar__initials">{initialsFor(name)}</span>
      {online && <span className="avatar__presence" aria-label="online" />}
    </div>
  );
}
