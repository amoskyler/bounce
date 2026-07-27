/**
 * The things you can do to a message that already exists.
 *
 * Reacting, replying, and deleting all needed the same thing first — somewhere
 * to put them. Signal reveals a small row of buttons beside a bubble on hover;
 * before this, right-click → Info was the only message action here, and a
 * feature with no affordance is a feature nobody finds.
 *
 * The row is deliberately not part of the bubble. It is absolutely positioned
 * beside it, so revealing it cannot change the bubble's width, wrap its text,
 * or move the row below — the same reasoning that took the typing indicator out
 * of the timeline's flow.
 */

import * as React from 'react';

import { EMOJI, type Emoji } from './emoji';
import { EmojiPicker } from './EmojiPicker';
import { InfoIcon, MoreIcon, ReactIcon, ReplyIcon, TrashIcon } from './icons';
import { loadPreferredReactions, notePreferredReaction } from './preferences';
import type { Message, Reaction } from '../preload';
import './message-actions.css';

/**
 * The frame type of the message being acted on, as the engine numbers them.
 *
 * `FrameType::DirectMessage = 0` and `GroupMessage = 1` in the core. The
 * renderer already knows which kind of thread it is in, so it passes the number
 * rather than making the engine look the message up to find out.
 */
export const MESSAGE_FRAME_TYPE = { direct: 0, group: 1 } as const;


/** What a message action needs to know to act. */
export interface MessageTarget {
  message: Message;
  frameType: number;
}

/* --------------------------------------------------------------------------
 * The hover row
 * -------------------------------------------------------------------------- */

export function HoverActions({
  outgoing,
  active,
  onReact,
  onReply,
  onMore,
  picker,
}: {
  outgoing: boolean;
  /** Keeps the row visible while a popover it opened is still up. */
  active: boolean;
  onReact: () => void;
  onReply: () => void;
  onMore: (anchor: HTMLElement) => void;
  /** The reaction strip, rendered above the react button. */
  picker?: React.ReactNode;
}) {
  return (
    <div
      className={[
        'hover-actions',
        `hover-actions--${outgoing ? 'outgoing' : 'incoming'}`,
        active && 'hover-actions--active',
      ]
        .filter(Boolean)
        .join(' ')}
      // The row belongs to the bubble it is beside, and clicking it must not be
      // read as clicking the message.
      onContextMenu={(event) => event.stopPropagation()}
    >
      {/*
        The strip is anchored here rather than to the bubble, because "above the
        react button" is where Signal puts it and where the pointer already is —
        anchoring to the bubble means the strip appears somewhere the hand is
        not, and on a tall message that can be most of a screen away.
      */}
      <span className="hover-actions__anchor">
        <button
          className="hover-actions__button"
          onClick={onReact}
          title="React"
          aria-label="React"
          type="button"
        >
          <ReactIcon size={20} />
        </button>
        {picker}
      </span>

      <button
        className="hover-actions__button"
        onClick={onReply}
        title="Reply"
        aria-label="Reply"
        type="button"
      >
        <ReplyIcon size={20} />
      </button>
      <button
        className="hover-actions__button"
        onClick={(event) => onMore(event.currentTarget)}
        title="More"
        aria-label="More actions"
        type="button"
      >
        <MoreIcon size={20} />
      </button>
    </div>
  );
}

/**
 * The box a floating element actually has to fit inside.
 *
 * The window is the obvious answer and is only right for something that escapes
 * the page — the message menu is `position: fixed`, so it is. A popover inside
 * the timeline is clipped by the *scroller* long before it reaches the window
 * edge, so measuring against the window would call a card that is already half
 * cut off perfectly well placed.
 */
function clippingBox(element: HTMLElement): DOMRect {
  for (let parent = element.parentElement; parent; parent = parent.parentElement) {
    const { overflowY, overflowX, position } = window.getComputedStyle(parent);
    // A fixed element is laid out against the viewport, so no scroller between
    // here and the root clips it.
    if (position === 'fixed') break;
    if (/(auto|scroll|hidden)/.test(overflowY) || /(auto|scroll|hidden)/.test(overflowX)) {
      return parent.getBoundingClientRect();
    }
  }

  return new DOMRect(0, 0, window.innerWidth, window.innerHeight);
}

/**
 * Nudge a floating element back inside whatever will clip it.
 *
 * A popover placed at the pointer, or hung off a control near an edge, will
 * happily render past the bottom of the timeline — where it is cut off and
 * unreachable, which reads as the control being broken rather than
 * mispositioned.
 *
 * Measured after layout and before paint, so the correction is never a visible
 * jump. It adjusts rather than re-anchors: the element keeps the position its
 * caller chose wherever that position fits.
 */
