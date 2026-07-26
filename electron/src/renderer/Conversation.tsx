/**
 * The conversation view: header, message timeline, and composer.
 *
 * The timeline interleaves two kinds of row. Messages are bubbles; status
 * changes — a rename, an invitation, a departure — are centred margin notes.
 * Both carry unix-second timestamps, so they merge into one ordered list and
 * the date separators and grouping rules apply across both.
 *
 * Only the rows near the viewport are mounted. A thread of ten thousand
 * messages would otherwise be ten thousand DOM nodes, all of which React would
 * reconcile on every arriving keystroke's worth of typing indicator.
 */

import * as React from 'react';

import { AttachmentList, AttachmentTray, ImageViewer, useAttachmentIntake } from './Attachments';
import type { BubbleAttachment, PendingAttachment } from './Attachments';
import { cachedFileUrl, fileUrl } from './attachment-data';
import { Avatar } from './Avatar';
import {
  AttachIcon,
  BounceLogo,
  DeliveredIcon,
  EmojiIcon,
  InfoIcon,
  JumpToBottomIcon,
  MoreIcon,
  SendIcon,
  SendingIcon,
  SentIcon,
  TimerIcon,
  UndeliverableIcon,
} from './icons';
import {
  dateSeparator,
  messageTimestamp,
  needsDateSeparator,
  shouldGroupWith,
} from './format';
import { MessageText } from './MessageText';
import { dismissNotifications, setTimelineAtBottom } from './notifications';
import { LOCAL_USER, SystemMessageRow, type DisplayNames } from './SystemMessage';
import { typingAvatarIds, typingLabel } from './typing';
import { useVisibleRange } from './useVisibleRange';
import type { Message, SystemMessage } from '../preload';
import type { Conversation as ConversationSummary, State } from './state';

/**
 * The starting guess at a row's height, refined from the DOM as rows render:
 * one line of text in a bubble plus the gap below its run.
 */
const ESTIMATED_ROW_HEIGHT = 56;

/**
 * How far above the end the reader has to be before the jump control appears.
 *
 * Go shows its icon when the content is taller than 2.5 screens *and* the
 * offset is more than 2.5 screens from the end (`ui/chat_history.go:576-589`),
 * which is the same thing as being more than 1.5 screens from the bottom: the
 * last screenful is the viewport itself.
 */
const JUMP_TO_BOTTOM_SCREENS = 1.5;

/**
 * Renders before the reveal is abandoned.
 *
 * Scrolling to a row that is not mounted takes a pass to move the window over
 * it and another to find it. A handful of passes is generous; a count at all is
 * what stops a bad height estimate from looping.
 */
const MAX_REVEAL_ATTEMPTS = 8;

/** One row of the timeline: either a message or a status change. */
export type Entry =
  | { kind: 'message'; at: number; id: string; message: Message }
  | { kind: 'system'; at: number; id: string; system: SystemMessage };

type ConversationProps = {
  conversation: ConversationSummary;
  state: State;
  onSend: (text: string, attachments: readonly PendingAttachment[]) => void;
  onDraftChange: (text: string) => void;
  onAcceptInvite: () => void;
  onDeclineInvite: () => void;
  onLeaveGroup: () => void;
  onCopyAddress: () => void;
  onShowDetails: () => void;
  onError: (message: string) => void;
};

/**
 * One conversation.
 *
 * The caller keys this on the conversation id, so switching chats remounts the
 * whole view: a fresh scroller, a composer holding the new thread's draft, and
 * no image viewer left open over a conversation it did not come from. Keying
 * the timeline and the composer individually would do the same job, but two
 * siblings under the same key is a React error — it duplicates the children
 * rather than replacing them — so the key belongs here, once.
 */
