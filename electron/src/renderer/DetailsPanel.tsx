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
import { choosePicture, prepareImage } from './outgoing-image';
import { shortAddress } from './format';
import type { Group, Settings, User } from '../preload';
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

/**
 * What a conversation's receipt and indicator settings fall back to when it has
 * no override, used only to label the first option of the tri-state selects.
 *
 * True on both counts because that is what the Fyne client ships and what the
 * settings panel falls back to; a label is the only thing this affects, and one
 * that guesses wrong is better than a control that renders "Default (?)".
 */
const FALLBACK_DEFAULTS = { readReceipts: true, typingIndicators: true };

type DetailsProps = {
  conversation: Conversation;
  state: State;
  onClose: () => void;
};

export function DetailsPanel({ conversation, state, onClose }: DetailsProps) {
  const group = state.groups[conversation.id];
  const user = state.users[conversation.id];
  const defaults = useProfileDefaults(state);
  const [pictureError, setPictureError] = React.useState<string | null>(null);

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
          {/*
            A group picture is shared state that any admin may change, so the
            control follows the same permission as renaming. A contact's
            picture is theirs to set, not ours, so it is never a button.
          */}
          {group && (group.admins.includes(state.profile?.id ?? '') || !group.restrictGroupEdits) ? (
            <button
              className="settings__avatar-button"
              onClick={() => void pickGroupPicture(group.id, setPictureError)}
              title="Change the group picture"
              aria-label="Change the group picture"
            >
              <Avatar
                id={conversation.id}
                name={conversation.name}
                images={group.images}
                size={96}
              />
              <span className="settings__avatar-overlay">Change</span>
            </button>
          ) : (
            <Avatar
              id={conversation.id}
              name={conversation.name}
              images={group ? group.images : user?.images}
              size={96}
            />
          )}
          <div className="details__name">{conversation.name}</div>
          {pictureError && <p className="settings__error">{pictureError}</p>}
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

        {/*
          Keyed by conversation so that selecting a different one remounts the
          body. The edit fields below seed their state from props once, and a
          panel that stayed mounted across a switch would keep the previous
          contact's text — and write it to the new contact on the next blur.
        */}
        {group ? (
          <GroupDetails key={group.id} group={group} state={state} />
        ) : (
          user && <ContactDetails key={user.id} user={user} state={state} defaults={defaults} />
        )}
      </div>
    </aside>
  );
}

/**
 * The profile-wide defaults, for labelling "Default (On)" and "Default (Off)".
 *
 * `state.settings` is only filled in once the engine has emitted a
 * `settingsUpdated`, which may not happen in a session, so the panel asks for
 * them itself and lets a later event win.
 */
function useProfileDefaults(state: State): Settings | null {
  const [fetched, setFetched] = React.useState<Settings | null>(null);

  React.useEffect(() => {
    if (state.settings) return;
    let cancelled = false;

    void window.bounce
      .settings()
      .then((settings) => {
        if (!cancelled) setFetched(settings);
      })
      .catch((error: unknown) => console.warn('could not read the profile defaults:', error));

    return () => {
      cancelled = true;
    };
  }, [state.settings]);

  return state.settings ?? fetched;
}

/**
 * A control's value, owned by the engine but not waiting for it.
 *
 * Every write here is echoed back as a `userUpdated` or `groupUpdated` event,
 * so the stored value is the source of truth and a change made on another
 * device still reaches the control. Keeping a copy beside it is what stops a
 * select snapping back to its old value for the length of a round trip, which
 * is what made the retention select look inert.
 */
function useStoredValue<T>(stored: T): [T, (next: T) => void] {
  const [value, setValue] = React.useState(stored);
  const [seed, setSeed] = React.useState(stored);

  // Adjusted during render rather than in an effect, which is React's own
  // advice for resetting state from a prop: nothing ever paints the stale one.
  if (seed !== stored) {
    setSeed(stored);
    setValue(stored);
  }

  return [value, setValue];
}

/**
 * Unblock a contact and put the conversation back in the list.
 *
 * Go ties the two together in the reducer — `SetBlocked` also sets
 * `open = !blocked` (`chat/update_dm.go:382-383`) — while the Rust reducer
 * applies only the flag it was handed (`engine/mod.rs:1333-1334`). The second
 * call is this client's half of that pairing, so unblocking cannot leave
 * someone unblocked but with no thread to reach them through.
 */
export async function unblockContact(userId: string): Promise<void> {
  await window.bounce.setUserBlocked(userId, false);
  await window.bounce.setOpenDm(userId, true);
}

/* ------------------------------------------------------------------ */

