/**
 * The bridge between the renderer and the engine.
 *
 * This is the entire attack surface the renderer has. Every method is
 * enumerated by hand, takes plain data, and returns plain data — there is no
 * generic `invoke(channel, ...)` escape hatch, so a compromised renderer cannot
 * reach an IPC channel that was not deliberately exposed.
 */

import { contextBridge, ipcRenderer, webUtils } from 'electron';

/** A message as the interface renders it. */
export interface Message {
  id: string;
  thread: string;
  author: string;
  text: string;
  writtenAt: number;
  expiresAt: number;
  seen: boolean;
  undeliverable: boolean;
  deliveredTo: string[];
  readBy: string[];
  attachments: Attachment[];
  outgoing: boolean;
}

export interface Attachment {
  id: string;
  fileId: string;
  name: string;
  size: number;
  width: number | null;
  height: number | null;
  blurHash: string | null;
  progress: number;
}

export interface User {
  id: string;
  name: string;
  alias: string;
  images: string[];
  blocked: boolean;
  accepted: boolean;
  introductionTime: number;
  lastActivity: number;
  /** When this conversation was last displayed. Local; never sent. */
  lastOpened: number;
  mutedUntil: number;
  /** How long this conversation's messages are kept, in seconds; 0 forever. */
  retention: number;
  /** Messages written before this timestamp were cleared for everyone. */
  clearBefore: number;
  /**
   * Whether this contact has an open conversation.
   *
   * The contact list and the conversation list are two lists, and this is what
   * separates them: someone met through a group invitation is a contact
   * without a thread until one of you writes. Hiding a conversation clears it
   * and loses nothing.
   */
  openDm: boolean;
  /** Private notes, never sent to the contact — only to your own devices. */
  notes: string;
  /**
   * Per-conversation overrides of the profile-wide privacy defaults.
   *
   * Two fields rather than a nullable one, because that is the shape on the
   * wire: Go encodes `[overridden, value]`. `overridden` false means the
   * profile default applies and the `enabled` flag is not to be read.
   */
  readReceiptsOverridden: boolean;
  readReceiptsEnabled: boolean;
  typingIndicatorsOverridden: boolean;
  typingIndicatorsEnabled: boolean;
  online: boolean;
}

export interface Group {
  id: string;
  name: string;
  images: string[];
  members: string[];
  admins: string[];
  invites: string[];
  createdBy: string;
  createdAt: number;
  lastActivity: number;
  /** When this conversation was last displayed. Local; never sent. */
  lastOpened: number;
  mutedUntil: number;
  retention: number;
  restrictPosting: boolean;
  restrictGroupEdits: boolean;
  restrictUserManagement: boolean;
}

export interface Device {
  id: string;
  name: string;
  address: string;
  createdAt: number;
  lastSeen: number;
  local: boolean;
  online: boolean;
  revoked: boolean;
}

export interface Draft {
  thread: string;
  text: string;
}

/**
 * A status change in a conversation: a rename, an invitation, a departure.
 *
 * The engine sends a kind and the ids involved rather than a finished
 * sentence, so names stay current and "You" is this client's word. See
 * `SystemMessage.tsx` for the wording.
 */
export interface SystemMessage {
  id: string;
  thread: string;
  actor: string;
  kind: string;
  subject?: string;
  value?: string;
  timestamp: number;
}

/** The profile-wide settings a conversation falls back to. */
export interface Settings {
  defaultGroupRetention: number;
  /** Note the lowercase `m`: serde renames `default_dm_retention` this way. */
  defaultDmRetention: number;
  defaultReadReceipts: boolean;
  defaultTypingIndicators: boolean;
  newGroupRestrictPosting: boolean;
  newGroupRestrictGroupEdits: boolean;
  newGroupRestrictUserManagement: boolean;
  /** 0 joins only groups with no unknown users, 1 never joins, 2 always does. */
  autoJoinGroups: number;
  blockedGroups: string[];
}

/**
 * A file being sent.
 *
 * `data` crosses to the main process once and is never sent back; the renderer
 * asks for a file by id when it needs to display it.
 */
export interface OutgoingAttachment {
  name: string;
  data: Uint8Array;
  isImage: boolean;
  width: number;
  height: number;
  blurHash?: string;
  /**
   * Where the file is on disk, for one too large to hold in memory.
   *
   * Set this and leave `data` empty and the engine streams it instead of
   * embedding it: hashed a chunk at a time, left where it is, and read back by
   * seeking when a peer asks. Get it from {@link BounceBridge.pathForFile};
   * a file with no path on disk — a pasted screenshot — cannot use this.
   */
  path?: string;
}

