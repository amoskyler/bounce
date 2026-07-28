/**
 * The left pane: search, the conversation list, and the compose actions.
 */

import * as React from 'react';

import { Avatar } from './Avatar';
import { deliveryState, showsDeliveryState } from './delivery';
import {
  ComposeIcon,
  DeliveryTick,
  NewGroupIcon,
  SearchIcon,
  SettingsIcon,
} from './icons';
import { conversationTimestamp, snippet } from './format';
import {
  clampLeftPaneWidth,
  DEFAULT_LEFT_PANE_WIDTH,
  loadLeftPaneWidth,
  MAX_LEFT_PANE_WIDTH,
  MIN_LEFT_PANE_WIDTH,
  saveLeftPaneWidth,
} from './preferences';
import { contacts as deriveContacts, type Conversation, type State } from './state';

type LeftPaneProps = {
  state: State;
  conversations: Conversation[];
  onSelect: (id: string) => void;
  onSearch: (query: string) => void;
  onNewGroup: () => void;
  onBrowseContacts: () => void;
  onOpenSettings: () => void;
};

export function LeftPane({
  state,
  conversations,
  onSelect,
  onSearch,
  onNewGroup,
  onBrowseContacts,
  onOpenSettings,
}: LeftPaneProps) {
  // The list holds open conversations only, so an empty one does not mean an
  // empty address book — it matters which of the two is missing.
  const contactCount = React.useMemo(() => deriveContacts(state).length, [state]);
  const resize = useResizableWidth();

  return (
    <div className="left-pane" style={{ width: resize.width }}>
      <div className="left-pane__header">
        {/*
          Two rows, not one.
          
          A profile button, a search field and three actions on a single line
          leaves the search box squeezed between two fixed-width clusters — it
          was the only thing that could give, so it gave on every pane width.
          Signal splits them: identity and actions above, search across the full
          width below.
        */}
        <div className="left-pane__header-top">
          {/* The avatar opens settings, the way the profile button does in
              Signal — it is the one thing in the header that is about you. */}
          {state.profile && (
            <button
              className="left-pane__profile"
              onClick={onOpenSettings}
              title="Settings"
              aria-label="Settings"
            >
              <Avatar
                id={state.profile.id}
                name={state.profile.name}
                images={state.profile.images}
                size={32}
              />
            </button>
          )}

          <div className="left-pane__actions">
            <button className="icon-button" onClick={onNewGroup} title="New group">
              <NewGroupIcon />
            </button>
            {/* The compose button opens the contact store, the way Go's menu
                reaches `showNewDM` (`ui/menu.go:71`) — starting a conversation
                is picking somebody you already know, and adding somebody new is
                a step inside that. */}
            <button className="icon-button" onClick={onBrowseContacts} title="New conversation">
              <ComposeIcon />
            </button>
            <button className="icon-button" onClick={onOpenSettings} title="Settings">
              <SettingsIcon />
            </button>
          </div>
        </div>

        <label className="left-pane__search">
          <SearchIcon />
          <input
            type="text"
            placeholder="Search"
            value={state.searchQuery}
            onChange={(event) => onSearch(event.target.value)}
            aria-label="Search conversations"
          />
        </label>
      </div>

      <div className="left-pane__list" role="list">
        {conversations.length === 0 ? (
          <EmptyList
            searching={state.searchQuery.trim().length > 0}
            contactCount={contactCount}
            onBrowseContacts={onBrowseContacts}
          />
        ) : (
          conversations.map((conversation) => (
            <ConversationRow
              key={conversation.id}
              conversation={conversation}
              state={state}
              selected={state.selectedConversation === conversation.id}
              onSelect={onSelect}
            />
          ))
        )}
      </div>

      <div
        className={`left-pane__resizer${resize.dragging ? ' left-pane__resizer--dragging' : ''}`}
        onPointerDown={resize.onPointerDown}
        onPointerMove={resize.onPointerMove}
        onPointerUp={resize.onPointerUp}
        onPointerCancel={resize.onPointerUp}
        onDoubleClick={resize.reset}
        onKeyDown={resize.onKeyDown}
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize sidebar"
        aria-valuenow={resize.width}
        aria-valuemin={MIN_LEFT_PANE_WIDTH}
        aria-valuemax={MAX_LEFT_PANE_WIDTH}
        tabIndex={0}
      />
    </div>
  );
}

/**
 * Drag the divider between the sidebar and the conversation.
 *
 * The width is held here rather than in the app state because nothing else
 * depends on it, and putting it in the shared reducer would re-render every
 * message bubble on every pixel of the drag.
 *
 * Pointer capture is what makes it survive a fast drag: without it the
 * pointer outruns the 7px handle, the element stops receiving moves, and the
 * divider is left behind halfway.
 */
