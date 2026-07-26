/**
 * Tests for the notification rules and the timeline windowing.
 *
 * Both are pure arithmetic over a context, and both fail in ways that are hard
 * to see in a running client: a suppression rule that is subtly wrong produces
 * a banner for a message the reader is already looking at, and a window that is
 * subtly wrong produces blank space where the conversation should be. Neither
 * shows up as an error.
 *
 * Bundle and run:
 *
 *   npx esbuild src/renderer/__tests__/notifications.test.ts --bundle \
 *     --platform=node --format=cjs --loader:.css=empty \
 *     --outfile=/tmp/notifications.test.cjs && node --test /tmp/notifications.test.cjs
 */

import assert from 'node:assert/strict';
import test from 'node:test';

import {
  MUTED_FOREVER,
  isMuted,
  messageNotification,
  setTimelineAtBottom,
  shouldNotify,
  timelineIsAtBottom,
  type NotificationContext,
} from '../notifications';
import {
  computeVisibleRange,
  createMetrics,
  offsetOf,
  resizeMetrics,
  totalHeight,
  withMeasurements,
  type RowMetrics,
} from '../useVisibleRange';

const NOW = 1_700_000_000;

/** The context for a message that has every reason to interrupt the reader. */
function arriving(overrides: Partial<NotificationContext> = {}): NotificationContext {
  return {
    conversationId: 'thread-a',
    outgoing: false,
    mutedUntil: 0,
    windowFocused: false,
    activeConversation: null,
    atBottom: false,
    now: NOW,
    ...overrides,
  };
}

/* -------------------------------------------------------------------------
 * Suppression
 * ---------------------------------------------------------------------- */

test('a message this device sent never notifies', () => {
  // Our own messages come back through the same event, echoed from our other
  // devices. Notifying on them would mean a banner for everything we type.
  assert.equal(shouldNotify(arriving({ outgoing: true })), false);
});

test('a message in the conversation being watched never notifies', () => {
  // The reader is looking at the timeline it landed in, pinned to the bottom,
  // so the message announced itself by appearing.
  assert.equal(
    shouldNotify(
      arriving({ windowFocused: true, activeConversation: 'thread-a', atBottom: true }),
    ),
    false,
  );
});

test('a conversation scrolled back through history still notifies', () => {
  // The timeline will not move, so nothing on screen says the message arrived.
  assert.equal(
    shouldNotify(
      arriving({ windowFocused: true, activeConversation: 'thread-a', atBottom: false }),
    ),
    true,
  );
});

test('an unfocused window notifies even for the open conversation', () => {
  // The conversation being on screen behind another application is not the
  // same as it being read.
  assert.equal(
    shouldNotify(
      arriving({ windowFocused: false, activeConversation: 'thread-a', atBottom: true }),
    ),
    true,
  );
});

test('a message in another conversation notifies while one is open', () => {
  // The suppression is per thread, not per window: this is the case the whole
  // rule exists to leave alone.
  assert.equal(
    shouldNotify(
      arriving({ windowFocused: true, activeConversation: 'thread-b', atBottom: true }),
    ),
    true,
  );
});

test('a conversation muted forever never notifies', () => {
  // MUTED_FOREVER is negative, so a client that only compared it against the
  // clock would treat "muted forever" as "the mute expired long ago".
  assert.equal(shouldNotify(arriving({ mutedUntil: MUTED_FOREVER })), false);
  assert.equal(isMuted(MUTED_FOREVER, NOW), true);
});

test('a mute that has run out stops suppressing', () => {
  assert.equal(shouldNotify(arriving({ mutedUntil: NOW + 60 })), false);
  assert.equal(shouldNotify(arriving({ mutedUntil: NOW - 60 })), true);
  // The boundary: a mute expiring exactly now is over.
  assert.equal(isMuted(NOW, NOW), false);
});

test('a catch up sync suppresses the whole backlog', () => {
  // A device that has been offline for a day receives its missed messages in a
  // burst. One banner each would be unusable.
  assert.equal(shouldNotify(arriving({ syncing: true })), false);
});

