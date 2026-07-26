/**
 * Renderer state.
 *
 * Signal Desktop keeps its renderer state in Redux; this is the same idea in
 * one reducer. The engine is the source of truth — every action here is either
 * a snapshot it handed us or an event it emitted, so the reducer only ever
 * folds facts in, and never decides anything the engine has not already
 * decided.
 */

import type {
  Device,
  EngineEvent,
  Group,
  InitialState,
  Message,
  Settings,
  SystemMessage,
  User,
} from '../preload';

/** A conversation, whether one-to-one or a group. */
export type Conversation = {
  id: string;
  kind: 'direct' | 'group';
  name: string;
  /** Members, for groups. */
  memberCount: number;
  lastActivity: number;
  online: boolean;
  muted: boolean;
  /** Set for a group we hold an invitation to but have not joined. */
  invitationPending: boolean;
};

/**
 * Somebody this profile knows, whether or not there is a conversation open
 * with them.
 *
 * The contact store and the conversation list are two different things: Go
 * adds every user to `ui.users` but only opens a thread for the ones whose
 * `State.Open` is set (`ui/ui.go:459-465`). This is the first of the pair, and
 * the only route back to somebody the sidebar is not showing — there is no
 * directory to search and no way to re-find a person otherwise.
 */
export type Contact = {
  id: string;
  name: string;
  /** Image file ids, so the row can show a face rather than initials. */
  images: string[];
  online: boolean;
  blocked: boolean;
  /** Whether the sidebar is already showing a conversation with them. */
  open: boolean;
  /**
   * Whether closing the conversation would actually take a row off the
   * sidebar. False once there is history to keep it there; see
   * `conversations` for why history wins.
   */
  hideable: boolean;
};

export type State = {
  /** False until the initial snapshot arrives. */
  loaded: boolean;
  profile: User | null;
  address: string;
  networkOnline: boolean;
  /**
   * True until the network has reported either way. Go opens on "network is
   * starting..." and only says the connection was lost once it has been
   * (`ui/ui.go:358`, `:880-886`) — the two read very differently to somebody
   * who has just launched the app.
   */
  networkStarting: boolean;
  /** This device's authorisation was withdrawn from another one of yours. */
  deviceRevoked: boolean;
  syncing: boolean;
  syncProgress: number;

  users: Record<string, User>;
  groups: Record<string, Group>;
  devices: Device[];

  /** Messages by conversation, oldest first. */
  messagesByThread: Record<string, Message[]>;
  /** Status changes by conversation, oldest first. */
  systemMessagesByThread: Record<string, SystemMessage[]>;
  drafts: Record<string, string>;

  /** The profile-wide defaults, once the settings screen has asked for them. */
  settings: Settings | null;

  /** Users currently typing, by conversation. */
  typingByThread: Record<string, string[]>;

  selectedConversation: string | null;
  searchQuery: string;
  error: string | null;
};

export const initialState: State = {
  loaded: false,
  profile: null,
  address: '',
  networkOnline: false,
  networkStarting: true,
  deviceRevoked: false,
  syncing: false,
  syncProgress: 0,
  users: {},
  groups: {},
  devices: [],
  messagesByThread: {},
  systemMessagesByThread: {},
  drafts: {},
  settings: null,
  typingByThread: {},
  selectedConversation: null,
  searchQuery: '',
  error: null,
};

export type Action =
  | { type: 'loaded'; state: InitialState; address: string }
  | { type: 'engineEvent'; event: EngineEvent }
  | { type: 'selectConversation'; id: string | null }
  | { type: 'search'; query: string }
  | { type: 'setDraft'; thread: string; text: string }
  | { type: 'settingsLoaded'; settings: Settings }
  | { type: 'dismissError' };

/** Append a message to its thread, keeping the thread ordered and deduplicated. */
function withMessage(
  byThread: Record<string, Message[]>,
  message: Message,
): Record<string, Message[]> {
  const existing = byThread[message.thread] ?? [];

  // A message can arrive twice: once live and once through a catch up. The
  // engine deduplicates by ID in storage, and the interface must too.
  if (existing.some((candidate) => candidate.id === message.id)) {
    return byThread;
  }

  const updated = [...existing, message].sort((a, b) => {
    if (a.writtenAt !== b.writtenAt) return a.writtenAt - b.writtenAt;
    // Two messages written in the same second still need a stable order.
    return a.id < b.id ? -1 : 1;
  });

  return { ...byThread, [message.thread]: updated };
}

/** Add a status row to its thread, ordered and deduplicated like a message. */
function withSystemMessage(
  byThread: Record<string, SystemMessage[]>,
  row: SystemMessage,
): Record<string, SystemMessage[]> {
  const existing = byThread[row.thread] ?? [];

  // A status change is gossiped like everything else and can arrive twice.
  if (existing.some((candidate) => candidate.id === row.id)) {
    return byThread;
  }

  const updated = [...existing, row].sort(
    (a, b) => a.timestamp - b.timestamp || (a.id < b.id ? -1 : 1),
  );

  return { ...byThread, [row.thread]: updated };
}

