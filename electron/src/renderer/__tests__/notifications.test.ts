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
  measuredRowHeight,
  type RangeInputs,
  type VisibleRange,
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
 * ---------------------------------------------------------------------- */

const THREAD: RangeInputs = {
  scrollTop: 0,
  viewportHeight: 600,
  count: 10_000,
  rowHeight: 56,
  overscanRows: 10,
};

/** The scroll height the caller's container will report for a range. */
function totalHeight(range: VisibleRange, rowHeight: number): number {
  return range.topSpacer + (range.end - range.start) * rowHeight + range.bottomSpacer;
}

test('a ten thousand message thread windows down to a screenful of rows', () => {
  // The whole point: mounting ten thousand rows is what this replaces.
  const range = computeVisibleRange({ ...THREAD, scrollTop: 280_000 });
  assert.ok(range.end - range.start < 100, `windowed ${range.end - range.start} rows`);
});

test('the spacers and the rendered rows always add up to the full list height', () => {
  // If they did not, the scrollbar would misreport the length of the thread and
  // the position would jump as the window moved.
  for (const scrollTop of [0, 1_000, 280_000, 559_400]) {
    const range = computeVisibleRange({ ...THREAD, scrollTop });
    assert.equal(totalHeight(range, THREAD.rowHeight), THREAD.count * THREAD.rowHeight);
  }
});

test('the window covers the viewport even when every row is twice its estimate', () => {
  // Rows are variable: a wrapped bubble is taller than the estimate. The
  // overscan has to absorb that, or the reader sees blank space below the fold.
  const range = computeVisibleRange({ ...THREAD, scrollTop: 280_000 });
  const realHeight = (range.end - range.start) * THREAD.rowHeight * 2;
  assert.ok(realHeight > THREAD.viewportHeight * 2, `covered only ${realHeight}px`);
});

test('a container that has not been laid out yet still renders rows', () => {
  // A zero height is a container mid-mount or hidden. Believing it would mount
  // nothing, and the list would come up blank and stay that way.
  const range = computeVisibleRange({ ...THREAD, viewportHeight: 0 });
  assert.ok(range.end - range.start > 0);
});

test('a scroll position past the end of the list still renders the tail', () => {
  // Happens when the thread shrinks under the reader — a retention sweep, or a
  // conversation switched while scrolled far down.
  const range = computeVisibleRange({ ...THREAD, scrollTop: 99_000_000 });
  assert.equal(range.end, THREAD.count);
  assert.ok(range.start < range.end);
});

test('an empty list has no window and no spacers', () => {
  assert.deepEqual(computeVisibleRange({ ...THREAD, count: 0 }), {
    start: 0,
    end: 0,
    topSpacer: 0,
    bottomSpacer: 0,
  });
});

test('a list shorter than the window renders every row', () => {
  const range = computeVisibleRange({ ...THREAD, count: 4 });
  assert.deepEqual(range, { start: 0, end: 4, topSpacer: 0, bottomSpacer: 0 });
});

test('a nonsense row height cannot produce an empty window', () => {
  // Division by a zero or negative height is how a windowing bug turns into a
  // blank conversation.
  for (const rowHeight of [0, -20]) {
    const range = computeVisibleRange({ ...THREAD, rowHeight });
    assert.ok(range.end > range.start, `empty window at row height ${rowHeight}`);
  }
});

/* -------------------------------------------------------------------------
 * Measurement
 * ---------------------------------------------------------------------- */

const RENDERED: VisibleRange = { start: 100, end: 140, topSpacer: 5_600, bottomSpacer: 5_600 };

test('a measurement close to the working estimate is ignored', () => {
  // Adopting every measurement moves both spacers, which moves the content
  // under a fixed scroll position — the list would twitch as it scrolled.
  const scrollHeight = RENDERED.topSpacer + 40 * 58 + RENDERED.bottomSpacer;
  assert.equal(measuredRowHeight(scrollHeight, RENDERED, 56), null);
});

test('a measurement far from the working estimate is adopted', () => {
  // Rows twice the estimate mean the spacers are half the height they should
  // be, and the scrollbar is lying about the length of the thread.
  const scrollHeight = RENDERED.topSpacer + 40 * 112 + RENDERED.bottomSpacer;
  assert.equal(measuredRowHeight(scrollHeight, RENDERED, 56), 112);
});

test('a handful of rows is not a large enough sample to measure', () => {
  // The measured block also holds the container's padding and whatever else
  // the caller renders in the scroller; over a few rows that overhead is most
  // of what is being measured.
  const few: VisibleRange = { start: 0, end: 3, topSpacer: 0, bottomSpacer: 0 };
  assert.equal(measuredRowHeight(900, few, 56), null);
});

test('a container reporting nothing rendered is not a measurement', () => {
  // Mid-layout the spacers can add up to more than the scroll height, and a
  // negative row height would poison every window after it.
  assert.equal(measuredRowHeight(RENDERED.topSpacer, RENDERED, 56), null);
});
