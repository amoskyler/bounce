/**
 * The disappearing-message clock.
 *
 * The bucket arithmetic is lifted from Signal's `ts/util/timer.std.ts`, and the
 * point of these tests is that it stays lifted: the dial has thirteen
 * positions, they are named as minutes on a clock face, and the boundaries fall
 * where `Math.round` puts them. Rounding where Signal rounds is what makes the
 * hand sit in the same place theirs does; rounding where it floors puts every
 * message half a step out for its whole life.
 *
 * Bundle and run:
 *
 *   npx esbuild src/renderer/__tests__/expire-timer.test.ts --bundle \
 *     --platform=node --format=cjs --outfile=/tmp/expire-timer.test.cjs \
 *     && node --test /tmp/expire-timer.test.cjs
 */

import assert from 'node:assert/strict';
import test from 'node:test';

import {
  activeTimerCount,
  subscribeToTimer,
  timerBucket,
  timerIncrement,
  timerNow,
  timerPeriod,
} from '../expire-timer';

const MINUTE = 60_000;

/* --------------------------------------------------------------------------
 * Buckets
 * -------------------------------------------------------------------------- */

test('a full dial reads 60 and an expired one reads 00', () => {
  const length = 10 * MINUTE;
  const now = 1_000_000;

  assert.equal(timerBucket(now + length, length, now), '60');
  assert.equal(timerBucket(now, length, now), '00');
});

test('the dial has thirteen positions, five apart', () => {
  const length = 12_000;
  // Deliberately not zero: an expiry of zero means "never expires" rather than
  // "expired", which is Signal's convention and worth not tripping over here.
  const now = 500_000;

  // One reading per twelfth of the message's life, sampled at the middle of
  // each so rounding cannot land on a boundary.
  const seen = new Set<string>();
  for (let step = 0; step <= 24; step += 1) {
    seen.add(timerBucket(now + (length * step) / 24, length, now));
  }

  assert.deepEqual(
    [...seen].sort(),
    ['00', '05', '10', '15', '20', '25', '30', '35', '40', '45', '50', '55', '60'],
  );
});

test('the bucket is the remaining fraction rounded to a twelfth', () => {
  const length = 1_200;
  const now = 500_000;

  // Signal: Math.round((remaining / length) * 12) * 5, zero padded.
  for (const [remaining, expected] of [
    [1_200, '60'],
    [1_100, '55'],
    [600, '30'],
    [100, '05'],
    [0, '00'],
  ] as const) {
    assert.equal(timerBucket(now + remaining, length, now), expected, `${remaining}ms left`);
  }
});

test('rounding is to nearest, not down', () => {
  const length = 1_200;
  const now = 500_000;

  // 549/1200 is 5.49 twelfths, which rounds to 5 → "25". Flooring would give
  // the same here, so the case that matters is just above the halfway point.
  assert.equal(timerBucket(now + 549, length, now), '25');
  assert.equal(timerBucket(now + 551, length, now), '30', 'should round up, not down');
});

test('a dial past its expiry reads empty rather than going negative', () => {
  assert.equal(timerBucket(1_000, 10_000, 5_000), '00');
  assert.equal(timerBucket(1_000, 10_000, 9_999_999), '00');
});

test('a clock with more time left than its own length is full', () => {
  // Happens when the retention is shortened after a message was sent: the
  // stored expiry still reflects the old, longer setting.
  assert.equal(timerBucket(100_000, 1_000, 0), '60');
});

test('no expiry at all is a full dial', () => {
  assert.equal(timerBucket(undefined, 10_000, 0), '60');
  assert.equal(timerBucket(0, 10_000, 0), '60');
});

/* --------------------------------------------------------------------------
 * Cadence
 * -------------------------------------------------------------------------- */

test('a timer redraws twelve times over its life', () => {
  // One redraw per position on the dial: any more is wasted, any fewer skips.
  assert.equal(timerIncrement(12_000), 1_000);
  assert.equal(timerIncrement(60_000), 5_000);
});

