/**
 * The left pane: search, the conversation list, and the compose actions.
 */

import * as React from 'react';

import { Avatar } from './Avatar';
import { ComposeIcon, NewGroupIcon, SearchIcon, SettingsIcon } from './icons';
import { conversationTimestamp, snippet } from './format';
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

  return (
    <div className="left-pane">
      <div className="left-pane__header">
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
              size={28}
            />
          </button>
        )}

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

        <div className="left-pane__actions">
          {/* The compose button opens the contact store, the way Go's menu
              reaches `showNewDM` (`ui/menu.go:71`) — starting a conversation
              is picking somebody you already know, and adding somebody new is
              a step inside that. */}
          <button className="icon-button" onClick={onBrowseContacts} title="New conversation">
            <ComposeIcon />
          </button>
          <button className="icon-button" onClick={onNewGroup} title="New group">
            <NewGroupIcon />
          </button>
          <button className="icon-button" onClick={onOpenSettings} title="Settings">
            <SettingsIcon />
          </button>
        </div>
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
    </div>
  );
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
            className={
              draft
                ? 'conversation-row__snippet conversation-row__snippet--draft'
                : 'conversation-row__snippet'
            }
          >
            {preview}
          </span>
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
