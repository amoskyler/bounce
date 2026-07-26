/**
 * Windowing for long lists.
 *
 * The Fyne client's timeline is a `widget.List`, which renders only the rows
 * intersecting the viewport and reuses them as it scrolls; a thread of ten
 * thousand messages costs the same as a thread of twenty. React has no such
 * widget, so this hook does the arithmetic and the caller renders a slice
 * between two spacers that stand in for the rows left out.
 *
 * The rows are not the same height, and not nearly: a one-line bubble is about
 * 56px, a date separator half that, and a photograph over four hundred. An
 * earlier version of this file kept a single average and re-derived it from
 * whatever happened to be on screen — which meant the estimate tracked what you
 * were looking at, and every revision resized both spacers and slid the
 * conversation under the reader. Scrolling down through a run of images could
 * throw you back a thousand pixels, and a message arriving while the list was
 * pinned to the bottom could oscillate for as long as you left it.
 *
 * So heights are remembered per row instead. A row that has been on screen once
 * has an exact height from then on; rows never seen are priced at the average of
 * the ones that have been. That leaves two jobs, both handled here: offsets are
 * a prefix sum rather than a multiplication, and on the occasions when a height
 * does change, the scroll position is corrected by the amount the content above
 * the viewport moved — so what you are reading stays where it is.
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
 * A height table: what each row measured, and what to charge for the rest.
 *
 * `sumKnown` and `countUnknown` are prefix sums, so the offset of any row is
 * two array reads and a multiply. They are rebuilt whenever a measurement
 * lands, which is O(n) — but only once per frame, however many rows were
 * measured in it.
 */
export type RowMetrics = {
  /** Height per row; `0` means never measured. */
  known: number[];
  /** `sumKnown[i]` is the total measured height of rows below `i`. */
  sumKnown: number[];
  /** `countUnknown[i]` is how many rows below `i` have never been measured. */
  countUnknown: number[];
  /** What an unmeasured row is charged: the mean of the measured ones. */
  average: number;
};

/** An empty table of `count` rows, every one priced at `estimate`. */
export function createMetrics(count: number, estimate: number): RowMetrics {
  const rows = Math.max(0, Math.floor(count));
  return rebuild(new Array<number>(rows).fill(0), Math.max(1, estimate));
}

/**
 * Grow or shrink a table to `count` rows, keeping what is already measured.
 *
 * Messages arrive at the end of a thread and history is loaded at the front,
 * but a row's index is its position in the merged list — so an insertion at the
 * top shifts every index below it. Rather than track that, the table is
 * rebuilt: `anchorIndex` is where the caller believes existing rows moved to,
 * and everything else is simply forgotten and re-measured when next on screen.
 */
export function resizeMetrics(
  metrics: RowMetrics,
  count: number,
  fallback: number,
  shift = 0,
): RowMetrics {
  const rows = Math.max(0, Math.floor(count));
  const known = new Array<number>(rows).fill(0);

  for (let index = 0; index < metrics.known.length; index += 1) {
    const moved = index + shift;
    if (moved >= 0 && moved < rows) known[moved] = metrics.known[index];
  }

  return rebuild(known, fallback);
}

/** Record a measured height. Returns a new table, or the same one if unchanged. */
export function withMeasurements(
  metrics: RowMetrics,
  measurements: readonly (readonly [index: number, height: number])[],
  fallback: number,
): RowMetrics {
  let changed = false;
  const known = metrics.known;

  for (const [index, height] of measurements) {
    if (index < 0 || index >= known.length) continue;
    // Sub-pixel churn is not worth a rebuild, and a row measured at zero is one
    // that is hidden rather than one that takes no space.
    if (height <= 0 || Math.abs(known[index] - height) < 0.5) continue;
    if (!changed) changed = true;
    known[index] = height;
  }

  return changed ? rebuild(known.slice(), fallback) : metrics;
}