/** Apply a change to one message wherever it lives. */
function mapMessage(
  byThread: Record<string, Message[]>,
  messageId: string,
  change: (message: Message) => Message,
): Record<string, Message[]> {
  let touched = false;
  const next: Record<string, Message[]> = {};

  for (const [thread, messages] of Object.entries(byThread)) {
    let threadTouched = false;
    const updated = messages.map((message) => {
      if (message.id !== messageId) return message;
      threadTouched = true;
      return change(message);
    });
    next[thread] = threadTouched ? updated : messages;
    touched ||= threadTouched;
  }

  return touched ? next : byThread;
}

function addUnique(list: string[], value: string): string[] {
  return list.includes(value) ? list : [...list, value];
}

export function reducer(state: State, action: Action): State {
  switch (action.type) {
    case 'loaded': {
      const users: Record<string, User> = {};
      for (const user of action.state.users) users[user.id] = user;

      const groups: Record<string, Group> = {};
      for (const group of action.state.groups) groups[group.id] = group;

      const messagesByThread: Record<string, Message[]> = {};
      for (const message of action.state.messages) {
        const bucket = messagesByThread[message.thread] ?? [];
        bucket.push(message);
        messagesByThread[message.thread] = bucket;
      }

      const systemMessagesByThread: Record<string, SystemMessage[]> = {};
      for (const row of action.state.systemMessages ?? []) {
        const bucket = systemMessagesByThread[row.thread] ?? [];
        bucket.push(row);
        systemMessagesByThread[row.thread] = bucket;
      }

      const drafts: Record<string, string> = {};
      for (const draft of action.state.drafts) drafts[draft.thread] = draft.text;

      return {
        ...state,
        loaded: true,
        profile: action.state.profile,
        address: action.address,
        networkOnline: action.state.networkOnline,
        // A snapshot that already says we are online ends the starting state
        // the same way an event would (`ui/ui.go:827-829` calls the same
        // `NetworkOnline`); a snapshot saying otherwise leaves us starting,
        // because nothing has been lost yet.
        networkStarting: state.networkStarting && !action.state.networkOnline,
        deviceRevoked: action.state.deviceRevoked,
        users,
        groups,
        devices: action.state.syncDevices,
        messagesByThread,
        systemMessagesByThread,
        drafts,
      };
    }

    case 'selectConversation':
      return { ...state, selectedConversation: action.id };

    case 'search':
      return { ...state, searchQuery: action.query };

    case 'setDraft':
      return {
        ...state,
        drafts: { ...state.drafts, [action.thread]: action.text },
      };

    case 'settingsLoaded':
      return { ...state, settings: action.settings };

    case 'dismissError':
      return { ...state, error: null };

    case 'engineEvent':
      return applyEvent(state, action.event);

    default:
      return state;
  }
}