test('a thread that has never been rendered counts as not at the bottom', () => {
  // The safe default: not knowing where a timeline is scrolled must err
  // towards notifying, never towards silence.
  assert.equal(timelineIsAtBottom('never-opened'), false);

  setTimelineAtBottom('never-opened', true);
  assert.equal(timelineIsAtBottom('never-opened'), true);
  setTimelineAtBottom('never-opened', false);
  assert.equal(timelineIsAtBottom('never-opened'), false);
});

/* -------------------------------------------------------------------------
 * Content
 * ---------------------------------------------------------------------- */

test('a direct message is titled with its author', () => {
  assert.deepEqual(
    messageNotification(
      { text: 'on my way', attachments: [] },
      { authorName: 'Ada', conversationName: 'Ada', isGroup: false },
    ),
    { title: 'Ada', body: 'on my way' },
  );
});

test('a group message names the group and prefixes its author', () => {
  // Without the prefix a group notification says nothing about who spoke,
  // which is the first thing a reader wants from one.
  assert.deepEqual(
    messageNotification(
      { text: 'on my way', attachments: [] },
      { authorName: 'Ada', conversationName: 'Cycling', isGroup: true },
    ),
    { title: 'Cycling', body: 'Ada: on my way' },
  );
});

test('a message with only attachments still says something', () => {
  // The wording comes from the same helper as the conversation list preview,
  // so the two cannot drift apart.
  assert.deepEqual(
    messageNotification(
      { text: '', attachments: [{}, {}] },
      { authorName: 'Ada', conversationName: 'Ada', isGroup: false },
    ),
    { title: 'Ada', body: '2 attachments' },
  );
});

test('a message with nothing in it falls back rather than showing a blank body', () => {
  assert.deepEqual(
    messageNotification(
      { text: '   ', attachments: [] },
      { authorName: 'Ada', conversationName: 'Ada', isGroup: false },
    ),
    { title: 'Ada', body: 'New message' },
  );
});

/* -------------------------------------------------------------------------
 * Windowing
 *
 * The rows in a timeline are not one height: a date separator is around 30px,
 * a one-line bubble 56, a photograph over four hundred. Every test below that
 * mixes heights is there because a single global average got it wrong — the
 * estimate followed whatever was on screen, and revising it resized the
 * spacers and slid the conversation under the reader.
 * ---------------------------------------------------------------------- */

const ROW = 56;
const VIEWPORT = 600;

/** A table of `count` rows, none of them measured yet. */
function fresh(count = 10_000): RowMetrics {
  return createMetrics(count, ROW);
}

/** Measure every row in a range at one height, as a screenful would. */
function measureAll(metrics: RowMetrics, from: number, to: number, height: number): RowMetrics {
  const measurements: [number, number][] = [];
  for (let index = from; index < to; index += 1) measurements.push([index, height]);
  return withMeasurements(metrics, measurements, ROW);
}

function window(metrics: RowMetrics, scrollTop: number, viewportHeight = VIEWPORT) {
  return computeVisibleRange({ scrollTop, viewportHeight, metrics, overscanRows: 10 });
}

test('a ten thousand message thread windows down to a screenful of rows', () => {
  // The whole point: mounting ten thousand rows is what this replaces.
  const range = window(fresh(), 280_000);
  assert.ok(range.end - range.start < 100, `windowed ${range.end - range.start} rows`);
});

test('the spacers and the rendered rows always add up to the full list height', () => {
  // If they did not, the scrollbar would misreport the length of the thread and
  // the position would jump as the window moved.
  const metrics = fresh();
  for (const scrollTop of [0, 1_000, 280_000, 559_400]) {
    const range = window(metrics, scrollTop);
    const rendered = offsetOf(metrics, range.end) - offsetOf(metrics, range.start);
    assert.equal(range.topSpacer + rendered + range.bottomSpacer, totalHeight(metrics));
  }
});

test('the window covers the viewport even when every row is twice its estimate', () => {
  // The overscan has to absorb rows taller than expected, or the reader sees
  // blank space below the fold before the next window is computed.
  const metrics = fresh();
  const range = window(metrics, 280_000);
  const realHeight = (range.end - range.start) * ROW * 2;
  assert.ok(realHeight > VIEWPORT * 2, `covered only ${realHeight}px`);
});

