/**
 * The application shell: wiring, layout, and the dialogs that hang off it.
 */

import * as React from 'react';

import type { PendingAttachment } from './Attachments';
import { Avatar } from './Avatar';
import { blurHashFromImage } from './blurhash';
import { ConversationView, NoConversationSelected } from './Conversation';
import { MessageInfoPanel } from './MessageInfo';
import { QrCode } from './icons';
import { DetailsPanel } from './DetailsPanel';
import { LeftPane } from './LeftPane';
import {
  messageNotification,
  notify,
  shouldNotify,
  timelineIsAtBottom,
  useWindowFocus,
} from './notifications';
import { Onboarding } from './Onboarding';
import { SettingsPanel } from './SettingsPanel';
import {
  contacts as deriveContacts,
  conversations as deriveConversations,
  filterContacts,
  filterConversations,
  initialState,
  reducer,
  type Contact,
  type Conversation,
  type State,
} from './state';
import type { BounceApi, Message, OutgoingAttachment, TransportInfo } from '../preload';

declare global {
  interface Window {
    bounce: BounceApi;
  }
}

export function App() {
  const [state, dispatch] = React.useReducer(reducer, initialState);
  const [hasProfile, setHasProfile] = React.useState<boolean | null>(null);
  const [dialog, setDialog] = React.useState<'newGroup' | 'addContact' | 'contacts' | null>(null);
  const [transport, setTransport] = React.useState<TransportInfo | null>(null);
  const [detailsOpen, setDetailsOpen] = React.useState(false);
  /*
   * Somebody whose details are open who is *not* the selected conversation —
   * a member of the group you are reading, reached by clicking their face.
   *
   * Held separately rather than by selecting them, because opening a person's
   * card should not navigate away from the conversation you were reading.
   */
  const [detailsUser, setDetailsUser] = React.useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = React.useState(false);
  // The message behind the info drawer, or null. Held by value rather than by
  // id so the panel keeps showing what you opened even if the thread reloads
  // underneath it.
  const [infoMessage, setInfoMessage] = React.useState<Message | null>(null);

  const windowFocused = useWindowFocus();

  // The subscription below is installed once and never rebuilt, so everything
  // the notification rules consult is read through a ref at the moment a
  // message lands rather than captured when we subscribed.
  const notificationContext = React.useRef({ state, windowFocused });
  React.useEffect(() => {
    notificationContext.current = { state, windowFocused };
  });

  const announce = React.useCallback((message: Message) => {
    const { state: current, windowFocused: focused } = notificationContext.current;
    const group = current.groups[message.thread];
    const user = current.users[message.thread];

    if (
      !shouldNotify({
        conversationId: message.thread,
        outgoing: message.outgoing,
        mutedUntil: group ? group.mutedUntil : (user?.mutedUntil ?? 0),
        windowFocused: focused,
        activeConversation: current.selectedConversation,
        atBottom: timelineIsAtBottom(message.thread),
        syncing: current.syncing,
      })
    ) {
      return;
    }

    const author = current.users[message.author];
    const { title, body } = messageNotification(message, {
      authorName: author ? author.alias || author.name : 'Unknown',
      conversationName: group ? group.name : user ? user.alias || user.name : 'Bounce',
      isGroup: group !== undefined,
    });

    notify({
      title,
      body,
      conversationId: message.thread,
      onActivate: () => dispatch({ type: 'selectConversation', id: message.thread }),
    });
  }, []);

  // Subscribe before loading, so nothing emitted during start-up is missed.
  React.useEffect(() => {
    const unsubscribe = window.bounce.onEvent((event) => {
      dispatch({ type: 'engineEvent', event });
      if (event.type === 'messageReceived') announce(event.message);
    });
    return unsubscribe;
  }, [announce]);

  React.useEffect(() => {
    let cancelled = false;

    void (async () => {
      try {
        const [profileExists, address, transportInfo] = await Promise.all([
          window.bounce.hasProfile(),
          window.bounce.address(),
          window.bounce.transport(),
        ]);
        if (cancelled) return;

        setTransport(transportInfo);
        setHasProfile(profileExists);
        if (!profileExists) return;

        const snapshot = await window.bounce.initialState();
        if (cancelled) return;

        dispatch({ type: 'loaded', state: snapshot, address });
      } catch (error) {
        if (!cancelled) {
          dispatch({
            type: 'engineEvent',
            event: { type: 'error', message: String(error) },
          });
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [hasProfile]);

  // Follow the operating system's light or dark preference, the way Signal
  // does by default.
  React.useEffect(() => {
    const media = window.matchMedia('(prefers-color-scheme: dark)');
    const apply = (dark: boolean) => {
      document.documentElement.dataset.theme = dark ? 'dark' : 'light';
    };
    apply(media.matches);

    const listener = (event: MediaQueryListEvent) => apply(event.matches);
    media.addEventListener('change', listener);
    const unsubscribe = window.bounce.onThemeChange(apply);

    return () => {
      media.removeEventListener('change', listener);
      unsubscribe();
    };
  }, []);

  /*
   * Publish the window's visibility so CSS can stand animations down.
   *
   * Only the pending-delivery ring uses it today. It is an attribute rather
   * than React state deliberately: a hidden window flipping a state flag would
   * re-render the whole tree to pause an animation nobody is watching.
   */
  React.useEffect(() => {
    const apply = () => {
      document.documentElement.dataset.windowHidden = String(document.hidden);
    };
    apply();

    document.addEventListener('visibilitychange', apply);
    return () => document.removeEventListener('visibilitychange', apply);
  }, []);

  // The platform class drives the title bar inset on macOS.
  React.useEffect(() => {
    document.body.classList.add(`platform-${window.bounce.platform}`);
  }, []);

  const allConversations = React.useMemo(() => deriveConversations(state), [state]);
  const visibleConversations = React.useMemo(
    () => filterConversations(allConversations, state.searchQuery),
    [allConversations, state.searchQuery],
  );

  const selected = allConversations.find(
    (conversation) => conversation.id === state.selectedConversation,
  );

  /*
   * Whose card the details pane is showing.
   *
   * A group member reached by clicking their face may have no open
   * conversation at all — that is the ordinary case for somebody met through a
   * group — so the summary is built from the user record rather than looked up
   * in the sidebar's list, which would find nothing and show nothing.
   */
  const detailsSubject: Conversation | undefined = React.useMemo(() => {
    if (!detailsUser) return selected;

    const found = allConversations.find((conversation) => conversation.id === detailsUser);
    if (found) return found;

    const user = state.users[detailsUser];
    if (!user) return undefined;

    return {
      id: user.id,
      kind: 'direct',
      name: user.alias || user.name,
      memberCount: 0,
      lastActivity: user.lastActivity,
      online: user.online,
      muted: user.mutedUntil !== 0,
      invitationPending: false,
    };
  }, [detailsUser, selected, allConversations, state.users]);

  const openConversation = React.useCallback((id: string) => {
    dispatch({ type: 'selectConversation', id });

    // Displaying a thread is what stamps its last-opened time in Go
    // (`ui/direct_message.go:309-310`), which is what holds a thread carrying
    // a draft near the top of the list.
    void window.bounce.setLastOpened(id).catch(() => {
      // Losing the stamp costs ordering on the next launch, nothing more.
    });

    // Opening a conversation is a statement that the other side is
    // wanted; the engine's own peering pass is a minute away.
    void window.bounce.reachFor(id).catch(() => {
      // A dial that fails is retried by the peering audit.
    });

    setDetailsOpen(false);
    setSettingsOpen(false);
  }, []);

  // Starting a conversation from the contact list is what puts it in the
  // sidebar, and the flag syncs, so it opens on every device you own.
  const startConversation = React.useCallback(
    async (id: string) => {
      try {
        // Awaited rather than fired off, so the `userUpdated` it provokes has
        // landed before we select — otherwise the row we are selecting does
        // not exist yet and the pane flashes its empty state.
        await window.bounce.setOpenDm(id, true);
      } catch (error) {
        dispatch({ type: 'engineEvent', event: { type: 'error', message: String(error) } });
        return;
      }
      setDialog(null);
      openConversation(id);
    },
    [openConversation],
  );

  const handleSend = React.useCallback(
    async (
      text: string,
      attachments: readonly PendingAttachment[],
      replyTo?: string,
    ) => {
      if (!selected) return;
      try {
        if (attachments.length > 0) {
          // Dimensions come from the preview the composer already decoded; the
          // engine does not decode images.
          const outgoing = await Promise.all(attachments.map(measure));
          if (selected.kind === 'group') {
            await window.bounce.sendGroupMessageWithAttachments(
              selected.id,
              text,
              outgoing,
              replyTo,
            );
          } else {
            await window.bounce.sendDirectMessageWithAttachments(
              selected.id,
              text,
              outgoing,
              replyTo,
            );
          }
        } else if (selected.kind === 'group') {
          await window.bounce.sendGroupMessage(selected.id, text, replyTo);
        } else {
          await window.bounce.sendDirectMessage(selected.id, text, replyTo);
        }
        // Clearing the stored draft is what removes the "Draft:" prefix from
        // the conversation list.
        await window.bounce.saveDraft(selected.id, '');
        dispatch({ type: 'setDraft', thread: selected.id, text: '' });
      } catch (error) {
        dispatch({
          type: 'engineEvent',
          event: { type: 'error', message: String(error) },
        });
      }
    },
    [selected],
  );

  const handleDraftChange = React.useCallback(
    (text: string) => {
      if (!selected) return;
      dispatch({ type: 'setDraft', thread: selected.id, text });
      void window.bounce.saveDraft(selected.id, text).catch(() => {
        // A draft that fails to sync is not worth interrupting typing over; it
        // will be retried on the next keystroke.
      });

      // Announce composing. The engine throttles this, so calling it on every
      // keystroke is deliberate rather than wasteful.
      if (text.length > 0) {
        void window.bounce
          .typingIn(selected.id, selected.kind === 'group')
          .catch((error) => {
            // Not worth interrupting typing over, but silently swallowing it
            // makes a broken indicator impossible to diagnose.
            console.warn('typing indicator not sent:', error);
          });
      }
    },
    [selected],
  );

  // Opening a conversation marks its unread incoming messages as read. Whether
  // the author actually learns of it is the engine's decision, based on this
  // device's privacy settings.
  //
  // The engine also emits `messageSeen`, which flips `seen` in the reducer, so
  // a message drops out of this list once reported. `reportedRead` covers the
  // gap before that event arrives — without it the effect re-fires on the very
  // state change it caused and re-sends receipts indefinitely.
  const reportedRead = React.useRef(new Set<string>());

  const selectedId = selected?.id;
  const selectedIsGroup = selected?.kind === 'group';

  const unreadIds = React.useMemo(() => {
    if (!selectedId) return '';
    return (state.messagesByThread[selectedId] ?? [])
      .filter((message) => !message.outgoing && !message.seen)
      .map((message) => message.id)
      .join(',');
  }, [selectedId, state.messagesByThread]);

  React.useEffect(() => {
    // Go gates this on the thread being active AND the window being focused
    // (ui/chat_history.go:807). Without the second half, selecting a
    // conversation while the window is behind another one tells the author
    // their message was read by somebody who has not looked at it.
    if (!selectedId || unreadIds === '' || !windowFocused) return;

    for (const id of unreadIds.split(',')) {
      if (reportedRead.current.has(id)) continue;
      reportedRead.current.add(id);
      void window.bounce.markAsRead(id, selectedIsGroup).catch(() => {
        // Allow a retry if it did not get through.
        reportedRead.current.delete(id);
      });
    }
  }, [selectedId, selectedIsGroup, unreadIds, windowFocused]);

  if (hasProfile === null) {
    // The engine may have failed to start, in which case there is a reason to
    // show rather than an indefinite blank window.
    if (state.error) {
      return (
        <div className="onboarding">
          <div className="onboarding__title">Bounce could not start</div>
          <p className="onboarding__body">{state.error}</p>
        </div>
      );
    }
    return <div className="app" />;
  }

  if (!hasProfile) {
    return <Onboarding onCreated={() => setHasProfile(true)} />;
  }

  return (
    <div className="app">
      {/* Banners stack rather than share the grid row they are placed in, so
          two conditions at once read as two lines instead of overlapping. */}
      <div className="app__banners">
        {/* Nothing this device does will reach anyone once it has been
            revoked, so this outranks every other warning. */}
        {state.deviceRevoked && (
          <div className="banner banner--revoked" role="alert">
            This device has been revoked
          </div>
        )}

        {transport && !transport.anonymous && (
          <div className="banner banner--insecure" role="alert">
            Running without Tor — this connection protects no metadata.
          </div>
        )}

        {!state.networkOnline && (
          <div className="banner banner--offline" role="status">
            {state.networkStarting
              ? 'network is starting...'
              : 'network connection lost, reconnecting...'}
          </div>
        )}

        {state.error && (
          <div className="banner banner--error" role="alert">
            <span style={{ flex: 1 }}>{state.error}</span>
            <button
              className="banner__dismiss"
              onClick={() => dispatch({ type: 'dismissError' })}
              aria-label="Dismiss"
            >
              ×
            </button>
          </div>
        )}
      </div>

      <LeftPane
        state={state}
        conversations={visibleConversations}
        onSelect={openConversation}
        onSearch={(query) => dispatch({ type: 'search', query })}
        onNewGroup={() => setDialog('newGroup')}
        onBrowseContacts={() => setDialog('contacts')}
        onOpenSettings={() => {
          setSettingsOpen(true);
          setDetailsOpen(false);
        }}
      />

      {selected ? (
        <ConversationView
          key={selected.id}
          conversation={selected}
          state={state}
          onSend={handleSend}
          onShowUser={(userId) => {
            // Our own face opens our own settings, which is where our profile
            // actually lives; a contact card of ourselves would be a dead end.
            if (userId === state.profile?.id) {
              setDetailsOpen(false);
              setSettingsOpen(true);
              return;
            }
            setDetailsUser(userId);
            setDetailsOpen(true);
            setSettingsOpen(false);
          }}
          onDraftChange={handleDraftChange}
          onAcceptInvite={() => void window.bounce.respondToInvite(selected.id, true)}
          onDeclineInvite={() => void window.bounce.respondToInvite(selected.id, false)}
          onLeaveGroup={() => {
            void window.bounce.leaveGroup(selected.id);
            dispatch({ type: 'selectConversation', id: null });
          }}
          onCopyAddress={() => void navigator.clipboard.writeText(state.address)}
          onShowDetails={() => setDetailsOpen(true)}
          onShowMessageInfo={(message) => {
            // The three right-hand panels share one column, so opening this
            // has to close whichever of the others was up.
            setDetailsOpen(false);
            setSettingsOpen(false);
            setInfoMessage(message);
          }}
          onError={(message) => dispatch({ type: 'engineEvent', event: { type: 'error', message } })}
        />
      ) : (
        <NoConversationSelected address={state.address} />
      )}

      {/* Both are 340px right-hand asides, so they are mutually exclusive. */}
      {infoMessage && (
        <MessageInfoPanel
          message={infoMessage}
          state={state}
          onClose={() => setInfoMessage(null)}
        />
      )}

      {detailsSubject && detailsOpen && !settingsOpen && !infoMessage && (
        <DetailsPanel
          /*
           * Namespaced, not the bare id.
           *
           * These panes are siblings in one children array, and
           * `ConversationView` is keyed on the selected conversation — which is
           * what `detailsSubject` falls back to. A bare id therefore collided
           * whenever the details pane was showing the conversation you were
           * already in, and React does not recover gracefully from duplicate
           * keys in one array: the previous conversation was left mounted
           * underneath the new one, and state updates landed on the wrong
           * fiber, so controls inside the pane stopped responding.
           */
          key={`details-${detailsSubject.id}`}
          conversation={detailsSubject}
          state={state}
          onClose={() => {
            setDetailsOpen(false);
            setDetailsUser(null);
          }}
        />
      )}

      {settingsOpen && !infoMessage && (
        <SettingsPanel state={state} onClose={() => setSettingsOpen(false)} />
      )}

      {dialog === 'contacts' && (
        <ContactsDialog
          state={state}
          onStart={startConversation}
          onHide={(id) =>
            void window.bounce.setOpenDm(id, false).catch((error) => {
              dispatch({ type: 'engineEvent', event: { type: 'error', message: String(error) } });
            })
          }
          onAddContact={() => setDialog('addContact')}
          onClose={() => setDialog(null)}
        />
      )}

      {dialog === 'newGroup' && (
        // Built from the contact store rather than from the sidebar: the
        // sidebar carries a note-to-self row, and inviting yourself to a group
        // you are already in costs a signed frame and a nonsense status line
        // (`ui/new_group_container.go:340-347` skips the profile too).
        <NewGroupDialog contacts={deriveContacts(state)} onClose={() => setDialog(null)} />
      )}

      {dialog === 'addContact' && <AddContactDialog onClose={() => setDialog(null)} />}
    </div>
  );
}

/**
 * The contact store, as a browser.
 *
 * The counterpart to the sidebar: it lists everyone regardless of whether a
 * conversation is open with them, which is the only way back to somebody the
 * sidebar is not showing — Bounce has no directory, so a contact you cannot
 * reach from here is a contact you would have to re-pair with in person.
 * Modelled on `ui/new_dm_container.go`, down to the "Show blocked" checkbox.
 */
function ContactsDialog({
  state,
  onStart,
  onHide,
  onAddContact,
  onClose,
}: {
  state: State;
  onStart: (id: string) => void;
  onHide: (id: string) => void;
  onAddContact: () => void;
  onClose: () => void;
}) {
  const [query, setQuery] = React.useState('');
  const [showBlocked, setShowBlocked] = React.useState(false);

  const visible = React.useMemo(
    () => filterContacts(deriveContacts(state, showBlocked), query),
    [state, showBlocked, query],
  );

  return (
    <div className="modal__backdrop" onClick={onClose}>
      <div className="modal modal--wide" onClick={(event) => event.stopPropagation()}>
        <div className="modal__title">Contacts</div>

        <input
          className="onboarding__field"
          placeholder="Search contacts"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          aria-label="Search contacts"
          autoFocus
        />

        <label className="contacts__filter">
          <input
            type="checkbox"
            checked={showBlocked}
            onChange={(event) => setShowBlocked(event.target.checked)}
          />
          Show blocked
        </label>

        <div className="contacts__list" role="list">
          {visible.length === 0 ? (
            <div className="contacts__empty">
              {query.trim().length > 0
                ? 'No contacts found.'
                : 'Nobody here yet. Contacts are added by exchanging a code in person.'}
            </div>
          ) : (
            visible.map((contact) => (
              <ContactRow
                key={contact.id}
                contact={contact}
                onStart={onStart}
                onHide={onHide}
              />
            ))
          )}
        </div>

        <div className="modal__actions">
          <button className="modal__button" onClick={onClose}>
            Close
          </button>
          <button className="modal__button modal__button--primary" onClick={onAddContact}>
            Add contact
          </button>
        </div>
      </div>
    </div>
  );
}

function ContactRow({
  contact,
  onStart,
  onHide,
}: {
  contact: Contact;
  onStart: (id: string) => void;
  onHide: (id: string) => void;
}) {
  return (
    <div className="contact-row" role="listitem">
      <Avatar
        id={contact.id}
        name={contact.name}
        images={contact.images}
        size={32}
        online={contact.online}
      />
      <span className="contact-row__name">{contact.name}</span>

      {contact.blocked ? (
        // Listed so you can see who you blocked, but not a way in: the sidebar
        // refuses blocked rows, so opening one would select a conversation
        // that does not render. Unblocking lives in the conversation details.
        <span className="contact-row__tag">Blocked</span>
      ) : (
        <>
          {/* Offered only when it would visibly do something. A conversation
              with messages in it stays in the sidebar whatever the flag says,
              so a Hide button on one would look broken. */}
          {contact.hideable && (
            <button className="contact-row__action" onClick={() => onHide(contact.id)}>
              Hide
            </button>
          )}
          <button
            className="contact-row__action contact-row__action--primary"
            onClick={() => onStart(contact.id)}
          >
            {contact.open ? 'Open' : 'Message'}
          </button>
        </>
      )}
    </div>
  );
}

function NewGroupDialog({
  contacts,
  onClose,
}: {
  contacts: Contact[];
  onClose: () => void;
}) {
  const [name, setName] = React.useState('');
  const [selected, setSelected] = React.useState<string[]>([]);
  const [busy, setBusy] = React.useState(false);

  const toggle = (id: string) => {
    setSelected((current) =>
      current.includes(id) ? current.filter((value) => value !== id) : [...current, id],
    );
  };

  const create = async () => {
    setBusy(true);
    try {
      await window.bounce.createGroup(name.trim(), selected);
      onClose();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="modal__backdrop" onClick={onClose}>
      <div className="modal" onClick={(event) => event.stopPropagation()}>
        <div className="modal__title">New group</div>

        <input
          className="onboarding__field"
          placeholder="Group name"
          value={name}
          onChange={(event) => setName(event.target.value)}
          autoFocus
        />

        {contacts.length > 0 && (
          <div style={{ marginTop: 12, maxHeight: 220, overflowY: 'auto' }}>
            {contacts.map((contact) => (
              <label
                key={contact.id}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 10,
                  padding: '8px 4px',
                  cursor: 'pointer',
                }}
              >
                <input
                  type="checkbox"
                  checked={selected.includes(contact.id)}
                  onChange={() => toggle(contact.id)}
                />
                <span>{contact.name}</span>
              </label>
            ))}
          </div>
        )}

        <div className="modal__actions">
          <button className="modal__button" onClick={onClose}>
            Cancel
          </button>
          <button
            className="modal__button modal__button--primary"
            onClick={create}
            disabled={busy || name.trim().length === 0}
          >
            Create
          </button>
        </div>
      </div>
    </div>
  );
}

function AddContactDialog({ onClose }: { onClose: () => void }) {
  const [code, setCode] = React.useState('');
  const [copied, setCopied] = React.useState(false);

  // The confirmation reverts on its own; a button stuck on "Copied" reads as
  // broken the second time you use it.
  React.useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), 2000);
    return () => window.clearTimeout(timer);
  }, [copied]);
  const [theirCode, setTheirCode] = React.useState('');
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  // Generate on open. Showing a code invalidates any previous one, so this is
  // also what retires a code left on screen earlier.
  React.useEffect(() => {
    void window.bounce
      .createPairingCode()
      .then(setCode)
      .catch((failure) => setError(String(failure)));
  }, []);

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await window.bounce.requestToAddUser(theirCode.trim());
      onClose();
    } catch (failure) {
      setError(String(failure));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="modal__backdrop" onClick={onClose}>
      <div className="modal" onClick={(event) => event.stopPropagation()}>
        <div className="modal__title">Add a contact</div>

        <p style={{ margin: '0 0 12px', fontSize: 13, color: 'var(--text-secondary)' }}>
          There is no directory to search. Show this code to someone next to you, or
          paste theirs below. Codes are single-use and expire after five minutes.
        </p>

        {code && (
          <div style={{ display: 'flex', justifyContent: 'center', marginBottom: 12 }}>
            <QrCode text={code} size={196} />
          </div>
        )}

        <div className="settings__address-row" style={{ marginBottom: 16 }}>
          <code className="settings__address">{code || 'Generating…'}</code>
          <button
            className="settings__copy"
            disabled={!code}
            onClick={() => {
              void navigator.clipboard.writeText(code).then(() => setCopied(true));
            }}
          >
            {copied ? 'Copied' : 'Copy'}
          </button>
        </div>

        <input
          className="onboarding__field"
          placeholder="Paste their code"
          value={theirCode}
          onChange={(event) => setTheirCode(event.target.value)}
          autoFocus
        />

        {error && (
          <div style={{ marginTop: 8, fontSize: 12, color: 'var(--red)' }}>{error}</div>
        )}

        <div className="modal__actions">
          <button className="modal__button" onClick={onClose}>
            Cancel
          </button>
          <button
            className="modal__button modal__button--primary"
            onClick={submit}
            disabled={busy || theirCode.trim().length === 0}
          >
            {busy ? 'Connecting…' : 'Add'}
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * Read an image's dimensions, so the recipient can reserve the right space
 * before the pixels arrive.
 *
 * A file that will not decode as an image is sent as a plain attachment rather
 * than being rejected — the mime type came from the operating system, and being
 * wrong about it is not a reason to lose the file.
 */
async function measure(attachment: PendingAttachment): Promise<OutgoingAttachment> {
  const base: OutgoingAttachment = {
    name: attachment.name,
    data: attachment.bytes,
    isImage: false,
    width: 0,
    height: 0,
    blurHash: '',
    // Set for a file too large to embed, and empty otherwise. The engine sends
    // by path when it is there and by value when it is not, so this is the
    // whole of what the client has to decide.
    path: attachment.path ?? '',
  };

  // Nothing to measure: a file staged by path was never read, and anything
  // without a preview is not an image.
  if (!attachment.previewUrl) return base;

  try {
    const image = await new Promise<HTMLImageElement>((resolve, reject) => {
      const probe = new Image();
      probe.onload = () => resolve(probe);
      probe.onerror = () => reject(new Error('not an image'));
      probe.src = attachment.previewUrl as string;
    });

    // The hash travels with the message, so the recipient sees a blur of the
    // picture the moment it lands rather than a grey box for the length of
    // the transfer. A failure here costs the placeholder, not the send.
    return {
      ...base,
      isImage: true,
      width: image.naturalWidth,
      height: image.naturalHeight,
      blurHash: blurHashFromImage(image) ?? '',
    };
  } catch {
    return base;
  }
}