export interface InitialState {
  profile: User | null;
  networkOnline: boolean;
  deviceRevoked: boolean;
  syncDevices: Device[];
  users: User[];
  groups: Group[];
  messages: Message[];
  systemMessages: SystemMessage[];
  drafts: Draft[];
}

/**
 * An engine event.
 *
 * Mirrors the `Event` enum in `libbounce`, which serialises with a `type`
 * discriminator. Writing it as a union rather than a bag of unknowns means the
 * reducer's switch is checked against the real payloads, and a variant renamed
 * on the Rust side fails to compile here rather than silently doing nothing.
 */
export type EngineEvent =
  | { type: 'ready'; state: InitialState }
  | { type: 'networkOnline' }
  | { type: 'networkOffline' }
  | { type: 'profileCreated'; user: User; device: Device }
  | { type: 'messageReceived'; message: Message }
  | { type: 'messageSent'; message: Message }
  | { type: 'systemMessage'; message: SystemMessage }
  | { type: 'messageDelivered'; messageId: string; userId: string }
  | { type: 'messageRead'; messageId: string; userId: string }
  | { type: 'messageSeen'; messageId: string }
  | { type: 'messageUndeliverable'; messageId: string }
  | { type: 'messageDeleted'; messageId: string }
  | { type: 'typingStarted'; userId: string; thread: string }
  | { type: 'typingStopped'; userId: string; thread: string }
  | { type: 'userAdded'; user: User }
  | { type: 'userUpdated'; user: User }
  | { type: 'userOnline'; userId: string }
  | { type: 'userOffline'; userId: string }
  | { type: 'groupUpdated'; group: Group }
  | { type: 'groupRemoved'; groupId: string; actor: string }
  | { type: 'deviceAdded'; device: Device }
  | { type: 'deviceUpdated'; device: Device }
  | { type: 'deviceOnline'; deviceId: string }
  | { type: 'deviceOffline'; deviceId: string }
  | { type: 'draftUpdated'; draft: Draft }
  | { type: 'syncStarted' }
  | { type: 'syncProgress'; fraction: number }
  | { type: 'syncComplete' }
  | { type: 'fileProgress'; fileId: string; fraction: number }
  | { type: 'fileComplete'; fileId: string }
  | { type: 'settingsUpdated'; settings: Settings }
  | { type: 'error'; message: string };

/** Which transport the engine is running on. */
export interface TransportInfo {
  name: 'tor' | 'tcp';
  /** False means no metadata protection at all. */
  anonymous: boolean;
}