test('a container that has not been laid out yet still renders rows', () => {
  // A zero height is a container mid-mount or hidden. Believing it would mount
  // nothing, and the list would come up blank and stay that way.
  assert.ok(window(fresh(), 0, 0).end > 0);
});

test('a scroll position past the end of the list still renders the tail', () => {
  // Happens when the thread shrinks under the reader — a retention sweep, or a
  // conversation switched while scrolled far down.
  const range = window(fresh(), 99_000_000);
  assert.equal(range.end, 10_000);
  assert.ok(range.start < range.end);
});

test('an empty list has no window and no spacers', () => {
  assert.deepEqual(window(fresh(0), 0), {
    start: 0,
    end: 0,
    topSpacer: 0,
    bottomSpacer: 0,
  });
});

test('a list shorter than the window renders every row', () => {
  assert.deepEqual(window(fresh(4), 0), { start: 0, end: 4, topSpacer: 0, bottomSpacer: 0 });
});

test('a nonsense estimate cannot produce an empty window', () => {
  // Division by a zero or negative height is how a windowing bug turns into a
  // blank conversation.
  for (const estimate of [0, -20]) {
    const range = window(createMetrics(10_000, estimate), 0);
    assert.ok(range.end > range.start, `empty window at estimate ${estimate}`);
  }
});

test('the window brackets the viewport rather than starting inside it', () => {
  // An off-by-one at either edge shows as a sliver of blank space at the top or
  // bottom of the scroller, which is exactly where it is least visible in a
  // screenshot and most obvious in use.
  const metrics = measureAll(fresh(500), 0, 500, ROW);
  const range = window(metrics, 10_000);
  assert.ok(offsetOf(metrics, range.start) <= 10_000, 'window starts below the viewport top');
  assert.ok(
    offsetOf(metrics, range.end) >= 10_000 + VIEWPORT,
    'window ends above the viewport bottom',
  );
});

/* -------------------------------------------------------------------------
 * Per-row heights
 * ---------------------------------------------------------------------- */

test('a measured row keeps its own height instead of an average', () => {
  // The failure this prevents: one 450px photograph among 56px bubbles used to
  // drag the estimate for every other row up with it.
  let metrics = fresh(100);
  metrics = withMeasurements(metrics, [[10, 450]], ROW);

  assert.equal(metrics.known[10], 450);
  // Row 11 sits after the tall one, not after an average of it.
  assert.equal(offsetOf(metrics, 11) - offsetOf(metrics, 10), 450);
});

test('unmeasured rows are priced at the mean of the measured ones', () => {
  let metrics = fresh(100);
  metrics = measureAll(metrics, 0, 10, 80);

  assert.equal(metrics.average, 80);
  assert.equal(offsetOf(metrics, 10), 800);
  assert.equal(totalHeight(metrics), 800 + 90 * 80);
});

test('a tall row moves the average by its share and no more', () => {
  let metrics = fresh(100);
  metrics = measureAll(metrics, 0, 9, 50);
  metrics = withMeasurements(metrics, [[9, 500]], ROW);

  // Nine rows at 50 and one at 500: the mean is 95, not 500.
  assert.equal(metrics.average, 95);
  // And the nine measured rows are still exactly 50 each.
  assert.equal(offsetOf(metrics, 9), 450);
});

test('re-measuring a row at the same height changes nothing', () => {
  // Identity is what the hook uses to decide whether a scroll correction is
  // owed, so a no-op measurement must not look like a change.
  const metrics = measureAll(fresh(100), 0, 20, 60);
  assert.equal(withMeasurements(metrics, [[5, 60]], ROW), metrics);
  assert.equal(withMeasurements(metrics, [[5, 60.2]], ROW), metrics);
  assert.notEqual(withMeasurements(metrics, [[5, 90]], ROW), metrics);
});

test('a row measured at zero is ignored', () => {
  // A hidden or mid-layout row reports zero, and believing it would collapse
  // the offsets of everything below.
  const metrics = measureAll(fresh(100), 0, 20, 60);
  assert.equal(withMeasurements(metrics, [[5, 0]], ROW), metrics);
});