function applyEvent(state: State, event: EngineEvent): State {
  switch (event.type) {
    case 'networkOnline':
      return { ...state, networkOnline: true, networkStarting: false };

    case 'networkOffline':
      return { ...state, networkOnline: false, networkStarting: false };

    case 'profileCreated':
      return { ...state, profile: event.user };

    case 'messageReceived':
    case 'messageSent': {
      const message = event.message;
      return { ...state, messagesByThread: withMessage(state.messagesByThread, message) };
    }

    case 'systemMessage':
      return {
        ...state,
        systemMessagesByThread: withSystemMessage(state.systemMessagesByThread, event.message),
      };

    case 'settingsUpdated':
      return { ...state, settings: event.settings };

    case 'messageDelivered': {
      const { messageId, userId } = event;
      return {
        ...state,
        messagesByThread: mapMessage(state.messagesByThread, messageId, (message) => ({
          ...message,
          deliveredTo: addUnique(message.deliveredTo, userId),
        })),
      };
    }

    case 'messageRead': {
      const { messageId, userId } = event;
      return {
        ...state,
        messagesByThread: mapMessage(state.messagesByThread, messageId, (message) => ({
          ...message,
          readBy: addUnique(message.readBy, userId),
        })),
      };
    }

    case 'messageSeen': {
      const { messageId } = event;
      return {
        ...state,
        messagesByThread: mapMessage(state.messagesByThread, messageId, (message) => ({
          ...message,
          seen: true,
        })),
      };
    }

    case 'messageUndeliverable': {
      const { messageId } = event;
      return {
        ...state,
        messagesByThread: mapMessage(state.messagesByThread, messageId, (message) => ({
          ...message,
          undeliverable: true,
        })),
      };
    }

    case 'messageDeleted': {
      const { messageId } = event;
      const next: Record<string, Message[]> = {};
      for (const [thread, messages] of Object.entries(state.messagesByThread)) {
        next[thread] = messages.filter((message) => message.id !== messageId);
      }
      return { ...state, messagesByThread: next };
    }

    case 'fileProgress':
    case 'fileComplete': {
      // Progress arrives by file id; the bubble that shows it is found by
      // scanning, because a message does not know it is being downloaded.
      const fileId = event.fileId;
      const progress = event.type === 'fileComplete' ? 1 : event.fraction;

      let touched = false;
      const next: Record<string, Message[]> = {};
      for (const [thread, messages] of Object.entries(state.messagesByThread)) {
        let threadTouched = false;
        const updated = messages.map((message) => {
          if (!message.attachments.some((a) => a.fileId === fileId)) return message;
          threadTouched = true;
          return {
            ...message,
            attachments: message.attachments.map((attachment) =>
              attachment.fileId === fileId ? { ...attachment, progress } : attachment,
            ),
          };
        });
        next[thread] = threadTouched ? updated : messages;
        touched ||= threadTouched;
      }

      return touched ? { ...state, messagesByThread: next } : state;
    }

    case 'typingStarted': {
      const { userId, thread } = event;
      return {
        ...state,
        typingByThread: {
          ...state.typingByThread,
          [thread]: addUnique(state.typingByThread[thread] ?? [], userId),
        },
      };
    }

    case 'typingStopped': {
      const { userId, thread } = event;
      return {
        ...state,
        typingByThread: {
          ...state.typingByThread,
          [thread]: (state.typingByThread[thread] ?? []).filter((id) => id !== userId),
        },
      };
    }

    case 'userAdded':
    case 'userUpdated': {
      const user = event.user;
      return { ...state, users: { ...state.users, [user.id]: user } };
    }

    case 'userOnline':
    case 'userOffline': {
      const { userId } = event;
      const user = state.users[userId];
      if (!user) return state;
      return {
        ...state,
        users: {
          ...state.users,
          [userId]: { ...user, online: event.type === 'userOnline' },
        },
      };
    }

    case 'groupUpdated': {
      const group = event.group;
      return { ...state, groups: { ...state.groups, [group.id]: group } };
    }

    case 'groupRemoved': {
      const { groupId } = event;
      const groups = { ...state.groups };
      delete groups[groupId];
      return {
        ...state,
        groups,
        selectedConversation:
          state.selectedConversation === groupId ? null : state.selectedConversation,
      };
    }

    case 'deviceAdded':
    case 'deviceUpdated': {
      const device = event.device;
      const others = state.devices.filter((candidate) => candidate.id !== device.id);
      return { ...state, devices: [...others, device] };
    }

    case 'deviceOnline':
    case 'deviceOffline': {
      const { deviceId } = event;
      return {
        ...state,
        devices: state.devices.map((device) =>
          device.id === deviceId ? { ...device, online: event.type === 'deviceOnline' } : device,
        ),
      };
    }

    case 'draftUpdated': {
      const draft = event.draft;
      return { ...state, drafts: { ...state.drafts, [draft.thread]: draft.text } };
    }

    case 'syncStarted':
      return { ...state, syncing: true, syncProgress: 0 };

    case 'syncProgress':
      return { ...state, syncProgress: event.fraction };

    case 'syncComplete':
      return { ...state, syncing: false, syncProgress: 1 };

    case 'error':
      return { ...state, error: event.message };

    default:
      return state;
  }
}

/**
 * The conversations that are open, ordered the way the list shows them.
 *
 * This is the sidebar, not the address book. Every user Go knows about goes
 * into `ui.users`, but a thread only exists for the ones whose `open_dm` is
 * set (`ui/ui.go:459-465`, `ui/direct_message.go:383`), and it is live
 * membership rather than a start-up filter — `SetDMState` adds and removes the
 * thread as the flag moves (`ui/direct_message.go:993-1017`). Somebody met
 * through a group is a contact and nothing more, so joining a twenty-person
 * group costs twenty contacts and no conversations.
 *
 * Recency comes from the last message rather than the stored activity
 * timestamp, so the ordering matches what is actually on screen.
 */
