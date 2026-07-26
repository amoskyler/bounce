/**
 * The application shell: wiring, layout, and the dialogs that hang off it.
 */

import * as React from 'react';

import type { PendingAttachment } from './Attachments';
import { blurHashFromImage } from './blurhash';
import { ConversationView, NoConversationSelected } from './Conversation';
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
  conversations as deriveConversations,
  filterConversations,
  initialState,
  reducer,
} from './state';
import type { BounceApi, Message, TransportInfo } from '../preload';

declare global {
  interface Window {
    bounce: BounceApi;
  }
}

export function App() {
  const [state, dispatch] = React.useReducer(reducer, initialState);
  const [hasProfile, setHasProfile] = React.useState<boolean | null>(null);
  const [dialog, setDialog] = React.useState<'newGroup' | 'addContact' | null>(null);
  const [transport, setTransport] = React.useState<TransportInfo | null>(null);
  const [detailsOpen, setDetailsOpen] = React.useState(false);
  const [settingsOpen, setSettingsOpen] = React.useState(false);

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

  const handleSend = React.useCallback(
    async (text: string, attachments: readonly PendingAttachment[]) => {
      if (!selected) return;
      try {
        if (attachments.length > 0) {
          // Dimensions come from the preview the composer already decoded; the
          // engine does not decode images.
          const outgoing = await Promise.all(attachments.map(measure));
          if (selected.kind === 'group') {
            await window.bounce.sendGroupMessageWithAttachments(selected.id, text, outgoing);
          } else {
            await window.bounce.sendDirectMessageWithAttachments(selected.id, text, outgoing);
          }
        } else if (selected.kind === 'group') {
          await window.bounce.sendGroupMessage(selected.id, text);
        } else {
          await window.bounce.sendDirectMessage(selected.id, text);
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
    if (!selectedId || unreadIds === '') return;

    for (const id of unreadIds.split(',')) {
      if (reportedRead.current.has(id)) continue;
      reportedRead.current.add(id);
      void window.bounce.markAsRead(id, selectedIsGroup).catch(() => {
        // Allow a retry if it did not get through.
        reportedRead.current.delete(id);
      });
    }
  }, [selectedId, selectedIsGroup, unreadIds]);

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
      {transport && !transport.anonymous && (
        <div className="banner banner--insecure" role="alert">
          Running without Tor — this connection protects no metadata.
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
      <LeftPane
        state={state}
        conversations={visibleConversations}
        onSelect={(id) => {
          dispatch({ type: 'selectConversation', id });
          // Opening a conversation is a statement that the other side is
          // wanted; the engine's own peering pass is a minute away.
          void window.bounce.reachFor(id).catch(() => {
            // A dial that fails is retried by the peering audit.
          });
          setDetailsOpen(false);
          setSettingsOpen(false);
        }}
        onSearch={(query) => dispatch({ type: 'search', query })}
        onNewGroup={() => setDialog('newGroup')}
        onNewContact={() => setDialog('addContact')}
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
          onDraftChange={handleDraftChange}
          onAcceptInvite={() => void window.bounce.respondToInvite(selected.id, true)}
          onDeclineInvite={() => void window.bounce.respondToInvite(selected.id, false)}
          onLeaveGroup={() => {
            void window.bounce.leaveGroup(selected.id);
            dispatch({ type: 'selectConversation', id: null });
          }}
          onCopyAddress={() => void navigator.clipboard.writeText(state.address)}
          onShowDetails={() => setDetailsOpen(true)}
          onError={(message) => dispatch({ type: 'engineEvent', event: { type: 'error', message } })}
        />
      ) : (
        <NoConversationSelected address={state.address} />
      )}

      {/* Both are 340px right-hand asides, so they are mutually exclusive. */}
      {selected && detailsOpen && !settingsOpen && (
        <DetailsPanel
          conversation={selected}
          state={state}
          onClose={() => setDetailsOpen(false)}
        />
      )}

      {settingsOpen && <SettingsPanel state={state} onClose={() => setSettingsOpen(false)} />}

      {dialog === 'newGroup' && (
        <NewGroupDialog
          contactNames={allConversations
            .filter((conversation) => conversation.kind === 'direct')
            .map((conversation) => ({ id: conversation.id, name: conversation.name }))}
          onClose={() => setDialog(null)}
        />
      )}

      {dialog === 'addContact' && <AddContactDialog onClose={() => setDialog(null)} />}
    </div>
  );
}

function NewGroupDialog({
  contactNames,
  onClose,
}: {
  contactNames: Array<{ id: string; name: string }>;
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

        {contactNames.length > 0 && (
          <div style={{ marginTop: 12, maxHeight: 220, overflowY: 'auto' }}>
            {contactNames.map((contact) => (
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

        <div className="onboarding__address" style={{ marginBottom: 16 }}>
          {code || 'Generating…'}
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
async function measure(attachment: PendingAttachment): Promise<{
  name: string;
  data: Uint8Array;
  isImage: boolean;
  width: number;
  height: number;
  blurHash: string;
}> {
  const base = {
    name: attachment.name,
    data: attachment.bytes,
    isImage: false,
    width: 0,
    height: 0,
    blurHash: '',
  };

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
