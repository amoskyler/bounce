/**
 * The emoji panel, and the suggestion list the `:` typeahead puts on screen.
 *
 * Both render the same cells and both close on Escape or an outside click, so
 * they live together — the difference between them is only where they are
 * anchored and how a choice is applied.
 *
 * Nearly two thousand cells is more than is worth keeping in the DOM at once,
 * so the grid renders the section you are looking at plus its neighbours and
 * lets the rest be blank space of the right height. Scrolling stays smooth and
 * the tab strip still jumps to the right place.
 */

import * as React from 'react';

import { EMOJI, EMOJI_CATEGORIES, searchEmoji, type Emoji } from './emoji';
import { loadRecentEmoji, noteRecentEmoji } from './preferences';
import { SearchIcon } from './icons';
import './emoji.css';

/** Cells per row. Fixed, so a section's height can be worked out arithmetically. */
const COLUMNS = 8;

/** Must match `--emoji-cell` in the stylesheet. */
const CELL_SIZE = 36;

/** Must match the `.emoji-picker__heading` height in the stylesheet. */
const HEADING_HEIGHT = 26;

/** How far outside the viewport to keep rendering, in rows. */
const OVERSCAN_ROWS = 4;

/** Cap on typeahead suggestions: enough to choose from, few enough to scan. */
const SUGGESTION_LIMIT = 10;

interface Section {
  title: string;
  emoji: readonly Emoji[];
  /** Pixels from the top of the scroller to this section's heading. */
  offset: number;
  /** Heading plus grid. */
  height: number;
}

function buildSections(recent: readonly string[]): Section[] {
  const byCategory: Emoji[][] = EMOJI_CATEGORIES.map(() => []);
  for (const emoji of EMOJI) byCategory[emoji.category]?.push(emoji);

  const groups: { title: string; emoji: readonly Emoji[] }[] = [];

  if (recent.length > 0) {
    // Resolved against the table on every build so an unknown character —
    // saved by an older build, dropped since — simply does not appear.
    const known = new Map(EMOJI.map((emoji) => [emoji.char, emoji]));
    const emoji = recent
      .map((character) => known.get(character))
      .filter((entry): entry is Emoji => entry !== undefined);
    if (emoji.length > 0) groups.push({ title: 'Recent', emoji });
  }

  EMOJI_CATEGORIES.forEach((title, index) => {
    groups.push({ title, emoji: byCategory[index] });
  });

  let offset = 0;
  return groups.map((group) => {
    const height = HEADING_HEIGHT + Math.ceil(group.emoji.length / COLUMNS) * CELL_SIZE;
    const section = { ...group, offset, height };
    offset += height;
    return section;
  });
}

/** One emoji cell. Memoised because a scroll re-renders hundreds of them. */
const EmojiCell = React.memo(function EmojiCell({
  emoji,
  selected,
  onChoose,
}: {
  emoji: Emoji;
  selected?: boolean;
  onChoose: (emoji: Emoji) => void;
}) {
  return (
    <button
      className={`emoji-cell${selected ? ' emoji-cell--selected' : ''}`}
      // Losing focus would close the panel before the click landed.
      onMouseDown={(event) => event.preventDefault()}
      onClick={() => onChoose(emoji)}
      title={`:${emoji.shortcodes[0]}:`}
      aria-label={emoji.label}
      type="button"
    >
      {emoji.char}
    </button>
  );
});

/**
 * Close when something outside is clicked, or Escape is pressed.
 *
 * Bound on `mousedown` rather than `click`: a click that starts inside the
 * panel and ends outside it — dragging across the grid, say — is not a
 * dismissal, and `click` fires on the common ancestor for exactly that case.
 */
function useDismiss(
  reference: React.RefObject<HTMLElement>,
  onDismiss: () => void,
  extra?: React.RefObject<HTMLElement>,
) {
  React.useEffect(() => {
    const onMouseDown = (event: MouseEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (reference.current?.contains(target)) return;
      if (extra?.current?.contains(target)) return;
      onDismiss();
    };

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopPropagation();
        onDismiss();
      }
    };

    document.addEventListener('mousedown', onMouseDown);
    document.addEventListener('keydown', onKeyDown, true);
    return () => {
      document.removeEventListener('mousedown', onMouseDown);
      document.removeEventListener('keydown', onKeyDown, true);
    };
  }, [reference, extra, onDismiss]);
}

/**
 * The full picker, anchored above the emoji button.
 *
 * `anchorRef` is the button itself, excluded from the outside-click check so
 * that clicking it while the panel is open closes it once rather than closing
 * and immediately reopening.
 */
