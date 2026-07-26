/**
 * The disappearing-message clock.
 *
 * Signal draws thirteen 12px icons — a dial whose ring is solid for the time
 * remaining and dotted for the time spent, with a hand sweeping round it — and
 * swaps between them as a message ages. This draws the same dial from geometry
 * instead of shipping thirteen sprites, which makes it one component rather
 * than a folder of near-identical files, and lets it inherit `currentColor` the
 * way the rest of the icon set does.
 *
 * Rendering is a leaf, memoised, and re-runs only when the bucket changes —
 * roughly twelve times over the life of a message, no matter how long that is.
 * See `expire-timer.ts` for why there is one ticker for the whole window.
 */

import * as React from 'react';

import { subscribeToTimer, timerBucket, timerNow, type TimerBucket } from './expire-timer';

/** Matches Signal: a 12×12 icon on a 12-unit viewBox. */
const SIZE = 12;
const CENTRE = SIZE / 2;

/** Leaves room for the stroke inside the box. */
const RADIUS = 5.45;

/** Signal's ring weight at this size. */
const STROKE = 1.1;

/** The hand, stopping short of the ring. */
const HAND = 3.25;

const CIRCUMFERENCE = 2 * Math.PI * RADIUS;

/**
 * The dial for one bucket.
 *
 * Two circles and a line. The dotted ring underneath is the whole face; the
 * solid arc on top covers the fraction still remaining, drawn clockwise from
 * twelve o'clock. The hand points at the same fraction, so a full dial has it
 * straight up and an empty one has swept the whole way round.
 */
function Dial({ bucket }: { bucket: TimerBucket }) {
  // '60' is a full dial and '00' an empty one, so the label doubles as the
  // fraction remaining once read as minutes on a clock face.
  const fraction = Number(bucket) / 60;
  const arc = CIRCUMFERENCE * fraction;

  return (
    <svg
      width={SIZE}
      height={SIZE}
      viewBox={`0 0 ${SIZE} ${SIZE}`}
      fill="none"
      aria-hidden="true"
      className="expire-timer__dial"
    >
      {/*
        The spent portion, dotted. Drawn as the whole ring rather than only the
        gap, because the solid arc sits on top of it — one fewer arc to get the
        endpoints of, and no seam where the two meet.
      */}
      <circle
        cx={CENTRE}
        cy={CENTRE}
        r={RADIUS}
        stroke="currentColor"
        strokeWidth={STROKE}
        strokeLinecap="round"
        strokeDasharray="0.1 1.75"
        opacity={0.55}
      />

      {fraction > 0 && (
        <circle
          cx={CENTRE}
          cy={CENTRE}
          r={RADIUS}
          stroke="currentColor"
          strokeWidth={STROKE}
          strokeLinecap="round"
          strokeDasharray={`${arc} ${CIRCUMFERENCE}`}
          // Rotated so the arc starts at twelve o'clock and runs clockwise;
          // an SVG circle otherwise starts at three.
          transform={`rotate(-90 ${CENTRE} ${CENTRE})`}
        />
      )}

      <line
        x1={CENTRE}
        y1={CENTRE}
        x2={CENTRE}
        y2={CENTRE - HAND}
        stroke="currentColor"
        strokeWidth={STROKE}
        strokeLinecap="round"
        transform={`rotate(${fraction * 360} ${CENTRE} ${CENTRE})`}
      />
    </svg>
  );
}

const MemoDial = React.memo(Dial);

/**
 * A clock for a message that will disappear.
 *
 * `expiresAt` and `writtenAt` are unix *seconds*, as the engine reports them;
 * the message's whole lifetime is the difference, which is what the dial is a
 * fraction of.
 */
export function ExpireTimer({
  expiresAt,
  writtenAt,
}: {
  expiresAt: number;
  writtenAt: number;
}) {
  const length = Math.max(0, (expiresAt - writtenAt) * 1000);
  const expiresAtMs = expiresAt * 1000;

  const subscribe = React.useCallback(
    (notify: () => void) => subscribeToTimer(length, notify),
    [length],
  );

  /*
   * The subscription's value is the bucket itself, not a tick count.
   *
   * That is the whole performance story: the ticker fires, this recomputes one
   * rounded fraction, and React compares it to what was rendered last. Eleven
   * times in twelve the answer is the same string and nothing re-renders at
   * all — no reconciliation, no DOM, no layout.
   */
  const bucket = React.useSyncExternalStore(
    subscribe,
    // `timerNow`, not `Date.now`: the snapshot has to be the same on two calls
    // in the same render, and a clock read fresh each time is not.
    () => timerBucket(expiresAtMs, length, timerNow()),
    // No timer has run down on the server, so the first paint is a full dial.
    () => '60' as TimerBucket,
  );

  return (
    <span
      className="expire-timer"
      title={length > 0 ? 'This message will disappear' : undefined}
    >
      <MemoDial bucket={bucket} />
    </span>
  );
}