function rebuild(known: number[], fallback: number): RowMetrics {
  const rows = known.length;
  const sumKnown = new Array<number>(rows + 1);
  const countUnknown = new Array<number>(rows + 1);

  let total = 0;
  let unknown = 0;
  sumKnown[0] = 0;
  countUnknown[0] = 0;

  for (let index = 0; index < rows; index += 1) {
    const height = known[index];
    if (height > 0) total += height;
    else unknown += 1;
    sumKnown[index + 1] = total;
    countUnknown[index + 1] = unknown;
  }

  const measuredRows = rows - unknown;
  return {
    known,
    sumKnown,
    countUnknown,
    // Falling back to the caller's estimate until something has been seen; once
    // anything has, the rows on screen are a better guide than a constant.
    average: measuredRows > 0 ? total / measuredRows : Math.max(1, fallback),
  };
}

/** The distance from the top of the list to the top of row `index`. */
export function offsetOf(metrics: RowMetrics, index: number): number {
  const clamped = clamp(index, 0, metrics.known.length);
  return metrics.sumKnown[clamped] + metrics.countUnknown[clamped] * metrics.average;
}

/** The full scrollable height the list would have with every row mounted. */
export function totalHeight(metrics: RowMetrics): number {
  return offsetOf(metrics, metrics.known.length);
}

/** The lowest index whose offset is at or past `target`. */
function indexAt(metrics: RowMetrics, target: number): number {
  let low = 0;
  let high = metrics.known.length;

  // Offsets are non-decreasing, so a binary search is exact — no scanning, and
  // no dependence on the rows happening to be the same height.
  while (low < high) {
    const middle = (low + high) >> 1;
    if (offsetOf(metrics, middle) < target) low = middle + 1;
    else high = middle;
  }

  return low;
}

/** The measurements {@link computeVisibleRange} works from. */
export type RangeInputs = {
  scrollTop: number;
  /** The container's visible height. Zero means "not laid out yet". */
  viewportHeight: number;
  metrics: RowMetrics;
  /** Extra rows to keep mounted beyond a viewport's worth at each end. */
  overscanRows: number;
};

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
  metrics,
  overscanRows,
}: RangeInputs): VisibleRange {
  const rows = metrics.known.length;
  if (rows === 0) {
    return { start: 0, end: 0, topSpacer: 0, bottomSpacer: 0 };
  }

  // A container measured at zero is one that has not been laid out yet, not one
  // with nothing in it, so guess rather than window down to nothing.
  const viewport = viewportHeight > 0 ? viewportHeight : ASSUMED_VIEWPORT;

  // A whole viewport of overscan at each end is what absorbs rows that turn out
  // to be taller than the estimate: the window has to be wrong by more than a
  // screenful before anything visible is missing.
  const overscan = Math.max(viewport, Math.max(0, overscanRows) * metrics.average);

  let start = clamp(indexAt(metrics, scrollTop - overscan) - 1, 0, rows);
  let end = clamp(indexAt(metrics, scrollTop + viewport + overscan) + 1, 0, rows);

  if (end - start < MINIMUM_ROWS) {
    end = Math.min(rows, start + MINIMUM_ROWS);
    start = Math.max(0, end - MINIMUM_ROWS);
  }

  const total = totalHeight(metrics);
  return {
    start,
    end,
    topSpacer: offsetOf(metrics, start),
    bottomSpacer: Math.max(0, total - offsetOf(metrics, end)),
  };
}

/**
 * Window a scrollable list down to the rows near the viewport.
 *
 * The caller renders `messages.slice(start, end)` between two spacer elements
 * of `topSpacer` and `bottomSpacer` pixels, and the scroller behaves as though
 * every row were mounted. Each rendered row must carry `data-row="{index}"`,
 * which is how heights are attributed back to the rows they belong to.
 *
 * The hook listens on the container itself, so no scroll handler needs to be
 * threaded through; an existing `onScroll` on the same element keeps working.
 *
 * @param containerRef the scrolling element
 * @param count how many rows the list has in total
 * @param estimatedRowHeight what to charge for a row nobody has seen yet
 * @param overscanRows extra rows beyond a viewport's worth at each end
 */