test('an odd length rounds its increment up, never to zero', () => {
  assert.equal(timerIncrement(13), 2);
  assert.equal(timerIncrement(1), 1);
  assert.equal(timerIncrement(0), 0);
  assert.equal(timerIncrement(-5), 1_000, 'a negative length is not a cadence');
});

/* --------------------------------------------------------------------------
 * The shared ticker
 * -------------------------------------------------------------------------- */

test('no clocks on screen means no timer running at all', () => {
  assert.equal(activeTimerCount(), 0);
  assert.equal(timerPeriod(), 0, 'an interval is running with nothing to drive');
});

test('the ticker starts with the first clock and stops with the last', () => {
  const first = subscribeToTimer(60_000, () => {});
  assert.equal(activeTimerCount(), 1);
  assert.ok(timerPeriod() > 0, 'the ticker should be running');

  const second = subscribeToTimer(60_000, () => {});
  assert.equal(activeTimerCount(), 2);

  first();
  assert.equal(activeTimerCount(), 1);
  assert.ok(timerPeriod() > 0, 'still one clock to drive');

  second();
  assert.equal(activeTimerCount(), 0);
  assert.equal(timerPeriod(), 0, 'the ticker should have stopped');
});

test('the ticker runs at the finest cadence any clock asked for', () => {
  // A year-long timer and a minute-long one on screen together are served by
  // the minute; one interval covers both.
  const year = subscribeToTimer(365 * 24 * 60 * MINUTE, () => {});
  const coarse = timerPeriod();

  const minute = subscribeToTimer(MINUTE, () => {});
  assert.ok(timerPeriod() < coarse, 'adding a short timer should quicken the ticker');
  assert.equal(timerPeriod(), 5_000, 'a minute divided into twelve');

  // And it relaxes again when the short one goes.
  minute();
  assert.equal(timerPeriod(), coarse);
  year();
});

test('the cadence never drops below half a second', () => {
  // A one-second timer would otherwise ask to be redrawn every 84ms, which is
  // a repaint budget spent on a dial 12 pixels across.
  const stop = subscribeToTimer(1_000, () => {});
  assert.equal(timerPeriod(), 500);
  stop();
});

test('every clock is notified on a tick', async () => {
  const notified: string[] = [];
  const a = subscribeToTimer(6_000, () => notified.push('a'));
  const b = subscribeToTimer(6_000, () => notified.push('b'));

  await new Promise((resolve) => setTimeout(resolve, 620));

  assert.ok(notified.includes('a'), 'the first clock was never told');
  assert.ok(notified.includes('b'), 'the second clock was never told');

  a();
  b();
});

test('an unsubscribed clock stops being notified', async () => {
  let ticks = 0;
  const stop = subscribeToTimer(6_000, () => {
    ticks += 1;
  });

  await new Promise((resolve) => setTimeout(resolve, 620));
  const before = ticks;
  assert.ok(before > 0, 'never ticked at all');

  stop();
  await new Promise((resolve) => setTimeout(resolve, 620));
  assert.equal(ticks, before, 'kept ticking after unsubscribing');
});

test('the instant a dial reads is stable between ticks', () => {
  // `useSyncExternalStore` calls the snapshot more than once per render and
  // treats a changed value as a reason to render again. Reading the wall clock
  // each time makes that a coin flip on every bucket boundary.
  const stop = subscribeToTimer(60_000, () => {});
  const first = timerNow();
  for (let spin = 0; spin < 100_000; spin += 1) Math.sqrt(spin);
  assert.equal(timerNow(), first, 'the shared instant moved mid-render');
  stop();
});

test('the instant is refreshed when the first clock appears', async () => {
  // With nothing subscribed the ticker is stopped, so the shared instant goes
  // stale — a clock mounting into that would open on the wrong bucket.
  assert.equal(activeTimerCount(), 0);
  await new Promise((resolve) => setTimeout(resolve, 30));

  const before = Date.now();
  const stop = subscribeToTimer(60_000, () => {});
  assert.ok(timerNow() >= before, 'a stale instant was carried into a new clock');
  stop();
});