export function EmojiPicker({
  onChoose,
  onDismiss,
  anchorRef,
}: {
  onChoose: (emoji: Emoji) => void;
  onDismiss: () => void;
  anchorRef?: React.RefObject<HTMLElement>;
}) {
  const panelRef = React.useRef<HTMLDivElement>(null);
  const scrollerRef = React.useRef<HTMLDivElement>(null);
  const searchRef = React.useRef<HTMLInputElement>(null);

  const [query, setQuery] = React.useState('');
  const [scrollTop, setScrollTop] = React.useState(0);
  const [viewport, setViewport] = React.useState(280);

  // Read once per mount: the list is only allowed to reorder itself when the
  // panel is reopened, so a cell never moves out from under the pointer.
  const [recent] = React.useState(loadRecentEmoji);
  const sections = React.useMemo(() => buildSections(recent), [recent]);
  const results = React.useMemo(() => (query.trim() ? searchEmoji(query, 64) : null), [query]);

  useDismiss(panelRef, onDismiss, anchorRef);

  React.useEffect(() => {
    searchRef.current?.focus();
    const element = scrollerRef.current;
    if (element) setViewport(element.clientHeight);
  }, []);

  const choose = React.useCallback(
    (emoji: Emoji) => {
      noteRecentEmoji(emoji.char);
      onChoose(emoji);
    },
    [onChoose],
  );

  const total = sections.length > 0 ? sections[sections.length - 1].offset + sections[sections.length - 1].height : 0;

  // Which section the tab strip should light up: the one under the top edge.
  const activeIndex = sections.findIndex(
    (section, index) =>
      scrollTop < section.offset + section.height || index === sections.length - 1,
  );

  const jumpTo = (index: number) => {
    const section = sections[index];
    if (section && scrollerRef.current) scrollerRef.current.scrollTop = section.offset;
  };

  return (
    <div className="emoji-picker" ref={panelRef} role="dialog" aria-label="Emoji">
      <div className="emoji-picker__search">
        <SearchIcon />
        <input
          ref={searchRef}
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Search emoji"
          aria-label="Search emoji"
          spellCheck={false}
        />
      </div>

      <div
        className="emoji-picker__scroller"
        ref={scrollerRef}
        onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
      >
        {results ? (
          results.length === 0 ? (
            <div className="emoji-picker__empty">No emoji found</div>
          ) : (
            <div className="emoji-picker__grid">
              {results.map((emoji) => (
                <EmojiCell key={emoji.char} emoji={emoji} onChoose={choose} />
              ))}
            </div>
          )
        ) : (
          // One tall spacer holding absolutely positioned sections, so scroll
          // position stays put no matter which of them are currently rendered.
          <div className="emoji-picker__sections" style={{ height: total }}>
            {sections.map((section) => {
              const top = scrollTop - OVERSCAN_ROWS * CELL_SIZE;
              const bottom = scrollTop + viewport + OVERSCAN_ROWS * CELL_SIZE;
              if (section.offset + section.height < top || section.offset > bottom) return null;

              return (
                <div
                  className="emoji-picker__section"
                  key={section.title}
                  style={{ top: section.offset, height: section.height }}
                >
                  <div className="emoji-picker__heading">{section.title}</div>
                  <div className="emoji-picker__grid">
                    {section.emoji.map((emoji) => (
                      <EmojiCell key={emoji.char} emoji={emoji} onChoose={choose} />
                    ))}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>

      <div className="emoji-picker__tabs" role="tablist">
        {sections.map((section, index) => (
          <button
            key={section.title}
            className={`emoji-picker__tab${index === activeIndex ? ' emoji-picker__tab--active' : ''}`}
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => jumpTo(index)}
            title={section.title}
            aria-label={section.title}
            role="tab"
            aria-selected={index === activeIndex}
            type="button"
          >
            {section.emoji[0]?.char ?? '·'}
          </button>
        ))}
      </div>
    </div>
  );
}

/**
 * The list shown while a `:shortcode` is being typed.
 *
 * Keyboard handling lives in the composer, not here: the keys that drive this
 * list — arrows, Tab, Enter — are pressed in the textarea, which never gives up
 * focus, so this component only ever displays a selection somebody else owns.
 */
export function EmojiSuggestions({
  query,
  selected,
  onChoose,
  onDismiss,
}: {
  query: string;
  selected: number;
  onChoose: (emoji: Emoji) => void;
  onDismiss: () => void;
}) {
  const listRef = React.useRef<HTMLDivElement>(null);
  const matches = useEmojiSuggestions(query);

  useDismiss(listRef, onDismiss);

  // Follow the selection when the arrow keys walk it past an edge.
  React.useEffect(() => {
    listRef.current
      ?.querySelector('.emoji-suggestion--selected')
      ?.scrollIntoView({ block: 'nearest' });
  }, [selected]);

  if (matches.length === 0) return null;

  return (
    <div className="emoji-suggestions" ref={listRef} role="listbox" aria-label="Emoji suggestions">
      {matches.map((emoji, index) => (
        <button
          key={emoji.char}
          className={`emoji-suggestion${index === selected ? ' emoji-suggestion--selected' : ''}`}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => onChoose(emoji)}
          role="option"
          aria-selected={index === selected}
          type="button"
        >
          <span className="emoji-suggestion__char">{emoji.char}</span>
          <span className="emoji-suggestion__name">:{emoji.shortcodes[0]}:</span>
        </button>
      ))}
    </div>
  );
}

/**
 * The suggestions a partial shortcode offers, capped.
 *
 * Exported so the composer can ask what Enter would pick without rendering
 * anything, which keeps the two from ever disagreeing about the list.
 */
export function useEmojiSuggestions(query: string): Emoji[] {
  return React.useMemo(() => (query ? searchEmoji(query, SUGGESTION_LIMIT) : []), [query]);
}
