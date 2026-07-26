/**
 * The conversation details panel.
 *
 * Everything about one conversation that is not the messages: who is in it,
 * what it is called, how long it is kept, and the destructive actions. The Fyne
 * client puts these on dedicated screens; on a desktop-sized window a slide-in
 * panel keeps the conversation visible beside them.
 *
 * Controls that require permissions the user does not hold are *shown disabled*
 * rather than hidden, so the interface explains why an action is unavailable
 * instead of silently lacking it.
 */

import * as React from 'react';

import { Avatar } from './Avatar';
import { CloseIcon } from './icons';
import { shortAddress } from './format';
import type { Group, User } from '../preload';
import type { Conversation, State } from './state';

/** Retention choices, matching what the Fyne client offers. */
const RETENTION_OPTIONS: ReadonlyArray<{ label: string; seconds: number }> = [
  { label: 'Off', seconds: 0 },
  { label: '1 day', seconds: 24 * 60 * 60 },
  { label: '1 week', seconds: 7 * 24 * 60 * 60 },
  { label: '4 weeks', seconds: 4 * 7 * 24 * 60 * 60 },
  { label: '1 year', seconds: 365 * 24 * 60 * 60 },
];

const MUTED_FOREVER = -1;

type DetailsProps = {
  conversation: Conversation;
  state: State;
  onClose: () => void;
};

export function DetailsPanel({ conversation, state, onClose }: DetailsProps) {
  const group = state.groups[conversation.id];
  const user = state.users[conversation.id];

  return (
    <aside className="details" aria-label="Conversation details">
      <header className="details__header">
        <button className="icon-button" onClick={onClose} title="Close">
          <CloseIcon />
        </button>
        <span className="details__header-title">
          {group ? 'Group info' : 'Contact info'}
        </span>
      </header>

      <div className="details__body">
        <div className="details__identity">
          <Avatar id={conversation.id} name={conversation.name} size={96} />
          <div className="details__name">{conversation.name}</div>
          {group ? (
            <div className="details__subtitle">
              {group.members.length} {group.members.length === 1 ? 'member' : 'members'}
              {group.invites.length > 0 && `, ${group.invites.length} invited`}
            </div>
          ) : (
            <div className="details__subtitle">
              {user?.online ? 'Online' : 'Offline'}
            </div>
          )}
        </div>

        {group ? (
          <GroupDetails group={group} state={state} />
        ) : (
          user && <ContactDetails user={user} state={state} />
        )}
      </div>
    </aside>
  );
}

/* ------------------------------------------------------------------ */

function ContactDetails({ user, state }: { user: User; state: State }) {
  const [alias, setAlias] = React.useState(user.alias);
  const [notes, setNotes] = React.useState('');
  const isSelf = state.profile?.id === user.id;

  return (
    <>
      <Section title="Identity">
        {/*
          There is no directory and no verification service, so a contact's
          address is the only thing that identifies them — it is the encoding
          of their public key. Showing it is how you confirm, out of band, that
          you are talking to who you think.
        */}
        <Field label="Address">
          <code className="details__address" title={user.id}>
            {shortAddress(user.id)}
          </code>
        </Field>
        <p className="details__hint">
          A contact&rsquo;s address is their public key. Compare it in person to be
          certain who you are talking to.
        </p>
      </Section>

      {!isSelf && (
        <Section title="Local details">
          <p className="details__hint">Only you can see these.</p>
          <Field label="Nickname">
            <input
              className="details__input"
              value={alias}
              placeholder={user.name}
              onChange={(event) => setAlias(event.target.value)}
              onBlur={() => void window.bounce.setUserAlias(user.id, alias.trim())}
            />
          </Field>
          <Field label="Notes">
            <textarea
              className="details__input details__input--multiline"
              value={notes}
              rows={3}
              onChange={(event) => setNotes(event.target.value)}
              onBlur={() => void window.bounce.setUserNotes(user.id, notes)}
            />
          </Field>
        </Section>
      )}

      <ConversationSettings conversationId={user.id} mutedUntil={user.mutedUntil} />

      {!isSelf && (
        <Section title="Danger zone">
          <DestructiveButton
            label="Clear chat history"
            confirm={`Clear the history of your conversation with ${user.name}? This removes it for both of you.`}
            onConfirm={() => window.bounce.clearHistory(user.id)}
          />
          {user.blocked ? (
            <button
              className="details__action"
              onClick={() => void window.bounce.setUserBlocked(user.id, false)}
            >
              Unblock {user.name}
            </button>
          ) : (
            <DestructiveButton
              label={`Block ${user.name}`}
              confirm={`Block ${user.name}? Their messages will be refused rather than hidden.`}
              onConfirm={() => window.bounce.setUserBlocked(user.id, true)}
            />
          )}
        </Section>
      )}
    </>
  );
}