function ContactDetails({
  user,
  state,
  defaults,
}: {
  user: User;
  state: State;
  defaults: Settings | null;
}) {
  const [alias, setAlias] = React.useState(user.alias);
  // Seeded from the stored note, and re-seeded when one arrives from another
  // device, the way `SetUserState` re-seeds the Fyne entry (`ui/user.go:131`).
  const [notes, setNotes] = useStoredValue(user.notes);
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
              onBlur={() => {
                // Go saves from a button and cancels back to the stored text
                // (`ui/direct_message.go:780-791`); a blur is this panel's save
                // button, so it has to be a save of *something*. An unchanged
                // write is not harmless: `SetNotes` syncs to your own devices
                // and leaves no record to replay back from.
                if (notes !== user.notes) void window.bounce.setUserNotes(user.id, notes);
              }}
            />
          </Field>
        </Section>
      )}

      <ConversationSettings
        conversationId={user.id}
        mutedUntil={user.mutedUntil}
        retention={user.retention}
        defaults={defaults}
        // A `User` carries exactly the four fields `Overrides` names.
        overrides={user}
      />

      {!isSelf && (
        <Section title="Danger zone">
          <DestructiveButton
            label="Clear chat history"
            confirm={`Clear the history of your conversation with ${user.name}? This removes it for both of you.`}
            onConfirm={() => window.bounce.clearHistory(user.id)}
          />
          {/*
            The two halves of what people mean by "delete this chat": the
            messages go, and the row leaves the list. The contact stays,
            because there is nothing useful to delete — their devices are what
            make your remaining history verifiable, and a shared group would
            recreate the record anyway.
          */}
          <DestructiveButton
            label="Delete conversation"
            confirm={
              `Delete your conversation with ${user.name}? The messages are removed for ` +
              'both of you and it leaves your chat list. They stay in your contacts, and ' +
              'the conversation comes back if either of you writes again.'
            }
            onConfirm={() => deleteConversation(user.id)}
          />
          {user.blocked ? (
            <button className="details__action" onClick={() => void unblockContact(user.id)}>
              Unblock {user.name}
            </button>
          ) : (
            <DestructiveButton
              label={`Block ${user.name}`}
              // Blocking takes the conversation out of the list, so the
              // sentence says where the contact went and how to get them back.
              // There is no directory to re-find anyone through, and re-pairing
              // in person does not clear the flag (`engine/mod.rs:1079-1086`),
              // so an unadvertised route back is the same as no route back.
              confirm={
                `Block ${user.name}? Their messages will be refused rather than hidden, ` +
                'and the conversation leaves your list. You can unblock them from ' +
                'Settings, under Contacts.'
              }
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
                <Avatar
                  id={contact.id}
                  name={contact.alias || contact.name}
                  images={contact.images}
                  size={28}
                />
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
              <Avatar
                id={memberId}
                name={realName}
                images={state.users[memberId]?.images}
                size={28}
              />
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
              <Avatar
                id={inviteId}
                name={displayName}
                images={state.users[inviteId]?.images}
                size={28}
              />
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

/**
 * The per-conversation receipt and indicator state, as the views carry it.
 *
 * Two booleans rather than one nullable flag because that is the shape the
 * protocol writes — `vec![override_flag, value]` (`engine/mod.rs:1452-1480`),
 * the same two bytes Go sends — and the value byte is meaningless while the
 * override byte is clear.
 */
type Overrides = {
  readReceiptsOverridden: boolean;
  readReceiptsEnabled: boolean;
  typingIndicatorsOverridden: boolean;
  typingIndicatorsEnabled: boolean;
};

/** The three states Go's override selectors offer, in Go's order. */
export type Override = 'default' | 'on' | 'off';

/**
 * Which of the three a stored override reads as.
 *
 * Deliberately does not consult `enabled` while `overridden` is false, as
 * `refreshReadReceiptSettingSelection` does not (`ui/direct_message.go:125-137`):
 * a conversation that has never been overridden still carries a value byte, and
 * reading it would show "Off" for a conversation that follows an "On" default.
 */
export function overrideSelection(overridden: boolean, enabled: boolean): Override {
  if (!overridden) return 'default';
  return enabled ? 'on' : 'off';
}

/** What the bridge is told for a selection; null means "follow the profile". */
export function overrideSetting(selection: Override): boolean | null {
  if (selection === 'default') return null;
  return selection === 'on';
}

/**
 * The first option's label, which tracks the profile-wide default.
 *
 * Go relabels it whenever the default moves (`ui/settings_container.go:288-308`)
 * rather than writing a bare "Default", so choosing it is never a guess about
 * what you would be following.
 */
export function defaultOptionLabel(enabled: boolean): string {
  return enabled ? 'Default (On)' : 'Default (Off)';
}

/** Mute and retention, which work the same for contacts and groups. */
function ConversationSettings({
  conversationId,
  mutedUntil,
  retention,
  retentionDisabled,
  overrides,
  defaults,
}: {
  conversationId: string;
  mutedUntil: number;
  retention?: number;
  retentionDisabled?: boolean;
  /** Omitted while a conversation's view does not carry the override state. */
  overrides?: Overrides;
  defaults?: Settings | null;
}) {
  const muted = mutedUntil !== 0;
  const [seconds, setSeconds] = useStoredValue(retention ?? 0);

  return (
    <>
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
            value={seconds}
            disabled={retentionDisabled}
            title={retentionDisabled ? 'Only admins can change this' : undefined}
            onChange={(event) => {
              const chosen = Number(event.target.value);
              setSeconds(chosen);
              void window.bounce.setRetention(conversationId, chosen);
            }}
          >
            {RETENTION_OPTIONS.map((option) => (
              <option key={option.seconds} value={option.seconds}>
                {option.label}
              </option>
            ))}
          </select>
        </Field>
      </Section>

      {/*
        Hidden rather than shown empty when the view has no override state:
        a select seeded from nothing reads "Default" for a conversation that
        was overridden elsewhere, which is worse than not offering the control.
      */}
      {overrides && (
        <AdvancedOptions
          conversationId={conversationId}
          overrides={overrides}
          defaults={defaults ?? null}
        />
      )}
    </>
  );
}

/**
 * Read receipts and typing indicators for one conversation.
 *
 * Behind a disclosure because Go puts them behind an accordion on both edit
 * screens (`ui/direct_message.go:961-972`): they are settings you go looking
 * for, and a panel that leads with them reads as a privacy checklist rather
 * than a conversation.
 */
function AdvancedOptions({
  conversationId,
  overrides,
  defaults,
}: {
  conversationId: string;
  overrides: Overrides;
  defaults: Settings | null;
}) {
  // Collapsed, as Go's accordion is — except for a conversation that already
  // carries an override, which opens showing it. A privacy setting that
  // differs from your default should not be behind a click you have no reason
  // to make.
  const [open, setOpen] = React.useState(
    overrides.readReceiptsOverridden || overrides.typingIndicatorsOverridden,
  );

  return (
    <Section
      title="Advanced options"
      action={
        <button className="details__link" onClick={() => setOpen((value) => !value)}>
          {open ? 'Hide' : 'Show'}
        </button>
      }
    >
      {open && (
        <>
          <OverrideSelect
            label="Read receipts"
            overridden={overrides.readReceiptsOverridden}
            enabled={overrides.readReceiptsEnabled}
            defaultEnabled={defaults?.defaultReadReceipts ?? FALLBACK_DEFAULTS.readReceipts}
            onChange={(setting) => void window.bounce.setReadReceipts(conversationId, setting)}
          />
          <OverrideSelect
            label="Typing indicators"
            overridden={overrides.typingIndicatorsOverridden}
            enabled={overrides.typingIndicatorsEnabled}
            defaultEnabled={
              defaults?.defaultTypingIndicators ?? FALLBACK_DEFAULTS.typingIndicators
            }
            onChange={(setting) =>
              void window.bounce.setTypingIndicators(conversationId, setting)
            }
          />
          <p className="details__hint">
            These apply to this conversation only. Following the default means it moves
            when you change it in Settings.
          </p>
        </>
      )}
    </Section>
  );
}

function OverrideSelect({
  label,
  overridden,
  enabled,
  defaultEnabled,
  onChange,
}: {
  label: string;
  overridden: boolean;
  enabled: boolean;
  defaultEnabled: boolean;
  onChange: (setting: boolean | null) => void;
}) {
  const [selection, setSelection] = useStoredValue(overrideSelection(overridden, enabled));

  return (
    <Field label={label}>
      <select
        className="details__input"
        value={selection}
        onChange={(event) => {
          const chosen = event.target.value as Override;
          setSelection(chosen);
          onChange(overrideSetting(chosen));
        }}
      >
        <option value="default">{defaultOptionLabel(defaultEnabled)}</option>
        <option value="on">On</option>
        <option value="off">Off</option>
      </select>
    </Field>
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

/** Choose a picture and set it as the group's, reporting any refusal. */
async function pickGroupPicture(
  groupId: string,
  onError: (message: string | null) => void,
) {
  onError(null);
  const file = await choosePicture();
  if (!file) return;

  try {
    await window.bounce.setGroupImage(groupId, await prepareImage(file));
  } catch (failure) {
    onError(failure instanceof Error ? failure.message : String(failure));
  }
}

/**
 * Clear a conversation and take it off the list.
 *
 * Two existing operations rather than a new one, in the order that matters:
 * the history goes first, so that if closing fails the user is left with the
 * outcome they asked for rather than an empty conversation still sitting
 * there. Nothing about the contact is deleted.
 */
async function deleteConversation(userId: string): Promise<void> {
  await window.bounce.clearHistory(userId);
  await window.bounce.setOpenDm(userId, false);
}