export function ConversationView({
  conversation,
  state,
  onSend,
  onDraftChange,
  onAcceptInvite,
  onDeclineInvite,
  onLeaveGroup,
  onCopyAddress,
  onShowDetails,
  onError,
}: ConversationProps) {
  const messages = state.messagesByThread[conversation.id] ?? [];
  const systemMessages = state.systemMessagesByThread[conversation.id] ?? [];
  const typing = state.typingByThread[conversation.id] ?? [];

  const [viewerImage, setViewerImage] = React.useState<{ src: string; alt: string } | null>(null);

  // The header avatar's photo. The summary carries no images — it is derived
  // from both tables — so it is looked up here from whichever one owns the id.
  const images =
    conversation.kind === 'group'
      ? state.groups[conversation.id]?.images
      : state.users[conversation.id]?.images;

  return (
    <div className="conversation">
      <ConversationHeader
        conversation={conversation}
        images={images}
        onLeaveGroup={onLeaveGroup}
        onCopyAddress={onCopyAddress}
        onShowDetails={onShowDetails}
      />

      <Timeline
        threadId={conversation.id}
        messages={messages}
        systemMessages={systemMessages}
        state={state}
        isGroup={conversation.kind === 'group'}
        typing={typing}
        onOpenImage={(src, alt) => setViewerImage({ src, alt })}
      />

      {conversation.invitationPending ? (
        <InvitationActions onAccept={onAcceptInvite} onDecline={onDeclineInvite} />
      ) : (
        <Composer
          draft={state.drafts[conversation.id] ?? ''}
          onSend={onSend}
          onChange={onDraftChange}
          onError={onError}
        />
      )}

      {viewerImage && (
        <ImageViewer
          src={viewerImage.src}
          alt={viewerImage.alt}
          onClose={() => setViewerImage(null)}
        />
      )}
    </div>
  );
}

