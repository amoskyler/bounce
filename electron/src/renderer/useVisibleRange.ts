/**
 * Windowing for long lists.
 *
 * The Fyne client's timeline is a `widget.List`, which renders only the rows
 * intersecting the viewport and reuses them as it scrolls; a thread of ten
 * thousand messages costs the same as a thread of twenty. React has no such
 * widget, so this hook does the arithmetic and the caller renders a slice
 * between two spacers that stand in for the rows left out.
 *
 * Message rows are not a fixed height — a bubble wraps, a date separator sits
 * between days — so the arithmetic can only ever be an estimate. Every choice
 * here is therefore made in the direction of rendering too much: a window that
 * overshoots costs a few nodes, while one that falls short shows blank space
 * where the conversation should be.
 */

import * as React from 'react';

/** The slice to render, and the heights standing in for what is not rendered. */
export type VisibleRange = {
  /** First index to render, inclusive. */
  start: number;
  /** One past the last index to render. */
  end: number;
  /** Height of the spacer above the rendered rows, in pixels. */
  topSpacer: number;
  /** Height of the spacer below the rendered rows, in pixels. */
  bottomSpacer: number;
};

/** The measurements {@link computeVisibleRange} works from. */
export type RangeInputs = {
  scrollTop: number;
  /** The container's visible height. Zero means "not laid out yet". */
  viewportHeight: number;
  count: number;
  /** Best current guess at the height of one row, including any gap below it. */
  rowHeight: number;
  /** Extra rows to keep mounted beyond a viewport's worth at each end. */
  overscanRows: number;
};

/**
 * How many rows stay mounted no matter what the arithmetic says.
 *
 * A container whose height we could not read, or a row height measured badly,
 * would otherwise be able to produce a one-row window. Thirty rows is cheap and
 * covers any plausible viewport on its own.
 */
const MINIMUM_ROWS = 30;

/** The viewport height assumed before the container has been laid out. */
const ASSUMED_VIEWPORT = 800;

/** Rows kept beyond a viewport's worth at each end, unless the caller says otherwise. */
const DEFAULT_OVERSCAN_ROWS = 10;

/**
 * Rendered rows needed before a measurement is trusted.
 *
 * The measurement divides the rendered block's height by the number of rows in
 * it, and that block also contains the container's padding and whatever else
 * the caller puts in the scroller. Over a handful of rows that fixed overhead
 * dominates; over a screenful it disappears into the rounding.
 */
const MINIMUM_SAMPLE_ROWS = 8;

/** How far a measurement must differ from the working estimate to be adopted. */
const MEASUREMENT_TOLERANCE = 0.1;

/**
 * The rows to render for a given scroll position.
 *
 * Pure, so the windowing can be checked without a DOM: the whole of the
 * interesting behaviour — the overscan, the clamping, the floor on the window
 * size — is decided here.
 */
export function computeVisibleRange({
  scrollTop,
  viewportHeight,
  count,
  rowHeight,
  overscanRows,
}: RangeInputs): VisibleRange {
  const rows = Math.max(0, Math.floor(count));
  if (rows === 0) {
    return { start: 0, end: 0, topSpacer: 0, bottomSpacer: 0 };
  }

  const height = Math.max(1, rowHeight);

  // A container measured at zero is one that has not been laid out yet, not one
  // with nothing in it, so guess rather than window down to nothing.
  const viewport = viewportHeight > 0 ? viewportHeight : ASSUMED_VIEWPORT;

  // A whole viewport of overscan at each end is what absorbs rows that turn out
  // to be taller than the estimate: the window has to be wrong by more than a
  // screenful before anything visible is missing.
  const overscan = Math.max(viewport, Math.max(0, overscanRows) * height);

  let start = clamp(Math.floor((scrollTop - overscan) / height), 0, rows);
  let end = clamp(Math.ceil((scrollTop + viewport + overscan) / height), 0, rows);

  if (end - start < MINIMUM_ROWS) {
    end = Math.min(rows, start + MINIMUM_ROWS);
    start = Math.max(0, end - MINIMUM_ROWS);
  }

  return {
    start,
    end,
    topSpacer: start * height,
    bottomSpacer: (rows - end) * height,
  };
}

/**
 * The row height implied by what is actually on screen, or null to keep the
 * current estimate.
 *
 * Adopting every measurement would make the list twitch: changing the estimate
 * moves both spacers, which moves the content under a fixed scroll position. So
 * a measurement is only adopted when it is far enough out to be worth the
 * disturbance, which settles in a step or two and then stops.
 */