function useKeptInView(reference: React.RefObject<HTMLElement>, deps: React.DependencyList) {
  React.useLayoutEffect(() => {
    const element = reference.current;
    if (!element) return;

    // Clear a previous correction before measuring, or the second pass
    // measures the first pass's result and the element walks up the screen.
    element.style.transform = '';

    const box = element.getBoundingClientRect();
    const limit = clippingBox(element);
    const margin = 8;
    let shiftX = 0;
    let shiftY = 0;

    if (box.right > limit.right - margin) {
      shiftX = limit.right - margin - box.right;
    }
    if (box.left + shiftX < limit.left + margin) {
      shiftX = limit.left + margin - box.left;
    }

    // Flipped over the anchor rather than merely pulled up, when there is room
    // there: an element pinned to the bottom edge overlaps whatever opened it,
    // which for a menu means covering the button you are about to click again.
    if (box.bottom > limit.bottom - margin) {
      const flipped = box.top - box.height - margin;
      shiftY =
        flipped > limit.top + margin
          ? -(box.height + margin)
          : limit.bottom - margin - box.bottom;
    }
    if (box.top + shiftY < limit.top + margin) {
      shiftY = limit.top + margin - box.top;
    }

    if (shiftX !== 0 || shiftY !== 0) {
      element.style.transform = `translate(${shiftX}px, ${shiftY}px)`;
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
}

/* --------------------------------------------------------------------------
 * Choosing a reaction
 * -------------------------------------------------------------------------- */

/**
 * The six-emoji strip, with the full picker one click away.
 *
 * Positioned against the bubble it belongs to rather than at viewport
 * coordinates. `position: fixed` was the obvious choice and the wrong one: the
 * timeline is a CSS container (`container-type: inline-size`), which makes it
 * the containing block for fixed descendants — so viewport coordinates landed
 * the strip somewhere off the side of the conversation, where it could not be
 * clicked and looked like reacting did not work at all.
 */
export function ReactionPicker({
  mine,
  onChoose,
  onDismiss,
}: {
  /** Our current reaction, so it can be shown as already chosen. */
  mine: string | null;
  onChoose: (emoji: string) => void;
  onDismiss: () => void;
}) {
  const panelRef = React.useRef<HTMLDivElement>(null);
  const [expanded, setExpanded] = React.useState(false);
  const [preferred] = React.useState(loadPreferredReactions);

  React.useEffect(() => {
    const onMouseDown = (event: MouseEvent) => {
      const target = event.target;
      if (target instanceof Node && panelRef.current?.contains(target)) return;
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
  }, [onDismiss]);

  if (expanded) {
    return (
      <div
        className="reaction-picker reaction-picker--expanded"
        ref={panelRef}
      >
        <EmojiPicker
          onChoose={(emoji: Emoji) => {
            // Noted here rather than in the caller, so the strip learns from a
            // choice made through the full picker too — otherwise the six
            // never change for anybody who reaches past them.
            notePreferredReaction(emoji.char);
            onChoose(emoji.char);
          }}
          onDismiss={onDismiss}
        />
      </div>
    );
  }

  return (
    <div
      className="reaction-picker"
      ref={panelRef}
      role="menu"
      aria-label="React"
    >
      {preferred.map((emoji) => (
        <button
          key={emoji}
          className={`reaction-picker__emoji${emoji === mine ? ' reaction-picker__emoji--chosen' : ''}`}
          // Clicking the one already chosen withdraws it, which is what the
          // pill does too — the two affordances have to agree or the strip
          // becomes a way to set a reaction you cannot unset from here.
          onClick={() => {
            if (emoji !== mine) notePreferredReaction(emoji);
            onChoose(emoji === mine ? '' : emoji);
          }}
          title={emojiName(emoji)}
          aria-label={emojiName(emoji)}
          role="menuitem"
          type="button"
        >
          {emoji}
        </button>
      ))}
      <button
        className="reaction-picker__more"
        onClick={() => setExpanded(true)}
        title="More emoji"
        aria-label="More emoji"
        role="menuitem"
        type="button"
      >
        <MoreIcon size={16} />
      </button>
    </div>
  );
}

/** A readable name for an emoji, falling back to the character itself. */
function emojiName(character: string): string {
  const known = EMOJI.find((emoji) => emoji.char === character);
  return known ? known.label : character;
}

/* --------------------------------------------------------------------------
 * Showing reactions
 * -------------------------------------------------------------------------- */

/**
 * The pills under a bubble.
 *
 * One per emoji with a count, ours outlined, clicking ours to withdraw. Grouped
 * by the engine rather than here, so a fresh snapshot and a live event produce
 * the same arrangement — those two disagreeing is a bug that only appears after
 * a restart.
 */
export function ReactionPills({
  reactions,
  outgoing,
  names,
}: {
  reactions: readonly Reaction[];
  outgoing: boolean;
  /** Display names by user id. */
  names: (userId: string) => string;
}) {
  /** The open tab, or null when the viewer is closed. `ALL_REACTIONS` included. */
  const [open, setOpen] = React.useState<string | null>(null);

  // A reaction being withdrawn while the viewer is open can leave the selected
  // tab pointing at nothing. Fall back rather than rendering an empty list.
  const selected =
    open !== null && open !== ALL_REACTIONS && !reactions.some((r) => r.emoji === open)
      ? ALL_REACTIONS
      : open;

  if (reactions.length === 0) return null;

  return (
    <div
      className={[
        'reactions',
        `reactions--${outgoing ? 'outgoing' : 'incoming'}`,
        selected !== null && 'reactions--open',
      ]
        .filter(Boolean)
        .join(' ')}
    >
      {reactions.map((reaction) => (
        <button
          key={reaction.emoji}
          className={[
            'reaction-pill',
            reaction.users.length > 1 && 'reaction-pill--with-count',
            reaction.mine && 'reaction-pill--mine',
          ]
            .filter(Boolean)
            .join(' ')}
          /*
           * Opens the list, rather than toggling.
           *
           * Signal's arrangement, and the right one: a pill in a group of
           * twenty is a question ("who?") far more often than it is a button.
           * Withdrawing is still one click — the reaction strip shows your own
           * emoji as chosen, and clicking it there clears it.
           */
          /*
           * Opens on "All", whichever pill was clicked.
           *
           * Clicking ❤️ looks like it should filter to ❤️, and it is the wrong
           * default: the pill is how you ask "who reacted", and pre-filtering
           * answers a narrower question than the one that was asked while
           * hiding the rest behind a tab you have to notice.
           */
          onClick={() => setOpen((current) => (current === null ? ALL_REACTIONS : null))}
          aria-expanded={selected !== null}
          aria-label={`${reaction.users.length} reacted with ${reaction.emoji}`}
          type="button"
        >
          <span className="reaction-pill__emoji">{reaction.emoji}</span>
          {/* A count of one is the pill itself; showing "1" beside every
              solitary reaction is noise in a conversation between two people,
              which is most of them. */}
          {reaction.users.length > 1 && (
            <span className="reaction-pill__count">{reaction.users.length}</span>
          )}
        </button>
      ))}

      {/*
        One viewer for the whole row, not one per pill.
        
        Rendering it inside the pill that was clicked meant switching tabs moved
        it to a different pill's box, so the card jumped sideways under the
        pointer on every tab — the one interaction the tabs exist to make easy.
      */}
      {selected !== null && (
        <ReactionViewer
          reactions={reactions}
          selected={selected}
          outgoing={outgoing}
          names={names}
          onSelect={setOpen}
          onDismiss={() => setOpen(null)}
        />
      )}
    </div>
  );
}

/** The tab that shows every reaction at once, whatever the emoji. */
const ALL_REACTIONS = '\u0000all';

/**
 * Who reacted, and with what.
 *
 * Every emoji on the message is a tab, behind an "All" that aggregates them —
 * the question is usually "who reacted", not "who reacted with this one", and
 * making the aggregate the first thing means the common case needs no choice.
 */
function ReactionViewer({
  reactions,
  selected,
  outgoing,
  names,
  onSelect,
  onDismiss,
}: {
  reactions: readonly Reaction[];
  selected: string;
  outgoing: boolean;
  names: (userId: string) => string;
  onSelect: (tab: string) => void;
  onDismiss: () => void;
}) {
  const panelRef = React.useRef<HTMLDivElement>(null);

  const total = reactions.reduce((count, reaction) => count + reaction.users.length, 0);

  // Every reaction on the message, or just the chosen emoji's.
  const rows =
    selected === ALL_REACTIONS
      ? reactions.flatMap((reaction) =>
          reaction.users.map((userId) => ({ userId, emoji: reaction.emoji })),
        )
      : (reactions.find((reaction) => reaction.emoji === selected)?.users ?? []).map((userId) => ({
          userId,
          emoji: selected,
        }));

  // The card is a fixed anchor but a changing size — a tab with two people is
  // shorter than one with twenty — so the fit is re-checked when it changes.
  useKeptInView(panelRef, [selected, rows.length]);

  React.useEffect(() => {
    const onMouseDown = (event: MouseEvent) => {
      const target = event.target;
      if (target instanceof Node && panelRef.current?.contains(target)) return;
      onDismiss();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopPropagation();
        onDismiss();
      }
    };

    // Deferred a task, so the click that opened this does not immediately
    // close it on its way back up.
    const timer = setTimeout(() => {
      document.addEventListener('mousedown', onMouseDown);
      document.addEventListener('keydown', onKeyDown, true);
    }, 0);

    return () => {
      clearTimeout(timer);
      document.removeEventListener('mousedown', onMouseDown);
      document.removeEventListener('keydown', onKeyDown, true);
    };
  }, [onDismiss]);

  return (
    <div
      className={`reaction-viewer reaction-viewer--${outgoing ? 'outgoing' : 'incoming'}`}
      ref={panelRef}
      role="dialog"
      aria-label="Reactions"
    >
      <div className="reaction-viewer__tabs" role="tablist">
        <button
          className={`reaction-viewer__tab${
            selected === ALL_REACTIONS ? ' reaction-viewer__tab--active' : ''
          }`}
          onClick={() => onSelect(ALL_REACTIONS)}
          role="tab"
          aria-selected={selected === ALL_REACTIONS}
          type="button"
        >
          <span className="reaction-viewer__tab-all">All</span>
          <span className="reaction-viewer__tab-count">{total}</span>
        </button>

        {reactions.map((reaction) => (
          <button
            key={reaction.emoji}
            className={`reaction-viewer__tab${
              reaction.emoji === selected ? ' reaction-viewer__tab--active' : ''
            }`}
            onClick={() => onSelect(reaction.emoji)}
            role="tab"
            aria-selected={reaction.emoji === selected}
            type="button"
          >
            <span>{reaction.emoji}</span>
            <span className="reaction-viewer__tab-count">{reaction.users.length}</span>
          </button>
        ))}
      </div>

      <ul className="reaction-viewer__list">
        {rows.map((row) => (
          <li className="reaction-viewer__row" key={`${row.userId}-${row.emoji}`}>
            <span className="reaction-viewer__name">{names(row.userId)}</span>
            <span className="reaction-viewer__emoji">{row.emoji}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

/* --------------------------------------------------------------------------
 * The overflow menu
 * -------------------------------------------------------------------------- */

export interface MessageMenuActions {
  onInfo: () => void;
  onReply: () => void;
  onCopy: () => void;
  onDeleteForMe: () => void;
  /** Absent when the window has closed or the message is not ours to withdraw. */
  onDeleteForEveryone: (() => void) | null;
}

export function MessageOverflowMenu({
  at,
  actions,
  onDismiss,
}: {
  at: { x: number; y: number };
  actions: MessageMenuActions;
  onDismiss: () => void;
}) {
  const menuRef = React.useRef<HTMLDivElement>(null);

  // Opened at the pointer, which near the bottom of the window puts it behind
  // the composer or off the canvas entirely — where it is clipped and
  // unreachable, and reads as the menu not working.
  useKeptInView(menuRef, [at.x, at.y, actions.onDeleteForEveryone !== null]);

  React.useEffect(() => {
    const dismiss = (event: MouseEvent) => {
      // Ignore anything inside the menu. Dismissing on `mousedown` anywhere
      // would unmount the button between press and release, and the click
      // would never land — which is exactly how "Info" came to do nothing.
      const target = event.target;
      if (target instanceof Node && menuRef.current?.contains(target)) return;
      onDismiss();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onDismiss();
    };

    // Deferred by a task: this menu is opened from inside a click that React
    // flushes synchronously, and a listener installed during that flush would
    // see the same event still propagating and close the menu it just opened.
    const timer = setTimeout(() => {
      document.addEventListener('mousedown', dismiss);
      document.addEventListener('contextmenu', dismiss);
      document.addEventListener('keydown', onKeyDown, true);
    }, 0);

    return () => {
      clearTimeout(timer);
      document.removeEventListener('mousedown', dismiss);
      document.removeEventListener('contextmenu', dismiss);
      document.removeEventListener('keydown', onKeyDown, true);
    };
  }, [onDismiss]);

  return (
    <div
      className="menu menu--at-pointer"
      ref={menuRef}
      style={{ left: at.x, top: at.y }}
      role="menu"
      aria-label="Message actions"
    >
      <button className="menu__item menu__item--icon" onClick={actions.onReply} role="menuitem" type="button">
        <ReplyIcon size={16} />
        Reply
      </button>
      <button className="menu__item menu__item--icon" onClick={actions.onInfo} role="menuitem" type="button">
        <InfoIcon size={16} />
        Info
      </button>
      <button className="menu__item menu__item--icon" onClick={actions.onCopy} role="menuitem" type="button">
        <CopyGlyph />
        Copy text
      </button>

      <div className="menu__divider" role="separator" />

      <button
        className="menu__item menu__item--icon menu__item--danger"
        onClick={actions.onDeleteForMe}
        role="menuitem"
        type="button"
      >
        <TrashIcon size={16} />
        Delete for me
      </button>
      {/*
        Offered only when it would work. The window closes a day after the
        message was written, so this appears and disappears with the clock —
        showing it always and failing on click would teach people the action is
        unreliable rather than time-limited.
      */}
      {actions.onDeleteForEveryone && (
        <button
          className="menu__item menu__item--icon menu__item--danger"
          onClick={actions.onDeleteForEveryone}
          role="menuitem"
          type="button"
        >
          <TrashIcon size={16} />
          Delete for everyone
        </button>
      )}
    </div>
  );
}

function CopyGlyph() {
  return (
    <svg
      width={16}
      height={16}
      viewBox="0 0 20 20"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.7}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <rect x="7.2" y="7.2" width="9.3" height="9.3" rx="2" />
      <path d="M12.8 4.6a2 2 0 0 0-2-2H5.5a2 2 0 0 0-2 2v5.3a2 2 0 0 0 2 2" />
    </svg>
  );
}

/* --------------------------------------------------------------------------
 * Quoting
 * -------------------------------------------------------------------------- */

/**
 * The quote block inside a reply.
 *
 * The bar down its leading edge takes the quoted author's colour, so a run of
 * replies in a group can be told apart at a glance — the same trick the author
 * name inside a bubble uses.
 */
export function QuoteBlock({
  quote,
  authorName,
  colors,
  onJump,
}: {
  quote: NonNullable<Message['quote']>;
  authorName: string;
  colors: { light: string; dark: string };
  onJump: (() => void) | null;
}) {
  const body = quote.expired ? (
    <span className="quote__unavailable">Original message not available</span>
  ) : quote.text ? (
    quote.text
  ) : (
    <span className="quote__kind">{quote.kind === 'image' ? 'Photo' : 'File'}</span>
  );

  return (
    <button
      className={`quote${onJump ? '' : ' quote--static'}`}
      style={
        {
          '--quote-light': colors.light,
          '--quote-dark': colors.dark,
        } as React.CSSProperties
      }
      onClick={onJump ?? undefined}
      disabled={!onJump}
      type="button"
    >
      <span className="quote__author">{authorName}</span>
      <span className="quote__text">{body}</span>
    </button>
  );
}

/**
 * The strip above the composer while a reply is being written.
 *
 * Same shape as the quote it will become, so what you are about to send looks
 * like what you will have sent.
 */
export function ReplyPreview({
  quote,
  authorName,
  colors,
  onCancel,
}: {
  quote: { text: string; kind: 'text' | 'image' | 'file' };
  authorName: string;
  colors: { light: string; dark: string };
  onCancel: () => void;
}) {
  return (
    <div
      className="reply-preview"
      style={
        {
          '--quote-light': colors.light,
          '--quote-dark': colors.dark,
        } as React.CSSProperties
      }
    >
      <div className="reply-preview__body">
        <span className="quote__author">Replying to {authorName}</span>
        <span className="quote__text">
          {quote.text || (
            <span className="quote__kind">{quote.kind === 'image' ? 'Photo' : 'File'}</span>
          )}
        </span>
      </div>
      <button
        className="reply-preview__cancel"
        onClick={onCancel}
        title="Cancel reply"
        aria-label="Cancel reply"
        type="button"
      >
        <svg
          width={16}
          height={16}
          viewBox="0 0 20 20"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.8}
          strokeLinecap="round"
          aria-hidden="true"
        >
          <path d="M5.6 5.6 14.4 14.4M14.4 5.6 5.6 14.4" />
        </svg>
      </button>
    </div>
  );
}

/* --------------------------------------------------------------------------
 * Deleted messages
 * -------------------------------------------------------------------------- */

/**
 * What a bubble says once its message has been withdrawn.
 *
 * Three sentences, not one. "You deleted this message" and "This message was
 * deleted" are the difference between remembering doing it and wondering what
 * you missed, and an admin removing somebody else's is a third thing again —
 * that one is moderation, and attributing it to the author would be wrong.
 */
export function deletedMessageText(message: Message, authorIsMe: boolean): string {
  if (message.deletedByAdmin) return 'This message was removed by an admin';
  return authorIsMe ? 'You deleted this message' : 'This message was deleted';
}