function useResizableWidth() {
  const [width, setWidth] = React.useState(loadLeftPaneWidth);
  const [dragging, setDragging] = React.useState(false);
  const origin = React.useRef<{ x: number; width: number } | null>(null);

  const onPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    // Suppresses the text selection that a drag across the list would
    // otherwise start in whatever it passes over.
    event.preventDefault();
    origin.current = { x: event.clientX, width };
    setDragging(true);
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const onPointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const start = origin.current;
    if (!start) return;
    setWidth(clampLeftPaneWidth(start.width + event.clientX - start.x));
  };

  const onPointerUp = (event: React.PointerEvent<HTMLDivElement>) => {
    if (!origin.current) return;
    origin.current = null;
    setDragging(false);
    event.currentTarget.releasePointerCapture(event.pointerId);
    // Written once at the end of the drag, not on every move: this is a
    // synchronous write to disk-backed storage.
    saveLeftPaneWidth(width);
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    const step = event.shiftKey ? 40 : 8;
    const delta =
      event.key === 'ArrowLeft' ? -step : event.key === 'ArrowRight' ? step : 0;
    if (delta === 0) return;

    event.preventDefault();
    const next = clampLeftPaneWidth(width + delta);
    setWidth(next);
    saveLeftPaneWidth(next);
  };

  const reset = () => {
    setWidth(DEFAULT_LEFT_PANE_WIDTH);
    saveLeftPaneWidth(DEFAULT_LEFT_PANE_WIDTH);
  };

  return { width, dragging, onPointerDown, onPointerMove, onPointerUp, onKeyDown, reset };
}

function EmptyList({
  searching,
  contactCount,
  onBrowseContacts,
}: {
  searching: boolean;
  contactCount: number;
  onBrowseContacts: () => void;
}) {
  if (searching) {
    return <div className="left-pane__empty">No conversations found.</div>;
  }

  return (
    <div className="left-pane__empty">
      No conversations yet.
      <br />
      {contactCount > 0 ? (
        <>
          You know {contactCount === 1 ? 'one person' : `${contactCount} people`}.
          <br />
          <button className="left-pane__empty-action" onClick={onBrowseContacts}>
            Start a conversation
          </button>
        </>
      ) : (
        <>
          Add a contact to get started.
          <br />
          <button className="left-pane__empty-action" onClick={onBrowseContacts}>
            Contacts
          </button>
        </>
      )}
    </div>
  );
}

type RowProps = {
  conversation: Conversation;
  state: State;
  selected: boolean;
  onSelect: (id: string) => void;
};

function ConversationRow({ conversation, state, selected, onSelect }: RowProps) {
  const messages = state.messagesByThread[conversation.id] ?? [];
  const latest = messages.length > 0 ? messages[messages.length - 1] : null;
  const draft = state.drafts[conversation.id];

  // Unread is anything incoming we have not marked seen.
  const unreadCount = messages.filter((message) => !message.outgoing && !message.seen).length;

  const preview = draft
    ? draft
    : // A tombstone has no text and no attachments, so the ordinary snippet is
      // empty — and a row showing a name, a time and nothing else reads as a
      // rendering fault rather than as a message somebody withdrew.
      latest?.deletedAt
      ? 'This message was deleted'
      : latest
        ? snippet(latest.text, latest.attachments.length)
        : conversation.invitationPending
          ? 'You have been invited to this group'
          : '';

  const className = [
    'conversation-row',
    selected && 'conversation-row--selected',
    unreadCount > 0 && !selected && 'conversation-row--unread',
  ]
    .filter(Boolean)
    .join(' ');

  return (
    <button
      className={className}
      onClick={() => onSelect(conversation.id)}
      role="listitem"
      aria-current={selected}
    >
      <Avatar
        id={conversation.id}
        name={conversation.name}
        images={
          state.groups[conversation.id]?.images ?? state.users[conversation.id]?.images
        }
        size={48}
        online={conversation.online}
      />

      <div className="conversation-row__body">
        <div className="conversation-row__top">
          <span className="conversation-row__name">{conversation.name}</span>
          {conversation.lastActivity > 0 && (
            <span className="conversation-row__time">
              {conversationTimestamp(conversation.lastActivity)}
            </span>
          )}
        </div>

        <div className="conversation-row__bottom">
          <span
            className={[
              'conversation-row__snippet',
              draft && 'conversation-row__snippet--draft',
              !draft && latest?.deletedAt && 'conversation-row__snippet--deleted',
            ]
              .filter(Boolean)
              .join(' ')}
          >
            {preview}
          </span>
          {/* Signal puts the tick here too, so the list answers "did that
              send?" without opening the thread. Only when the newest message
              is ours and is not being displaced by a draft. */}
          {!draft && latest?.outgoing && showsDeliveryState(latest, state.profile?.id) && (
            <DeliveryTick
              state={deliveryState(latest)}
              className="conversation-row__status"
            />
          )}
          {unreadCount > 0 && !selected && (
            <span className="conversation-row__badge">
              {unreadCount > 99 ? '99+' : unreadCount}
            </span>
          )}
        </div>
      </div>
    </button>
  );
}