/* ------------------------------------------------------------------ */

function GroupDetails({ group, state }: { group: Group; state: State }) {
  const myId = state.profile?.id ?? '';
  const isAdmin = group.admins.includes(myId);
  const [name, setName] = React.useState(group.name);
  const [inviting, setInviting] = React.useState(false);

  // Contacts not already in the group or invited to it.
  const invitable = Object.values(state.users).filter(
    (user) =>
      user.id !== myId &&
      !user.blocked &&
      !group.members.includes(user.id) &&
      !group.invites.includes(user.id),
  );

  const canEdit = isAdmin || !group.restrictGroupEdits;
  const canManageUsers = isAdmin || !group.restrictUserManagement;

  return (
    <>
      <Section title="Group">
        <Field label="Name">
          <input
            className="details__input"
            value={name}
            disabled={!canEdit}
            title={canEdit ? undefined : 'Only admins can rename this group'}
            onChange={(event) => setName(event.target.value)}
            onBlur={() => {
              const trimmed = name.trim();
              if (trimmed && trimmed !== group.name) {
                void window.bounce.renameGroup(group.id, trimmed);
              } else {
                setName(group.name);
              }
            }}
          />
        </Field>
      </Section>

      <Section
        title={`Members (${group.members.length})`}
        action={
          canManageUsers && invitable.length > 0 ? (
            <button className="details__link" onClick={() => setInviting((open) => !open)}>
              {inviting ? 'Done' : 'Invite'}
            </button>
          ) : undefined
        }
      >
        {inviting && (
          <div className="details__invite-list">
            {invitable.map((contact) => (
              <button
                key={contact.id}
                className="details__member"
                onClick={() => {
                  void window.bounce.inviteToGroup(group.id, contact.id);
                  setInviting(false);
                }}
              >
                <Avatar id={contact.id} name={contact.alias || contact.name} size={28} />
                <span className="details__member-name">{contact.alias || contact.name}</span>
                <span className="details__link">Invite</span>
              </button>
            ))}
          </div>
        )}

        {group.members.map((memberId) => {
          const member = state.users[memberId];
          const isMe = memberId === myId;
          const realName = isMe
            ? (state.profile?.name ?? 'You')
            : member
              ? member.alias || member.name
              : 'Unknown';
          // Labelled "You", but the avatar keeps the profile's own initials so
          // it matches every other place this person appears.
          const displayName = isMe ? 'You' : realName;
          const memberIsAdmin = group.admins.includes(memberId);

          return (
            <div key={memberId} className="details__member">
              <Avatar id={memberId} name={realName} size={28} />
              <span className="details__member-name">{displayName}</span>
              {memberIsAdmin && <span className="details__badge">Admin</span>}

              {isAdmin && memberId !== myId && (
                <MemberMenu
                  groupId={group.id}
                  memberId={memberId}
                  memberName={displayName}
                  isAdmin={memberIsAdmin}
                />
              )}
            </div>
          );
        })}

        {group.invites.map((inviteId) => {
          const invitee = state.users[inviteId];
          const displayName = invitee ? invitee.alias || invitee.name : 'Unknown';
          return (
            <div key={inviteId} className="details__member details__member--pending">
              <Avatar id={inviteId} name={displayName} size={28} />
              <span className="details__member-name">{displayName}</span>
              <span className="details__badge">Invited</span>
              {canManageUsers && (
                <button
                  className="details__link"
                  onClick={() => void window.bounce.revokeInvite(group.id, inviteId)}
                >
                  Revoke
                </button>
              )}
            </div>
          );
        })}
      </Section>

      <Section title="Permissions">
        {!isAdmin && <p className="details__hint">Only admins can change these.</p>}
        <Toggle
          label="Only admins can post"
          checked={group.restrictPosting}
          disabled={!isAdmin}
          onChange={(value) =>
            void window.bounce.setGroupPermission(group.id, 'posting', value)
          }
        />
        <Toggle
          label="Only admins can edit the group"
          checked={group.restrictGroupEdits}
          disabled={!isAdmin}
          onChange={(value) => void window.bounce.setGroupPermission(group.id, 'edits', value)}
        />
        <Toggle
          label="Only admins can manage members"
          checked={group.restrictUserManagement}
          disabled={!isAdmin}
          onChange={(value) =>
            void window.bounce.setGroupPermission(group.id, 'userManagement', value)
          }
        />
      </Section>

      <ConversationSettings
        conversationId={group.id}
        mutedUntil={group.mutedUntil}
        retention={group.retention}
        retentionDisabled={!canEdit}
      />

      <Section title="Danger zone">
        <DestructiveButton
          label="Clear chat history"
          confirm="Clear this group's history? This removes it for everyone."
          onConfirm={() => window.bounce.clearHistory(group.id)}
        />
        <DestructiveButton
          label="Leave group"
          confirm="Leave this group? You will stop receiving its messages."
          onConfirm={() => window.bounce.removeFromGroup(group.id, myId)}
        />
        <DestructiveButton
          label="Block group"
          confirm="Block this group? You will leave it and stop tracking any further changes."
          onConfirm={() => window.bounce.blockGroup(group.id)}
        />
        {isAdmin && (
          <DestructiveButton
            label="Delete group"
            confirm="Delete this group for everyone? This cannot be undone."
            onConfirm={() => window.bounce.deleteGroup(group.id)}
          />
        )}
      </Section>
    </>
  );
}