const api = {
  /** This device's onion address. */
  address: (): Promise<string> => ipcRenderer.invoke('bounce:address'),

  /** Which transport is in force, and whether it protects metadata. */
  transport: (): Promise<TransportInfo> => ipcRenderer.invoke('bounce:transport'),

  /**
   * Produce a pairing code for someone to scan.
   *
   * There is no directory to search, so this is the whole of contact
   * discovery. Each code is single-use and expires after five minutes.
   */
  createPairingCode: (): Promise<string> => ipcRenderer.invoke('bounce:createPairingCode'),

  /** Act on a scanned pairing code. */
  requestToAddUser: (code: string): Promise<void> =>
    ipcRenderer.invoke('bounce:requestToAddUser', code),

  /** Mark a message read, and tell its author if settings allow. */
  markAsRead: (messageId: string, isGroup: boolean): Promise<void> =>
    ipcRenderer.invoke('bounce:markAsRead', messageId, isGroup),

  /** Report that the user is composing. Safe to call per keystroke. */
  typingIn: (thread: string, isGroup: boolean): Promise<void> =>
    ipcRenderer.invoke('bounce:typingIn', thread, isGroup),

  /** Whether a profile has been created on this device. */
  hasProfile: (): Promise<boolean> => ipcRenderer.invoke('bounce:hasProfile'),

  /** Everything needed to render, fetched once on start-up. */
  initialState: (): Promise<InitialState> => ipcRenderer.invoke('bounce:initialState'),

  createProfile: (name: string, deviceName: string): Promise<string> =>
    ipcRenderer.invoke('bounce:createProfile', name, deviceName),

  sendDirectMessage: (recipient: string, text: string): Promise<Message> =>
    ipcRenderer.invoke('bounce:sendDirectMessage', recipient, text),

  sendGroupMessage: (groupId: string, text: string): Promise<Message> =>
    ipcRenderer.invoke('bounce:sendGroupMessage', groupId, text),

  /**
   * Send a direct message with files attached.
   *
   * The bytes are structured-cloned to the main process and handed straight to
   * the engine, which stores them and starts offering the chunks.
   */
  sendDirectMessageWithAttachments: (
    recipient: string,
    text: string,
    attachments: OutgoingAttachment[],
  ): Promise<Message> =>
    ipcRenderer.invoke('bounce:sendDirectMessageWithAttachments', recipient, text, attachments),

  sendGroupMessageWithAttachments: (
    groupId: string,
    text: string,
    attachments: OutgoingAttachment[],
  ): Promise<Message> =>
    ipcRenderer.invoke('bounce:sendGroupMessageWithAttachments', groupId, text, attachments),

  /**
   * Where a chosen file lives on disk, or '' if it has no path.
   *
   * Electron removed `File.path` in version 32, and `webUtils` exists only in
   * the preload, so this is the one place that can answer. A file from the
   * clipboard — a pasted screenshot — is a blob with no path, and gets ''.
   */
  pathForFile: (file: File): string => {
    try {
      return webUtils.getPathForFile(file);
    } catch {
      return '';
    }
  },

  /** An attachment's bytes, or null while it is still downloading. */
  fileData: (fileId: string): Promise<Uint8Array | null> =>
    ipcRenderer.invoke('bounce:fileData', fileId),

  createGroup: (name: string, invites: string[]): Promise<Group> =>
    ipcRenderer.invoke('bounce:createGroup', name, invites),

  inviteToGroup: (groupId: string, userId: string): Promise<void> =>
    ipcRenderer.invoke('bounce:inviteToGroup', groupId, userId),

  respondToInvite: (groupId: string, accept: boolean): Promise<void> =>
    ipcRenderer.invoke('bounce:respondToInvite', groupId, accept),

  renameGroup: (groupId: string, name: string): Promise<void> =>
    ipcRenderer.invoke('bounce:renameGroup', groupId, name),

  leaveGroup: (groupId: string): Promise<void> =>
    ipcRenderer.invoke('bounce:leaveGroup', groupId),

  saveDraft: (thread: string, text: string): Promise<void> =>
    ipcRenderer.invoke('bounce:saveDraft', thread, text),

  connectToPeer: (address: string): Promise<void> =>
    ipcRenderer.invoke('bounce:connectToPeer', address),

  /**
   * Ask the engine to open connections to a conversation's devices now.
   *
   * The engine peers on its own every minute; this is what makes opening a
   * chat feel immediate rather than waiting for the next pass.
   */
  reachFor: (conversation: string): Promise<void> =>
    ipcRenderer.invoke('bounce:reachFor', conversation),

  /**
   * A code another device can use to join this profile.
   *
   * Distinct from `createPairingCode` on purpose: that one invites a contact,
   * this one hands over the profile's private keys. They look identical on
   * screen and are not interchangeable — the engine keeps their secrets in
   * separate tables so one cannot be redeemed for the other.
   */
  createSyncCode: (): Promise<string> => ipcRenderer.invoke('bounce:createSyncCode'),

  /** Join an existing profile using a code from one of its devices. */
  requestToSync: (code: string): Promise<void> =>
    ipcRenderer.invoke('bounce:requestToSync', code),

  /** Take a device out of this profile's device group. */
  revokeDevice: (deviceId: string): Promise<void> =>
    ipcRenderer.invoke('bounce:revokeDevice', deviceId),

  setProfileImage: (image: OutgoingAttachment): Promise<void> =>
    ipcRenderer.invoke('bounce:setProfileImage', image),

  setGroupImage: (groupId: string, image: OutgoingAttachment): Promise<void> =>
    ipcRenderer.invoke('bounce:setGroupImage', groupId, image),

  setMutedUntil: (conversation: string, until: number): Promise<void> =>
    ipcRenderer.invoke('bounce:setMutedUntil', conversation, until),

  setUserBlocked: (userId: string, blocked: boolean): Promise<void> =>
    ipcRenderer.invoke('bounce:setUserBlocked', userId, blocked),

  /**
   * Show or hide a direct conversation.
   *
   * Sync-scoped, so hiding a conversation hides it on this profile's other
   * devices too. Nothing is deleted: the contact, the history and the device
   * group all stay, and setting this back to true restores the thread.
   */
  setOpenDm: (userId: string, open: boolean): Promise<void> =>
    ipcRenderer.invoke('bounce:setOpenDm', userId, open),

  setUserAlias: (userId: string, alias: string): Promise<void> =>
    ipcRenderer.invoke('bounce:setUserAlias', userId, alias),

  setUserNotes: (userId: string, notes: string): Promise<void> =>
    ipcRenderer.invoke('bounce:setUserNotes', userId, notes),

  setRetention: (conversation: string, seconds: number): Promise<void> =>
    ipcRenderer.invoke('bounce:setRetention', conversation, seconds),

  clearHistory: (conversation: string): Promise<void> =>
    ipcRenderer.invoke('bounce:clearHistory', conversation),

  setReadReceipts: (conversation: string, setting: boolean | null): Promise<void> =>
    ipcRenderer.invoke('bounce:setReadReceipts', conversation, setting),

  setTypingIndicators: (conversation: string, setting: boolean | null): Promise<void> =>
    ipcRenderer.invoke('bounce:setTypingIndicators', conversation, setting),

  /**
   * Stamp a conversation as opened, now.
   *
   * Local: the timestamp never leaves the device. It exists so a thread with
   * an unsent draft keeps its place near the top of the list instead of
   * sinking to the age of its last message.
   */
  setLastOpened: (conversation: string): Promise<void> =>
    ipcRenderer.invoke('bounce:setLastOpened', conversation),

  removeFromGroup: (groupId: string, userId: string): Promise<void> =>
    ipcRenderer.invoke('bounce:removeFromGroup', groupId, userId),

  revokeInvite: (groupId: string, userId: string): Promise<void> =>
    ipcRenderer.invoke('bounce:revokeInvite', groupId, userId),

  setGroupAdmin: (groupId: string, userId: string, admin: boolean): Promise<void> =>
    ipcRenderer.invoke('bounce:setGroupAdmin', groupId, userId, admin),

  deleteGroup: (groupId: string): Promise<void> =>
    ipcRenderer.invoke('bounce:deleteGroup', groupId),

  blockGroup: (groupId: string): Promise<void> =>
    ipcRenderer.invoke('bounce:blockGroup', groupId),

  setGroupPermission: (groupId: string, permission: 'posting' | 'edits' | 'userManagement', restricted: boolean): Promise<void> =>
    ipcRenderer.invoke('bounce:setGroupPermission', groupId, permission, restricted),

  updateProfileName: (name: string): Promise<void> =>
    ipcRenderer.invoke('bounce:updateProfileName', name),

  /** The profile-wide settings new conversations and groups inherit. */
  settings: (): Promise<Settings> => ipcRenderer.invoke('bounce:settings'),

  setDefaultRetention: (seconds: number): Promise<void> =>
    ipcRenderer.invoke('bounce:setDefaultRetention', seconds),

  setDefaultReadReceipts: (enabled: boolean): Promise<void> =>
    ipcRenderer.invoke('bounce:setDefaultReadReceipts', enabled),

  setDefaultTypingIndicators: (enabled: boolean): Promise<void> =>
    ipcRenderer.invoke('bounce:setDefaultTypingIndicators', enabled),

  /** The restriction a group created on this device is born with. */
  setDefaultGroupPermission: (
    permission: 'posting' | 'edits' | 'userManagement',
    restricted: boolean,
  ): Promise<void> =>
    ipcRenderer.invoke('bounce:setDefaultGroupPermission', permission, restricted),

  /** 0 joins only groups with no unknown users, 1 never joins, 2 always does. */
  setAutoJoinGroups: (setting: number): Promise<void> =>
    ipcRenderer.invoke('bounce:setAutoJoinGroups', setting),

  /** Every device in this profile's device group. */
  devices: (): Promise<Device[]> => ipcRenderer.invoke('bounce:devices'),

  /** Name one of this profile's devices. Local only; nothing is sent. */
  renameDevice: (deviceId: string, name: string): Promise<void> =>
    ipcRenderer.invoke('bounce:renameDevice', deviceId, name),


  /**
   * Subscribe to engine events. Returns an unsubscribe function.
   *
   * The raw Electron event object is deliberately not forwarded; the listener
   * receives only the payload.
   */
  onEvent: (listener: (event: EngineEvent) => void): (() => void) => {
    const wrapped = (_event: unknown, payload: EngineEvent) => listener(payload);
    ipcRenderer.on('bounce:event', wrapped);
    return () => ipcRenderer.removeListener('bounce:event', wrapped);
  },

  /** Subscribe to operating system theme changes. */
  onThemeChange: (listener: (dark: boolean) => void): (() => void) => {
    const wrapped = (_event: unknown, payload: { dark: boolean }) => listener(payload.dark);
    ipcRenderer.on('bounce:theme', wrapped);
    return () => ipcRenderer.removeListener('bounce:theme', wrapped);
  },

  platform: process.platform,
};

contextBridge.exposeInMainWorld('bounce', api);

export type BounceApi = typeof api;