export function useVisibleRange(
  containerRef: React.RefObject<HTMLElement | null>,
  count: number,
  estimatedRowHeight: number,
  overscanRows: number = DEFAULT_OVERSCAN_ROWS,
): VisibleRange {
  const fallback = Math.max(1, estimatedRowHeight);
  const metricsRef = React.useRef<RowMetrics>(createMetrics(count, fallback));

  // A conversation opens pinned to its newest message, so the first window —
  // drawn before there is a scroll position to read — is the tail of the list.
  // Starting at the head would show a screenful of ancient history and then
  // replace it.
  const [range, setRange] = React.useState<VisibleRange>(() =>
    computeVisibleRange({
      scrollTop: totalHeight(metricsRef.current),
      viewportHeight: 0,
      metrics: metricsRef.current,
      overscanRows,
    }),
  );

  const rangeRef = React.useRef(range);
  React.useLayoutEffect(() => {
    rangeRef.current = range;
  }, [range]);

  /*
   * The row the reader is looking at, and where on screen it was.
   *
   * Scroll anchoring, done against the DOM rather than against the arithmetic.
   * Revising a height moves everything below it, and rows coming into the
   * window from above are laid out at their real height for the first time —
   * both shift the conversation under a fixed `scrollTop`. Working out the
   * shift by prediction means predicting heights not yet measured, so instead
   * the topmost visible row is noted before the change and put back where it
   * was after: whatever moved, it did not.
   */
  const anchor = React.useRef<{ index: number; offset: number } | null>(null);

  // Rows are only added at the end in normal use, but a thread reloaded from
  // the engine can differ anywhere, so the table is re-sized rather than
  // rebuilt from nothing — measured heights are worth keeping.
  if (metricsRef.current.known.length !== count) {
    metricsRef.current = resizeMetrics(metricsRef.current, count, fallback);
  }

  const update = React.useCallback(() => {
    const element = containerRef.current;
    if (!element) return;

    const viewportHeight = element.clientHeight;
    // A container with no height is one that is hidden or not yet laid out. It
    // tells us nothing, and keeping the last window is what stops a resize —
    // or a panel being collapsed and reopened — leaving the list blank.
    if (viewportHeight === 0) return;

    // Every mounted row, measured where it actually sits. This is the only
    // source of truth about height; nothing here guesses from an average.
    const measurements: [number, number][] = [];
    const containerTop = element.getBoundingClientRect().top;
    let found: { index: number; offset: number } | null = null;

    for (const node of element.querySelectorAll<HTMLElement>('[data-row]')) {
      const index = Number(node.dataset.row);
      if (Number.isNaN(index)) continue;

      const rect = node.getBoundingClientRect();
      measurements.push([index, rect.height]);

      // The topmost row still showing any part of itself. Rows are in document
      // order, so the first one to qualify is the one to hold on to.
      if (found === null && rect.bottom > containerTop) {
        found = { index, offset: rect.top - containerTop };
      }
    }

    metricsRef.current = withMeasurements(metricsRef.current, measurements, fallback);

    const range = computeVisibleRange({
      scrollTop: element.scrollTop,
      viewportHeight,
      metrics: metricsRef.current,
      overscanRows,
    });

    setRange((current) => {
      if (isSameRange(current, range)) return current;
      // Only worth anchoring when the layout is about to change under us.
      anchor.current = found;
      return range;
    });
  }, [containerRef, count, fallback, overscanRows]);

  const updateRef = React.useRef(update);
  React.useLayoutEffect(() => {
    updateRef.current = update;
  }, [update]);

  // Before paint, so a window computed for the previous message count is never
  // the one the reader sees.
  React.useLayoutEffect(() => {
    update();
  }, [update]);

  /*
   * Put the anchor row back where it was.
   *
   * Runs after the render that carries the new spacers and still before paint,
   * so the shift and its cancellation land on the same frame and nothing is
   * ever drawn out of place. Declared before the caller's own scroll effects,
   * which is what lets "stay pinned to the newest message" still win.
   */
  React.useLayoutEffect(() => {
    const element = containerRef.current;
    const held = anchor.current;
    anchor.current = null;
    if (!element || !held) return;

    const node = element.querySelector<HTMLElement>(`[data-row="${held.index}"]`);
    // Scrolled clean out of the window in one jump: there is nothing to hold
    // on to, and the position the reader asked for is the right one anyway.
    if (!node) return;

    const moved =
      node.getBoundingClientRect().top - element.getBoundingClientRect().top - held.offset;
    // Sub-pixel differences are rounding, and writing `scrollTop` for one would
    // fire another scroll event for nothing.
    if (Math.abs(moved) >= 1) element.scrollTop += moved;
  }, [containerRef, range]);

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