export function conversations(state: State): Conversation[] {
  const myId = state.profile?.id;

  // Ordering carries a key the conversation itself does not, so it is held
  // alongside for the sort and dropped again — see `sortActivityFor`.
  const result: Array<{ conversation: Conversation; sortActivity: number }> = [];

  for (const user of Object.values(state.users)) {
    // The note-to-self row below is this profile's own, and is built from the
    // profile rather than from whatever the user table happens to hold.
    if (user.id === myId) continue;

    // Go blocks and closes together (`chat/update_dm.go:382-383` sets
    // `open = !blocked`), so a blocked contact leaves the sidebar through the
    // flag rather than a filter of its own. Skipping explicitly means the
    // history rule below cannot drag them back in.
    if (user.blocked) continue;

    // History outranks the flag. `open_dm` had no producer for the whole of
    // this port's life, so a conversation somebody has genuinely been having
    // may well carry a false one; hiding it would be a worse failure than
    // showing a row Go would have left out.
    if (!user.openDm && !hasHistory(state, user.id)) continue;

    const lastActivity = lastActivityFor(state, user.id, user.lastActivity);
    result.push({
      conversation: {
        id: user.id,
        kind: 'direct',
        name: user.alias || user.name,
        memberCount: 0,
        lastActivity,
        online: user.online,
        muted: user.mutedUntil !== 0,
        invitationPending: false,
      },
      sortActivity: sortActivityFor(state, user.id, lastActivity, user.lastOpened),
    });
  }

  for (const group of Object.values(state.groups)) {
    const invited = myId !== undefined && !group.members.includes(myId) && group.invites.includes(myId);
    const lastActivity = lastActivityFor(state, group.id, group.lastActivity);
    result.push({
      conversation: {
        id: group.id,
        kind: 'group',
        name: group.name,
        memberCount: group.members.length,
        lastActivity,
        online: false,
        muted: group.mutedUntil !== 0,
        invitationPending: invited,
      },
      sortActivity: sortActivityFor(state, group.id, lastActivity, group.lastOpened),
    });
  }

  // A note-to-self conversation, always available.
  if (myId) {
    const lastActivity = lastActivityFor(state, myId, 0);
    result.push({
      conversation: {
        id: myId,
        kind: 'direct',
        name: `${state.profile?.name ?? 'You'} (You)`,
        memberCount: 0,
        lastActivity,
        online: false,
        muted: false,
        invitationPending: false,
      },
      sortActivity: sortActivityFor(state, myId, lastActivity, state.profile?.lastOpened ?? 0),
    });
  }

  return result
    .sort(
      (a, b) =>
        b.sortActivity - a.sortActivity ||
        a.conversation.name.localeCompare(b.conversation.name),
    )
    .map((entry) => entry.conversation);
}

/**
 * Everyone this profile knows, name-ordered — the other half of the pair.
 *
 * Blocked contacts are listed only on request, the way Go's contact browser
 * hides them behind a "Show Blocked" checkbox (`ui/new_dm_container.go:64-68`,
 * filtered at `:94-96`).
 */
export function contacts(state: State, includeBlocked = false): Contact[] {
  const myId = state.profile?.id;
  const result: Contact[] = [];

  for (const user of Object.values(state.users)) {
    if (user.id === myId) continue;
    if (user.blocked && !includeBlocked) continue;

    const history = hasHistory(state, user.id);
    result.push({
      id: user.id,
      name: user.alias || user.name,
      images: user.images,
      online: user.online,
      blocked: user.blocked,
      open: !user.blocked && (user.openDm || history),
      hideable: !user.blocked && user.openDm && !history,
    });
  }

  return result.sort((a, b) => a.name.localeCompare(b.name));
}

/** Filter contacts by a search box, on the same rule as conversations. */
export function filterContacts(list: Contact[], query: string): Contact[] {
  const trimmed = query.trim().toLowerCase();
  if (!trimmed) return list;
  return list.filter((contact) => contact.name.toLowerCase().includes(trimmed));
}

function hasHistory(state: State, thread: string): boolean {
  const messages = state.messagesByThread[thread];
  return messages !== undefined && messages.length > 0;
}

function lastActivityFor(state: State, thread: string, fallback: number): number {
  const messages = state.messagesByThread[thread];
  if (messages && messages.length > 0) {
    return messages[messages.length - 1].writtenAt;
  }
  return fallback;
}

/**
 * Where a conversation sits in the list, which is its newest message except
 * when it is holding an unsent draft.
 *
 * A draft pins the thread to the time it was last opened (`ui/thread.go:47-62`
 * against `hasDraft`, `ui/direct_message.go:121-123`), so half a reply typed
 * into a quiet conversation stays to hand instead of sinking back to wherever
 * the last message left it. Without it the row displays "Draft:" from the
 * bottom of the list, which is the worst of both.
 */
function sortActivityFor(
  state: State,
  thread: string,
  lastActivity: number,
  lastOpened: number,
): number {
  const draft = state.drafts[thread];
  if (draft !== undefined && draft.length > 0 && lastOpened > lastActivity) {
    return lastOpened;
  }
  return lastActivity;
}

/** Filter conversations by the search box. */
export function filterConversations(list: Conversation[], query: string): Conversation[] {
  const trimmed = query.trim().toLowerCase();
  if (!trimmed) return list;
  return list.filter((conversation) => conversation.name.toLowerCase().includes(trimmed));
}
