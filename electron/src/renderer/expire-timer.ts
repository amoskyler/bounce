/**
 * The disappearing-message clock: what it should read, and when to look again.
 *
 * The arithmetic is Signal's, exactly. `ts/util/timer.std.ts` rounds the
 * remaining fraction into twelve increments and names the result in minutes on
 * a clock face — "60" for a full dial down to "00" for an empty one — and the
 * dial redraws every `ceil(length / 12)` milliseconds, floored at 500. Matching
 * the rounding matters because it decides where the hand sits, and rounding
 * where Signal floors puts it half a step out for the whole life of a message.
 *
 * ## Why one ticker
 *
 * Signal gives every timer its own `setInterval`. That is fine there and would
 * be fine here — only the rows near the viewport are mounted — but a timer per
 * message is a wake-up per message, and they do not align, so the main thread
 * is nudged continuously rather than in one batch.
 *
 * Instead every clock on screen subscribes to the single ticker below. It runs
 * at the finest cadence any subscriber asked for, stops entirely when the last
 * one unmounts, and stops again whenever the window is hidden — a background
 * window redrawing clocks nobody is looking at is pure waste.
 *
 * The saving that matters most, though, is that a tick is not a render. Each
 * clock's subscription value is its bucket, a string, and React skips the
 * re-render when that has not changed. Between one bucket and the next the cost
 * of a tick is one subtraction and one rounding per visible clock, and nothing
 * else happens at all.
 */

/** The dial positions, from a full clock face to an empty one. */
export type TimerBucket =
  | '60'
  | '55'
  | '50'
  | '45'
  | '40'
  | '35'
  | '30'
  | '25'
  | '20'
  | '15'
  | '10'
  | '05'
  | '00';

/** Never redraw faster than this, however short the timer. */
const MINIMUM_INTERVAL = 500;

/**
 * How often a timer of this length needs redrawing, in milliseconds.
 *
 * Twelve increments over the whole life of the message, because that is how
 * many positions the dial has. Signal's `getIncrement`.
 */
export function timerIncrement(length: number): number {
  if (length < 0) return 1000;
  return Math.ceil(length / 12);
}

/**
 * What the dial should read.
 *
 * `expiresAt` and `length` are milliseconds, matching Signal; the engine works
 * in seconds, so the caller converts. Signal's `getTimerBucket`.
 */
export function timerBucket(
  expiresAt: number | undefined,
  length: number,
  now: number,
): TimerBucket {
  if (!expiresAt) return '60';

  const remaining = expiresAt - now;
  if (remaining < 0) return '00';
  if (remaining > length) return '60';

  const bucket = Math.round((remaining / length) * 12);
  return String(bucket * 5).padStart(2, '0') as TimerBucket;
}

/** A clock waiting to be told to look at the time again. */
type Subscriber = {
  /** The message's full lifetime, in milliseconds. */
  length: number;
  notify: () => void;
};

const subscribers = new Set<Subscriber>();
let interval: ReturnType<typeof setInterval> | null = null;
let period = 0;

/*
 * The instant every dial reads, updated only when the ticker fires.
 *
 * `useSyncExternalStore` requires the snapshot to be stable between calls
 * within a render — React calls it more than once and treats a changed value as
 * a signal to render again. A snapshot derived from `Date.now()` is stable only
 * by luck: on a bucket boundary two calls a microsecond apart disagree, and
 * React re-renders until they stop, which is a loop nobody can see coming.
 *
 * Reading a shared instant fixes that and is more correct besides — every clock
 * on screen is now showing the same moment rather than thirteen of them.
 */
let sharedNow = Date.now();

/** The instant the dials are currently reading. */
export function timerNow(): number {
  return sharedNow;
}

/**
 * The cadence the ticker should run at: the finest any subscriber asked for.
 *
 * A one-minute timer and a one-year timer on screen together are served by the
 * one-minute timer's cadence. The year-long one recomputes its bucket far more
 * often than it needs to and answers "unchanged" every time, which costs a
 * subtraction — much less than a second interval would.
 */
function wantedPeriod(): number {
  let finest = Number.POSITIVE_INFINITY;
  for (const subscriber of subscribers) {
    finest = Math.min(finest, timerIncrement(subscriber.length));
  }
  return Math.max(MINIMUM_INTERVAL, finest);
}

function tick(): void {
  sharedNow = Date.now();
  for (const subscriber of subscribers) subscriber.notify();
}

function reschedule(): void {
  // A hidden window has nothing to redraw. Chromium throttles background
  // timers anyway, but not to zero, and this is zero.
  const wanted =
    subscribers.size === 0 || (typeof document !== 'undefined' && document.hidden)
      ? 0
      : wantedPeriod();

  if (wanted === period) return;

  if (interval !== null) {
    clearInterval(interval);
    interval = null;
  }

  period = wanted;
  if (wanted > 0) interval = setInterval(tick, wanted);
}

if (typeof document !== 'undefined') {
  document.addEventListener('visibilitychange', () => {
    // Catch up on the way back: the dials are stale by however long the window
    // was hidden, and waiting a whole period to correct them is visible.
    if (!document.hidden) tick();
    reschedule();
  });
}

/**
 * Subscribe to the shared ticker. Returns the unsubscribe function.
 *
 * Shaped for `useSyncExternalStore`, which is what gives us the bail-out: it
 * compares the value the component reads and skips the render when it has not
 * moved.
 */
export function subscribeToTimer(length: number, notify: () => void): () => void {
  // With nothing subscribed the ticker is stopped, so the shared instant can be
  // arbitrarily stale by the time the first clock appears.
  if (subscribers.size === 0) sharedNow = Date.now();

  const subscriber: Subscriber = { length, notify };
  subscribers.add(subscriber);
  reschedule();

  return () => {
    subscribers.delete(subscriber);
    reschedule();
  };
}

/** How many clocks are currently being driven. Exported for tests. */
export function activeTimerCount(): number {
  return subscribers.size;
}

/** The ticker's current period in milliseconds, or 0 when stopped. For tests. */
export function timerPeriod(): number {
  return period;
}