export function measuredRowHeight(
  scrollHeight: number,
  range: VisibleRange,
  currentRowHeight: number,
): number | null {
  const rendered = range.end - range.start;
  if (rendered < MINIMUM_SAMPLE_ROWS) return null;

  const renderedHeight = scrollHeight - range.topSpacer - range.bottomSpacer;
  if (renderedHeight <= 0) return null;

  const average = renderedHeight / rendered;
  if (Math.abs(average - currentRowHeight) <= currentRowHeight * MEASUREMENT_TOLERANCE) {
    return null;
  }

  return average;
}

/**
 * Window a scrollable list down to the rows near the viewport.
 *
 * The caller renders `messages.slice(start, end)` between two spacer elements
 * of `topSpacer` and `bottomSpacer` pixels, and the scroller behaves as though
 * every row were mounted. The hook listens on the container itself, so no
 * scroll handler needs to be threaded through; an existing `onScroll` on the
 * same element keeps working untouched.
 *
 * @param containerRef the scrolling element
 * @param count how many rows the list has in total
 * @param estimatedRowHeight the starting guess, refined from the DOM as rows render
 * @param overscanRows extra rows beyond a viewport's worth at each end
 */
export function useVisibleRange(
  containerRef: React.RefObject<HTMLElement | null>,
  count: number,
  estimatedRowHeight: number,
  overscanRows: number = DEFAULT_OVERSCAN_ROWS,
): VisibleRange {
  const rowHeightRef = React.useRef(Math.max(1, estimatedRowHeight));

  // A conversation opens pinned to its newest message, so the first window —
  // drawn before there is a scroll position to read — is the tail of the list.
  // Starting at the head would show a screenful of ancient history and then
  // replace it.
  const [range, setRange] = React.useState<VisibleRange>(() =>
    computeVisibleRange({
      scrollTop: count * rowHeightRef.current,
      viewportHeight: 0,
      count,
      rowHeight: rowHeightRef.current,
      overscanRows,
    }),
  );

  // The measurement needs the spacers that produced the current DOM, and the
  // scroll listener must not be torn down and rebuilt on every new message, so
  // both the range and the update function are read through refs.
  const rangeRef = React.useRef(range);
  React.useLayoutEffect(() => {
    rangeRef.current = range;
  }, [range]);

  const update = React.useCallback(() => {
    const element = containerRef.current;
    if (!element) return;

    const viewportHeight = element.clientHeight;
    // A container with no height is one that is hidden or not yet laid out. It
    // tells us nothing, and keeping the last window is what stops a resize —
    // or a panel being collapsed and reopened — leaving the list blank.
    if (viewportHeight === 0) return;

    const measured = measuredRowHeight(
      element.scrollHeight,
      rangeRef.current,
      rowHeightRef.current,
    );
    if (measured !== null) rowHeightRef.current = measured;

    const next = computeVisibleRange({
      scrollTop: element.scrollTop,
      viewportHeight,
      count,
      rowHeight: rowHeightRef.current,
      overscanRows,
    });

    setRange((current) => (isSameRange(current, next) ? current : next));
  }, [containerRef, count, overscanRows]);

  const updateRef = React.useRef(update);
  React.useLayoutEffect(() => {
    updateRef.current = update;
  }, [update]);

  // Before paint, so a window computed for the previous message count is never
  // the one the reader sees.
  React.useLayoutEffect(() => {
    update();
  }, [update]);

  React.useEffect(() => {
    const element = containerRef.current;
    if (!element) return;

    // Scroll fires far more often than a frame; recomputing per frame is both
    // enough to keep up and the most a render could use.
    let frame = 0;
    const schedule = () => {
      if (frame !== 0) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        updateRef.current();
      });
    };

    element.addEventListener('scroll', schedule, { passive: true });
    const observer = new ResizeObserver(schedule);
    observer.observe(element);

    return () => {
      if (frame !== 0) cancelAnimationFrame(frame);
      element.removeEventListener('scroll', schedule);
      observer.disconnect();
    };
  }, [containerRef]);

  return range;
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(high, Math.max(low, value));
}

function isSameRange(a: VisibleRange, b: VisibleRange): boolean {
  return (
    a.start === b.start &&
    a.end === b.end &&
    a.topSpacer === b.topSpacer &&
    a.bottomSpacer === b.bottomSpacer
  );
}