function ConversationHeader({
  conversation,
  images,
  onLeaveGroup,
  onCopyAddress,
  onShowDetails,
}: {
  conversation: ConversationSummary;
  images: readonly string[] | undefined;
  onLeaveGroup: () => void;
  onCopyAddress: () => void;
  onShowDetails: () => void;
}) {
  const [menuOpen, setMenuOpen] = React.useState(false);

  const subtitle =
    conversation.kind === 'group'
      ? `${conversation.memberCount} ${conversation.memberCount === 1 ? 'member' : 'members'}`
      : conversation.online
        ? 'Online'
        : '';

  // Dismiss on any outside click, which is what a menu is expected to do.
  React.useEffect(() => {
    if (!menuOpen) return;
    const close = () => setMenuOpen(false);
    window.addEventListener('click', close);
    return () => window.removeEventListener('click', close);
  }, [menuOpen]);

  return (
    <div className="conversation__header">
      <Avatar
        id={conversation.id}
        name={conversation.name}
        images={images}
        size={32}
        online={conversation.online}
      />

      <button className="conversation__identity" onClick={onShowDetails}>
        <div className="conversation__title">{conversation.name}</div>
        {subtitle && <div className="conversation__subtitle">{subtitle}</div>}
      </button>

      {/* Bounce has no voice or video calling, so there are no call buttons. */}
      <div className="conversation__header-actions">
        <button
          className="icon-button"
          onClick={onShowDetails}
          title={conversation.kind === 'group' ? 'Group info' : 'Contact info'}
        >
          <InfoIcon />
        </button>

        <div className="menu-anchor">
          <button
            className="icon-button"
            title="Conversation options"
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            onClick={(event) => {
              event.stopPropagation();
              setMenuOpen((open) => !open);
            }}
          >
            <MoreIcon />
          </button>

          {menuOpen && (
            <div className="menu" role="menu" onClick={(event) => event.stopPropagation()}>
              <button
                className="menu__item"
                role="menuitem"
                onClick={() => {
                  onShowDetails();
                  setMenuOpen(false);
                }}
              >
                {conversation.kind === 'group' ? 'Group info' : 'Contact info'}
              </button>
              {conversation.kind === 'direct' && (
                <button
                  className="menu__item"
                  role="menuitem"
                  onClick={() => {
                    onCopyAddress();
                    setMenuOpen(false);
                  }}
                >
                  Copy my address
                </button>
              )}
              {conversation.kind === 'group' && (
                <button
                  className="menu__item menu__item--destructive"
                  role="menuitem"
                  onClick={() => {
                    onLeaveGroup();
                    setMenuOpen(false);
                  }}
                >
                  Leave group
                </button>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

/** Merge messages and status rows into one list, oldest first. */
function buildEntries(messages: Message[], systemMessages: SystemMessage[]): Entry[] {
  const entries: Entry[] = [
    ...messages.map(
      (message): Entry => ({
        kind: 'message',
        at: message.writtenAt,
        id: message.id,
        message,
      }),
    ),
    ...systemMessages.map(
      (system): Entry => ({ kind: 'system', at: system.timestamp, id: system.id, system }),
    ),
  ];

  // Ties are broken by id, so the order is the same on every render and on
  // every device that holds the same rows.
  return entries.sort((a, b) => a.at - b.at || (a.id < b.id ? -1 : 1));
}

/**
 * The first row the reader has not seen, or -1 when the thread is fully read.
 *
 * Only an incoming message counts: a status row is not something to be read,
 * and one's own message never is. Go walks the same list with the same test
 * (`ui/chat_history.go:534-551`), skipping anything that does not count as
 * unread and stopping at the first unseen item.
 */
export function firstUnreadIndex(entries: readonly Entry[]): number {
  for (let index = 0; index < entries.length; index += 1) {
    const entry = entries[index];
    if (entry.kind !== 'message') continue;
    if (entry.message.outgoing) continue;
    if (!entry.message.seen) return index;
  }
  return -1;
}

function Timeline({
  threadId,
  messages,
  systemMessages,
  state,
  isGroup,
  typing,
  onOpenImage,
}: {
  threadId: string;
  messages: Message[];
  systemMessages: SystemMessage[];
  state: State;
  isGroup: boolean;
  /** User ids currently composing, oldest first. */
  typing: readonly string[];
  onOpenImage: (src: string, alt: string) => void;
}) {
  const scrollRef = React.useRef<HTMLDivElement>(null);
  const atBottomRef = React.useRef(true);

  // The row to bring into view on opening, and how many passes it has had. Refs
  // rather than state: the reveal has to survive the renders it takes to get
  // the row mounted, and none of it is anything to draw.
  const revealRef = React.useRef<number | null>(null);
  const revealAttemptsRef = React.useRef(0);

  const [farFromBottom, setFarFromBottom] = React.useState(false);

  const entries = React.useMemo(
    () => buildEntries(messages, systemMessages),
    [messages, systemMessages],
  );

  const unreadCount = React.useMemo(
    () => messages.filter((message) => !message.outgoing && !message.seen).length,
    [messages],
  );

  // Only the rows near the viewport are mounted; the spacers below stand in for
  // the rest, so a ten thousand message thread costs a screenful of nodes.
  const range = useVisibleRange(scrollRef, entries.length, ESTIMATED_ROW_HEIGHT);

  // Names for the status sentences. Mapping our own id to `LOCAL_USER` is what
  // makes one read "You created the group".
  const names = React.useMemo<DisplayNames>(() => {
    const table: Record<string, string> = {};
    for (const user of Object.values(state.users)) {
      table[user.id] = user.alias || user.name;
    }
    if (state.profile) table[state.profile.id] = LOCAL_USER;
    return table;
  }, [state.users, state.profile]);

  // Track whether the reader is at the bottom, so arriving messages only
  // autoscroll when they are not reading back through history. The notification
  // rules ask the same question, from far enough away that it has to be
  // published rather than kept in this ref.
  const handleScroll = React.useCallback(() => {
    const element = scrollRef.current;
    if (!element) return;
    const distance = element.scrollHeight - element.scrollTop - element.clientHeight;
    atBottomRef.current = distance < 80;
    setTimelineAtBottom(threadId, atBottomRef.current);
    setFarFromBottom(distance > element.clientHeight * JUMP_TO_BOTTOM_SCREENS);
  }, [threadId]);

  // Opening a conversation is reason enough to withdraw any notification still
  // on screen for it, and it is where the reader is put back at the last thing
  // they read rather than at the newest message: Go's `scrollToLastRead`
  // (`ui/thread.go:440`). A layout effect, and declared above the one that
  // scrolls, so the decision is made before the first paint rather than after
  // a frame pinned to the bottom.
  React.useLayoutEffect(() => {
    dismissNotifications(threadId);

    const unread = firstUnreadIndex(entries);
    revealAttemptsRef.current = 0;

    if (unread < 0) {
      // Everything read: open at the newest message, as before.
      atBottomRef.current = true;
      revealRef.current = null;
    } else {
      // Go scrolls to the row above the first unseen one, so what you last read
      // is at the top of the screen and the new material begins under it.
      atBottomRef.current = false;
      revealRef.current = Math.max(0, unread - 1);
    }

    setTimelineAtBottom(threadId, atBottomRef.current);
    setFarFromBottom(false);
    // `entries` is deliberately not a dependency: this is where the thread is
    // opened, and Go positions a thread on first open only (`ui/thread.go:435`).
    // Re-running it as messages arrive would drag the reader back up the list.
  }, [threadId]);

  // The window is in the dependencies because one that grows after a scroll has
  // to be re-pinned: the rows it added sit below where we last scrolled to. It
  // is also what gives the reveal below a second pass once the window it asked
  // for has been mounted.
  React.useLayoutEffect(() => {
    const element = scrollRef.current;
    if (!element) return;

    const reveal = revealRef.current;
    if (reveal !== null) {
      revealAttemptsRef.current += 1;
      const row = rowElement(element, reveal) ?? rowElement(element, reveal + 1);

      if (row) {
        // Align the row with the top of the viewport.
        element.scrollTop += row.getBoundingClientRect().top - element.getBoundingClientRect().top;
        revealRef.current = null;
      } else if (revealAttemptsRef.current >= MAX_REVEAL_ATTEMPTS) {
        // The row is not being mounted where the arithmetic says it is. The
        // approximate position is where we are, and it is close enough to stop.
        revealRef.current = null;
      } else {
        // It is outside the mounted window. Jumping to where the windowing
        // arithmetic puts it moves the window over it, and the next pass — this
        // effect again, on the render that scroll causes — finds the row.
        element.scrollTop = (reveal / entries.length) * element.scrollHeight;
      }
      return;
    }

    if (atBottomRef.current) {
      element.scrollTop = element.scrollHeight;
    }
  }, [entries.length, typing.length, range.start, range.end]);

  // Go's jump-to-bottom does three things: scrolls down, zeroes the unread
  // counter, and marks the thread read (`ui/chat_history.go:89-110`). The last
  // one matters here because the read sweep on selection is gated on the window
  // being focused, so a thread opened in the background is still unread when
  // the reader finally scrolls to the end of it.
  const jumpToBottom = React.useCallback(() => {
    const element = scrollRef.current;
    revealRef.current = null;
    atBottomRef.current = true;
    setTimelineAtBottom(threadId, true);
    setFarFromBottom(false);

    if (element) element.scrollTop = element.scrollHeight;

    for (const message of messages) {
      if (message.outgoing || message.seen) continue;
      void window.bounce.markAsRead(message.id, isGroup).catch(() => {
        // The engine will be asked again the next time the thread is opened.
      });
    }
  }, [threadId, messages, isGroup]);

  // A typing indicator has to render in the empty case too — a brand new
  // conversation is exactly where you first watch for one.
  //
  // The engine has always said *who*; the view used to reduce that to a count,
  // which in a group of eight told you only that somebody was composing
  // something. See `typing.ts` for the wording and the cap.
  const label = typingLabel(typing, names, {
    isGroup,
    selfId: state.profile?.id ?? null,
  });
  const typingFaces = isGroup
    ? typingAvatarIds(typing, { selfId: state.profile?.id ?? null })
    : [];

  const typingIndicator = typing.length > 0 && (
    <div className="typing" aria-label={label ?? 'typing'}>
      {typingFaces.map((userId) => (
        <Avatar
          key={userId}
          id={userId}
          name={names[userId] ?? '?'}
          images={state.users[userId]?.images}
          size={20}
          className="typing__face"
        />
      ))}
      {label && <span className="typing__who">{label}</span>}
      <span className="typing__dot" />
      <span className="typing__dot" />
      <span className="typing__dot" />
    </div>
  );

  if (entries.length === 0) {
    return (
      <div className="timeline-area">
        <div className="timeline" ref={scrollRef} onScroll={handleScroll}>
          <div className="timeline__spacer" />
          <div className="placeholder">
            <div className="placeholder__body">
              No messages yet. Say something to start the conversation.
            </div>
          </div>
          {typingIndicator}
        </div>
      </div>
    );
  }

  const rendered: React.ReactNode[] = [];
  // Seeded from the row above the window, so the top of it groups and dates
  // itself the way it would if the whole thread were mounted.
  let previous: Message | null = precedingMessage(entries, range.start);

  for (let index = range.start; index < range.end; index += 1) {
    const entry = entries[index];

    if (needsDateSeparator(previous?.writtenAt ?? null, entry.at)) {
      rendered.push(
        <div className="timeline__date" key={`date-${entry.id}`}>
          {dateSeparator(entry.at)}
        </div>,
      );
      previous = null;
    }

    if (entry.kind === 'system') {
      rendered.push(<SystemMessageRow key={entry.id} message={entry.system} names={names} />);
      // A status row breaks a run, or the two bubbles either side of it would
      // merge into one.
      previous = null;
      continue;
    }

    const message = entry.message;
    const grouped = shouldGroupWith(previous, message);
    // The tail belongs on the last bubble of a run, so look ahead — into the
    // whole thread, not the window, or the last rendered row loses its tail.
    const next = entries[index + 1];
    const continuesAfter =
      next !== undefined && next.kind === 'message' ? shouldGroupWith(message, next.message) : false;

    rendered.push(
      <MessageRow
        key={message.id}
        index={index}
        message={message}
        state={state}
        isGroup={isGroup}
        grouped={grouped}
        continuesAfter={continuesAfter}
        onOpenImage={onOpenImage}
      />,
    );

    previous = message;
  }

  return (
    <div className="timeline-area">
      <div className="timeline" ref={scrollRef} onScroll={handleScroll}>
        <div className="timeline__spacer" />
        {range.topSpacer > 0 && (
          // `flexShrink: 0` is not optional: `.timeline` is a flex column, so an
          // empty div with a height is shrunk away the moment the content
          // overflows, and the scroller collapses to the rendered rows.
          <div style={{ height: range.topSpacer, flexShrink: 0 }} aria-hidden="true" />
        )}
        {rendered}
        {range.bottomSpacer > 0 && (
          <div style={{ height: range.bottomSpacer, flexShrink: 0 }} aria-hidden="true" />
        )}
        {typingIndicator}
      </div>

      {farFromBottom && (
        <button
          className="timeline__jump"
          onClick={jumpToBottom}
          title="Jump to the newest message"
          aria-label="Jump to the newest message"
        >
          <JumpToBottomIcon />
          {unreadCount > 0 && (
            <span className="timeline__jump-badge">{unreadCount > 99 ? '99+' : unreadCount}</span>
          )}
        </button>
      )}
    </div>
  );
}

/** A mounted timeline row by its index, or null if it is outside the window. */
function rowElement(container: HTMLElement, index: number): HTMLElement | null {
  return container.querySelector<HTMLElement>(`[data-row="${index}"]`);
}

/** The nearest message above a window's start, for grouping and dating. */
function precedingMessage(entries: Entry[], start: number): Message | null {
  for (let index = start - 1; index >= 0; index -= 1) {
    const entry = entries[index];
    // A status row resets the run, so anything above it is irrelevant here.
    if (entry.kind === 'system') return null;
    return entry.message;
  }
  return null;
}

/**
 * Resolve attachments to something renderable.
 *
 * A file's bytes are in the main process, so a complete attachment is fetched
 * once and cached as an object URL. Anything still downloading renders as its
 * own progress, which is why the record is passed through either way.
 */
function useAttachmentUrls(attachments: readonly BubbleAttachment[]): BubbleAttachment[] {
  const [, forceRender] = React.useReducer((count: number) => count + 1, 0);

  const complete = attachments
    .filter((attachment) => attachment.progress >= 1)
    .map((attachment) => attachment.fileId)
    .join(',');

  React.useEffect(() => {
    if (!complete) return;
    let cancelled = false;

    void Promise.all(complete.split(',').map((fileId) => fileUrl(fileId))).then((results) => {
      // Re-render only if something actually landed, so an attachment that is
      // still assembling does not spin.
      if (!cancelled && results.some((url) => url !== null)) forceRender();
    });

    return () => {
      cancelled = true;
    };
  }, [complete]);

  return attachments.map((attachment) => ({
    ...attachment,
    url: cachedFileUrl(attachment.fileId),
  }));
}

function MessageRow({
  index,
  message,
  state,
  isGroup,
  grouped,
  continuesAfter,
  onOpenImage,
}: {
  /** Position in the whole thread, so the scroller can find this row again. */
  index: number;
  message: Message;
  state: State;
  isGroup: boolean;
  grouped: boolean;
  continuesAfter: boolean;
  onOpenImage: (src: string, alt: string) => void;
}) {
  const author = state.users[message.author];
  const authorName = author ? author.alias || author.name : 'Unknown';
  const attachments = useAttachmentUrls(message.attachments);

  const groupClassName = [
    'message-group',
    message.outgoing ? 'message-group--outgoing' : 'message-group--incoming',
    grouped && 'message-group--continued',
  ]
    .filter(Boolean)
    .join(' ');

  const bubbleClassName = [
    'bubble',
    message.outgoing ? 'bubble--outgoing' : 'bubble--incoming',
    continuesAfter && 'bubble--continued',
  ]
    .filter(Boolean)
    .join(' ');

  return (
    <div className={groupClassName} data-row={index}>
      <div className="message-group__avatar-slot">
        {/* In groups, the avatar sits beside the last bubble of an incoming run. */}
        {isGroup && !message.outgoing && !continuesAfter && (
          <Avatar id={message.author} name={authorName} images={author?.images} size={28} />
        )}
      </div>

      <div className="message-group__stack">
        {isGroup && !message.outgoing && !grouped && (
          <div className="message-group__author" style={{ color: 'var(--text-secondary)' }}>
            {authorName}
          </div>
        )}

        <div className={bubbleClassName}>
          {/* Ahead of the footer, so the floated timestamp wraps around the
              text rather than around the pictures. */}
          <AttachmentList attachments={attachments} onOpenImage={onOpenImage} />
          <span className="bubble__footer">
            {message.expiresAt > 0 && <TimerIcon />}
            <span>{messageTimestamp(message.writtenAt)}</span>
            {message.outgoing && <DeliveryStatus message={message} />}
          </span>
          <MessageText text={message.text} />
        </div>
      </div>
    </div>
  );
}

/**
 * The tick marks on an outgoing message.
 *
 * Bounce establishes delivery only from acknowledgements, so these four states
 * are the whole truth about a message: queued, written to somebody, confirmed
 * by a recipient's device, or given up on.
 */
function DeliveryStatus({ message }: { message: Message }) {
  if (message.undeliverable) {
    return (
      <span className="bubble__status bubble__undeliverable" title="Not delivered">
        <UndeliverableIcon />
      </span>
    );
  }

  if (message.readBy.length > 0) {
    return (
      <span className="bubble__status bubble__status--read" title="Read">
        <DeliveredIcon />
      </span>
    );
  }

  if (message.deliveredTo.length > 0) {
    return (
      <span className="bubble__status" title="Delivered">
        <SentIcon />
      </span>
    );
  }

  return (
    <span className="bubble__status" title="Sending">
      <SendingIcon />
    </span>
  );
}

function InvitationActions({
  onAccept,
  onDecline,
}: {
  onAccept: () => void;
  onDecline: () => void;
}) {
  return (
    <div className="composer" style={{ justifyContent: 'center', gap: 12 }}>
      <button className="modal__button" onClick={onDecline}>
        Decline
      </button>
      <button className="modal__button modal__button--primary" onClick={onAccept}>
        Join group
      </button>
    </div>
  );
}

function Composer({
  draft,
  onSend,
  onChange,
  onError,
}: {
  draft: string;
  onSend: (text: string, attachments: readonly PendingAttachment[]) => void;
  onChange: (text: string) => void;
  onError: (message: string) => void;
}) {
  const [text, setText] = React.useState(draft);
  const textareaRef = React.useRef<HTMLTextAreaElement>(null);
  const intake = useAttachmentIntake({ onError });

  // Grow with the content up to the CSS max height, then scroll.
  const resize = React.useCallback(() => {
    const element = textareaRef.current;
    if (!element) return;
    element.style.height = 'auto';
    element.style.height = `${element.scrollHeight}px`;
  }, []);

  React.useLayoutEffect(resize, [text, resize]);

  const submit = () => {
    const trimmed = text.trim();
    // An attachment on its own is a message; text is not required.
    if (!trimmed && intake.attachments.length === 0) return;
    onSend(trimmed, intake.attachments);
    // Clearing revokes every preview object URL, which is why it happens here
    // rather than being left to the next render.
    intake.clear();
    setText('');
    onChange('');
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // Enter sends; Shift+Enter inserts a newline.
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      submit();
    }
  };

  const areaClassName = ['composer-area', intake.dropActive && 'composer-area--dropping']
    .filter(Boolean)
    .join(' ');

  return (
    // Drag and drop are bound here so the target covers the tray as well as
    // the input row. Paste is not: it is caught on the document, because focus
    // is rarely in the composer at the moment somebody pastes a screenshot.
    <div
      className={areaClassName}
      onDragOver={intake.onDragOver}
      onDragLeave={intake.onDragLeave}
      onDrop={intake.onDrop}
    >
      <AttachmentTray attachments={intake.attachments} onRemove={intake.remove} />

      {intake.error && <div className="composer-area__error">{intake.error}</div>}

      {intake.fileInput}

      <div className="composer">
        <button className="icon-button" onClick={intake.openFilePicker} title="Attach a file">
          <AttachIcon />
        </button>

        <div className="composer__input-wrapper">
          <textarea
            ref={textareaRef}
            className="composer__input"
            rows={1}
            placeholder="Message"
            value={text}
            onChange={(event) => {
              setText(event.target.value);
              onChange(event.target.value);
            }}
            onKeyDown={handleKeyDown}
            aria-label="Message"
          />
          <button className="icon-button" title="Emoji" style={{ width: 24, height: 24 }}>
            <EmojiIcon size={18} />
          </button>
        </div>

        <button
          className="composer__send"
          onClick={submit}
          disabled={text.trim().length === 0 && intake.attachments.length === 0}
          title="Send"
          aria-label="Send"
        >
          <SendIcon />
        </button>
      </div>
    </div>
  );
}

/** Shown when no conversation is selected. */
export function NoConversationSelected({ address }: { address: string }) {
  return (
    <div className="conversation">
      <div className="placeholder">
        <BounceLogo size={72} className="placeholder__logo" />
        <div className="placeholder__title">Bounce</div>
        <div className="placeholder__body">
          Select a conversation, or add a contact to start a new one. Every connection runs
          over the mixnet, and your data never leaves your own devices.
        </div>
        {address && (
          <div className="onboarding__address" title="This device's address">
            {address}
          </div>
        )}
      </div>
    </div>
  );
}
