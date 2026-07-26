/**
 * Avatars.
 *
 * A contact or group can carry images, and the newest one is drawn
 * circle-cropped. With no photo — or before its bytes have arrived — this falls
 * back to initials on a tinted background, with the tint chosen
 * deterministically from the contact's identifier so the same person is always
 * the same colour on every device. Go makes the same two-way choice in
 * `ui/default_image.go:82-198`.
 */

import * as React from 'react';

import { cachedFileUrl, fileUrl } from './attachment-data';

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
 * The colour a person is known by, for text set in their name.
 *
 * The same palette entry their avatar uses, so a name and the circle beside it
 * agree — which is what lets a reader skim a group by colour rather than by
 * reading every name.
 *
 * Both ends are returned because the palette is built for one job and asked to
 * do two. Each entry is a pale tint carrying a saturated companion, sized for
 * dark-on-light initials; used as text on a dark bubble the saturated end is
 * far too dark to read, so the pair swaps over. The caller hands both to CSS
 * and lets the stylesheet choose, which keeps this correct across a theme
 * change without re-rendering a single message.
 */
export function colorsForId(identifier: string): { light: string; dark: string } {
  const entry = paletteFor(identifier);
  return { light: entry.foreground, dark: entry.background };
}

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

/**
 * The object URL for the image to draw, or undefined to fall back to initials.
 *
 * The last image in the list is the current one, which is why Go walks the list
 * backwards looking for one it can draw (`ui/default_image.go:84-100`). The
 * bytes come through the same cache the attachment bubbles use, so the same
 * contact in the sidebar, the header and a run of bubbles costs one decode
 * between them.
 *
 * A file still being fetched resolves to null, and this returns undefined: the
 * initials stand in, exactly as they do for someone with no photo at all. The
 * next render after the bytes land picks the URL up.
 */
function useAvatarImage(images: readonly string[] | undefined): string | undefined {
  const fileId = images && images.length > 0 ? images[images.length - 1] : undefined;
  const [, forceRender] = React.useReducer((count: number) => count + 1, 0);

  const cached = fileId === undefined ? undefined : cachedFileUrl(fileId);

  // `cached` is a dependency so that an avatar whose URL was evicted from the
  // cache — it is bounded, and a thread full of photographs can fill it — asks
  // for the bytes again on the next render instead of showing initials for the
  // rest of the session.
  React.useEffect(() => {
    if (fileId === undefined || cached !== undefined) return;

    let cancelled = false;
    void fileUrl(fileId).then((url) => {
      // Only a URL that arrived is worth a render; a file that is still
      // downloading would otherwise re-render every avatar on screen.
      if (!cancelled && url !== null) forceRender();
    });

    return () => {
      cancelled = true;
    };
  }, [fileId, cached]);

  return cached;
}

type AvatarProps = {
  /** Stable identifier, used to choose the colour. */
  id: string;
  /** Display name, used for the initials. */
  name: string;
  /** File ids of the profile or group images, oldest first. */
  images?: readonly string[];
  size?: number;
  /** Shows the presence dot when true. */
  online?: boolean;
  className?: string;
};

export function Avatar({ id, name, images, size = 48, online = false, className }: AvatarProps) {
  const { background, foreground } = paletteFor(id);
  const image = useAvatarImage(images);

  return (
    // The tint stays under the image rather than being dropped: it is what
    // shows through a photo with transparency, and what is on screen for the
    // moment between the element being laid out and the image decoding.
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
      {image === undefined ? (
        <span className="avatar__initials">{initialsFor(name)}</span>
      ) : (
        // Decorative: the name is already on the wrapper's title, and in every
        // call site it is also written beside the avatar.
        <img className="avatar__image" src={image} alt="" draggable={false} />
      )}
      {online && <span className="avatar__presence" aria-label="online" />}
    </div>
  );
}