test('measurements outside the list are dropped rather than growing it', () => {
  const metrics = fresh(10);
  assert.equal(withMeasurements(metrics, [[-1, 90], [999, 90]], ROW), metrics);
  assert.equal(metrics.known.length, 10);
});

test('offsets are clamped to the ends of the list', () => {
  const metrics = measureAll(fresh(10), 0, 10, 40);
  assert.equal(offsetOf(metrics, -5), 0);
  assert.equal(offsetOf(metrics, 999), 400);
});

test('growing the list keeps the heights already measured', () => {
  // New messages arrive at the end of a thread constantly. Forgetting what the
  // rows above them measured would re-introduce the jump on every arrival.
  const metrics = measureAll(fresh(100), 0, 100, 70);
  const grown = resizeMetrics(metrics, 101, ROW);

  assert.equal(grown.known.length, 101);
  assert.equal(grown.known[0], 70);
  assert.equal(grown.known[100], 0, 'the new row has not been measured');
  assert.equal(offsetOf(grown, 100), 7_000);
});

test('shrinking the list drops the heights past its new end', () => {
  const metrics = measureAll(fresh(100), 0, 100, 70);
  const shrunk = resizeMetrics(metrics, 10, ROW);

  assert.equal(shrunk.known.length, 10);
  assert.equal(totalHeight(shrunk), 700);
});

test('a shift moves measured heights to their new indices', () => {
  // History loaded at the front of a thread renumbers every row below it.
  let metrics = fresh(10);
  metrics = withMeasurements(metrics, [[0, 400]], ROW);

  const shifted = resizeMetrics(metrics, 13, ROW, 3);
  assert.equal(shifted.known[3], 400, 'the tall row moved down by three');
  assert.equal(shifted.known[0], 0, 'the rows in front of it are unmeasured');
});

/* -------------------------------------------------------------------------
 * Stability
 * ---------------------------------------------------------------------- */

test('scrolling through a run of tall rows does not move the rows above them', () => {
  // The reported bug, reduced: scrolling down past images kept throwing the
  // reader back up. Every row above the window has a known height, so its
  // offset must not move however tall the rows below turn out to be.
  let metrics = fresh(400);
  metrics = measureAll(metrics, 0, 100, 56);

  const anchor = offsetOf(metrics, 50);

  // Now a run of photographs is scrolled into view and measured.
  metrics = measureAll(metrics, 100, 120, 450);

  assert.equal(offsetOf(metrics, 50), anchor, 'a measured row above the window moved');
});

test('the top spacer only changes when a row above the window is re-measured', () => {
  // This is what the hook keys its scroll correction off. If the spacer moved
  // for any other reason the correction would fire when nothing had shifted.
  let metrics = measureAll(fresh(400), 0, 400, 56);
  const before = window(metrics, 10_000);

  // Something far below is re-measured much taller.
  metrics = withMeasurements(metrics, [[380, 450]], ROW);
  const after = window(metrics, 10_000);

  assert.equal(after.start, before.start);
  assert.equal(after.topSpacer, before.topSpacer, 'the spacer above the reader moved');
  assert.ok(after.bottomSpacer > before.bottomSpacer, 'the list did not get taller');
});

test('a mixed thread settles: measuring twice changes nothing the second time', () => {
  // The oscillation was a loop — measure, revise the estimate, resize the
  // spacers, land somewhere else, measure again. Once heights are per-row, a
  // second pass over the same rows is a no-op.
  let metrics = fresh(300);
  const heights = (index: number) => (index % 11 === 5 ? 450 : index % 7 === 3 ? 140 : 56);

  const pass = (from: number, to: number) => {
    const measurements: [number, number][] = [];
    for (let index = from; index < to; index += 1) measurements.push([index, heights(index)]);
    return withMeasurements(metrics, measurements, ROW);
  };

  metrics = pass(0, 60);
  const settled = window(metrics, 2_000);
  const total = totalHeight(metrics);

  const again = pass(0, 60);
  assert.equal(again, metrics, 'a second measurement of the same rows changed the table');
  assert.equal(totalHeight(again), total);
  assert.deepEqual(window(again, 2_000), settled);
});