/* ------------------------------------------------------------------ */

/** Mute and retention, which work the same for contacts and groups. */
function ConversationSettings({
  conversationId,
  mutedUntil,
  retention,
  retentionDisabled,
}: {
  conversationId: string;
  mutedUntil: number;
  retention?: number;
  retentionDisabled?: boolean;
}) {
  const muted = mutedUntil !== 0;

  return (
    <Section title="Settings">
      <Toggle
        label="Mute notifications"
        checked={muted}
        onChange={(value) =>
          void window.bounce.setMutedUntil(conversationId, value ? MUTED_FOREVER : 0)
        }
      />

      <Field label="Disappearing messages">
        <select
          className="details__input"
          value={retention ?? 0}
          disabled={retentionDisabled}
          title={retentionDisabled ? 'Only admins can change this' : undefined}
          onChange={(event) =>
            void window.bounce.setRetention(conversationId, Number(event.target.value))
          }
        >
          {RETENTION_OPTIONS.map((option) => (
            <option key={option.seconds} value={option.seconds}>
              {option.label}
            </option>
          ))}
        </select>
      </Field>
    </Section>
  );
}

function MemberMenu({
  groupId,
  memberId,
  memberName,
  isAdmin,
}: {
  groupId: string;
  memberId: string;
  memberName: string;
  isAdmin: boolean;
}) {
  const [open, setOpen] = React.useState(false);

  React.useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    window.addEventListener('click', close);
    return () => window.removeEventListener('click', close);
  }, [open]);

  return (
    <div className="menu-anchor">
      <button
        className="details__link"
        onClick={(event) => {
          event.stopPropagation();
          setOpen((value) => !value);
        }}
      >
        Manage
      </button>
      {open && (
        <div className="menu" role="menu" onClick={(event) => event.stopPropagation()}>
          <button
            className="menu__item"
            onClick={() => {
              void window.bounce.setGroupAdmin(groupId, memberId, !isAdmin);
              setOpen(false);
            }}
          >
            {isAdmin ? 'Remove as admin' : 'Make admin'}
          </button>
          <button
            className="menu__item menu__item--destructive"
            onClick={() => {
              if (window.confirm(`Remove ${memberName} from the group?`)) {
                void window.bounce.removeFromGroup(groupId, memberId);
              }
              setOpen(false);
            }}
          >
            Remove from group
          </button>
        </div>
      )}
    </div>
  );
}

/* ------------------------------------------------------------------ */

function Section({
  title,
  action,
  children,
}: {
  title: string;
  action?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <section className="details__section">
      <div className="details__section-title">
        <span>{title}</span>
        {action}
      </div>
      {children}
    </section>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="details__field">
      <span className="details__field-label">{label}</span>
      {children}
    </label>
  );
}

function Toggle({
  label,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <label className={disabled ? 'details__toggle details__toggle--disabled' : 'details__toggle'}>
      <span>{label}</span>
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
    </label>
  );
}

/**
 * An action that is hard to undo, gated behind a confirmation.
 *
 * Leaving, blocking and clearing all reach other people's devices, so none of
 * them should be one stray click away.
 */
function DestructiveButton({
  label,
  confirm,
  onConfirm,
}: {
  label: string;
  confirm: string;
  onConfirm: () => Promise<void>;
}) {
  return (
    <button
      className="details__action details__action--destructive"
      onClick={() => {
        if (window.confirm(confirm)) {
          void onConfirm();
        }
      }}
    >
      {label}
    </button>
  );
}
