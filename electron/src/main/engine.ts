/**
 * Loads the Rust core and adapts it for the main process.
 *
 * The native module is the only thing that ever touches key material or the
 * database. It is loaded here, in the main process, and never exposed to the
 * renderer — the renderer's entire view of the engine is the narrow, explicitly
 * enumerated surface in the preload script.
 */

import { toNative, type NativeAttachment, type OutgoingAttachment } from './attachments';
export type { OutgoingAttachment } from './attachments';

import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { EventEmitter } from 'node:events';

/** Shape of the class exported by `bounce-node`. */
interface NativeNode {
  readonly address: string;
  readonly transport: string;
  readonly anonymous: boolean;
  hasProfile(): boolean;
  createProfile(name: string, deviceName: string): string;
  initialState(): string;
  subscribe(callback: (event: string) => void): void;
  createPairingCode(): string;
  requestToAddUser(code: string): Promise<void>;
  markAsRead(messageId: string, isGroup: boolean): Promise<void>;
  typingIn(thread: string, isGroup: boolean): Promise<void>;
  sendDirectMessage(recipient: string, text: string, replyTo?: string): Promise<string>;
  sendGroupMessage(groupId: string, text: string, replyTo?: string): Promise<string>;
  sendDirectMessageWithAttachments(
    recipient: string,
    text: string,
    attachments: NativeAttachment[],
    replyTo?: string,
  ): Promise<string>;
  sendGroupMessageWithAttachments(
    groupId: string,
    text: string,
    attachments: NativeAttachment[],
    replyTo?: string,
  ): Promise<string>;
  react(target: string, targetType: number, emoji: string): Promise<void>;
  removeReaction(target: string, targetType: number): Promise<void>;
  deleteForMe(target: string, targetType: number): void;
  deleteForEveryone(target: string, targetType: number): Promise<void>;
  mayDeleteForEveryone(target: string, targetType: number): boolean;
  fileData(fileId: string): Buffer | null;
  messageInfo(messageId: string): string | null;
  createGroup(name: string, invites: string[]): Promise<string>;
  inviteToGroup(groupId: string, userId: string): Promise<void>;
  respondToInvite(groupId: string, accept: boolean): Promise<void>;
  renameGroup(groupId: string, name: string): Promise<void>;
  leaveGroup(groupId: string): Promise<void>;
  saveDraft(thread: string, text: string): Promise<void>;
  connectToPeer(address: string): Promise<void>;
  reachFor(conversation: string): Promise<void>;
  createSyncCode(): string;
  requestToSync(code: string): Promise<void>;
  revokeDevice(deviceId: string): Promise<void>;
  setProfileImage(image: NativeAttachment): Promise<void>;
  setGroupImage(groupId: string, image: NativeAttachment): Promise<void>;
  setMutedUntil(conversation: string, until: number): Promise<void>;
  setUserBlocked(userId: string, blocked: boolean): Promise<void>;
  setOpenDm(userId: string, open: boolean): Promise<void>;
  setUserAlias(userId: string, alias: string): Promise<void>;
  setUserNotes(userId: string, notes: string): Promise<void>;
  setRetention(conversation: string, seconds: number): Promise<void>;
  clearHistory(conversation: string): Promise<void>;
  setReadReceipts(conversation: string, setting: boolean | null): Promise<void>;
  setTypingIndicators(conversation: string, setting: boolean | null): Promise<void>;
  setLastOpened(conversation: string): void;
  removeFromGroup(groupId: string, userId: string): Promise<void>;
  revokeInvite(groupId: string, userId: string): Promise<void>;
  setGroupAdmin(groupId: string, userId: string, admin: boolean): Promise<void>;
  deleteGroup(groupId: string): Promise<void>;
  blockGroup(groupId: string): Promise<void>;
  setGroupPermission(groupId: string, permission: 'posting' | 'edits' | 'userManagement', restricted: boolean): Promise<void>;
  updateProfileName(name: string): Promise<void>;
  settings(): string;
  setDefaultRetention(seconds: number): void;
  setDefaultReadReceipts(enabled: boolean): void;
  setDefaultTypingIndicators(enabled: boolean): void;
  setNewGroupRestrictPosting(restricted: boolean): void;
  setNewGroupRestrictEdits(restricted: boolean): void;
  setNewGroupRestrictUserManagement(restricted: boolean): void;
  setAutoJoinGroups(setting: number): void;
  devices(): string;
  renameDevice(deviceId: string, name: string): void;
  shutdown(): void;
}

interface NativeModule {
  BounceNode: {
    open(dataDirectory: string, useTor: boolean): NativeNode;
  };
}

/**
 * Find the compiled native module.
 *
 * `npm run build:native` stages the cargo artifact as `native/bounce.node` —
 * Node only treats a file as an addon if it carries that extension. A packaged
 * build ships the same file under `resources/native`.
 */
function resolveNativeModule(): string {
  const candidates = [
    join(process.resourcesPath ?? '', 'native', 'bounce.node'),
    join(__dirname, '..', '..', 'native', 'bounce.node'),
  ];

  for (const candidate of candidates) {
    if (candidate && existsSync(candidate)) {
      return candidate;
    }
  }

  throw new Error(
    'Could not find the Bounce native module (native/bounce.node). Build it with:\n' +
      '  cd rust && cargo build --release -p bounce-node\n' +
      '  cd electron && npm run build:native',
  );
}

/**
 * A running engine, with events surfaced as an EventEmitter.
 */
export class BounceEngine extends EventEmitter {
  private constructor(private readonly node: NativeNode) {
    super();
  }

  /**
   * Start the engine.
   *
   * `useTor` decides whether this instance gets metadata protection at all.
   * Turning it off is a development affordance; the interface surfaces which
   * mode is in force rather than letting it pass unnoticed.
   *
   */
  static open(dataDirectory: string, useTor: boolean): BounceEngine {
    // eslint-disable-next-line @typescript-eslint/no-var-requires
    const native = require(resolveNativeModule()) as NativeModule;
    const node = native.BounceNode.open(dataDirectory, useTor);

    const engine = new BounceEngine(node);

    node.subscribe((payload: string) => {
      try {
        engine.emit('event', JSON.parse(payload));
      } catch {
        // A malformed event is a bug in the bridge, not something the
        // interface can act on; drop it rather than crashing the process.
      }
    });

    return engine;
  }

  get address(): string {
    return this.node.address;
  }

  /** Which transport is in force: 'tor' or 'tcp'. */
  get transport(): string {
    return this.node.transport;
  }

  /** Whether the active transport protects metadata. */
  get anonymous(): boolean {
    return this.node.anonymous;
  }

  createPairingCode(): string {
    return this.node.createPairingCode();
  }

  requestToAddUser(code: string): Promise<void> {
    return this.node.requestToAddUser(code);
  }

  markAsRead(messageId: string, isGroup: boolean): Promise<void> {
    return this.node.markAsRead(messageId, isGroup);
  }

  typingIn(thread: string, isGroup: boolean): Promise<void> {
    return this.node.typingIn(thread, isGroup);
  }

  hasProfile(): boolean {
    return this.node.hasProfile();
  }

  createProfile(name: string, deviceName: string): string {
    return this.node.createProfile(name, deviceName);
  }

  initialState(): unknown {
    return JSON.parse(this.node.initialState());
  }

  async sendDirectMessage(
    recipient: string,
    text: string,
    replyTo?: string,
  ): Promise<unknown> {
    return JSON.parse(await this.node.sendDirectMessage(recipient, text, replyTo));
  }

  async sendGroupMessage(groupId: string, text: string, replyTo?: string): Promise<unknown> {
    return JSON.parse(await this.node.sendGroupMessage(groupId, text, replyTo));
  }

  /*
   * Reacting, replying, and withdrawing.
   *
   * `targetType` is the frame type of the message being acted on — 0 for a
   * direct message, 1 for a group message — matching `FrameType` in the core.
   * The renderer knows which thread it is in, so passing it is cheaper than a
   * lookup on the other side of the boundary.
   */
  react(target: string, targetType: number, emoji: string): Promise<void> {
    return this.node.react(target, targetType, emoji);
  }

  removeReaction(target: string, targetType: number): Promise<void> {
    return this.node.removeReaction(target, targetType);
  }

  deleteForMe(target: string, targetType: number): void {
    this.node.deleteForMe(target, targetType);
  }

  deleteForEveryone(target: string, targetType: number): Promise<void> {
    return this.node.deleteForEveryone(target, targetType);
  }

  mayDeleteForEveryone(target: string, targetType: number): boolean {
    return this.node.mayDeleteForEveryone(target, targetType);
  }

  async sendDirectMessageWithAttachments(
    recipient: string,
    text: string,
    attachments: OutgoingAttachment[],
    replyTo?: string,
  ): Promise<unknown> {
    return JSON.parse(
      await this.node.sendDirectMessageWithAttachments(
        recipient,
        text,
        attachments.map(toNative),
        replyTo,
      ),
    );
  }

  async sendGroupMessageWithAttachments(
    groupId: string,
    text: string,
    attachments: OutgoingAttachment[],
    replyTo?: string,
  ): Promise<unknown> {
    return JSON.parse(
      await this.node.sendGroupMessageWithAttachments(
        groupId,
        text,
        attachments.map(toNative),
        replyTo,
      ),
    );
  }

  /** An attachment's bytes, or null while chunks are still missing. */
  /**
   * Everything known about what happened to one message.
   *
   * The native side hands this over as JSON rather than an object, so the
   * shape is declared once — in Rust — instead of a fourth time here.
   */
  messageInfo(messageId: string): unknown {
    const json = this.node.messageInfo(messageId);
    return json === null ? null : JSON.parse(json);
  }

  fileData(fileId: string): Uint8Array | null {
    return this.node.fileData(fileId);
  }

  async createGroup(name: string, invites: string[]): Promise<unknown> {
    return JSON.parse(await this.node.createGroup(name, invites));
  }

  inviteToGroup(groupId: string, userId: string): Promise<void> {
    return this.node.inviteToGroup(groupId, userId);
  }

  respondToInvite(groupId: string, accept: boolean): Promise<void> {
    return this.node.respondToInvite(groupId, accept);
  }

  renameGroup(groupId: string, name: string): Promise<void> {
    return this.node.renameGroup(groupId, name);
  }

  leaveGroup(groupId: string): Promise<void> {
    return this.node.leaveGroup(groupId);
  }

  saveDraft(thread: string, text: string): Promise<void> {
    return this.node.saveDraft(thread, text);
  }

  connectToPeer(address: string): Promise<void> {
    return this.node.connectToPeer(address);
  }

  /** Dial a conversation's devices now, rather than at the next audit. */
  reachFor(conversation: string): Promise<void> {
    return this.node.reachFor(conversation);
  }

  /**
   * A code another device can use to join this profile.
   *
   * Not the same as `createPairingCode`, which invites a contact — that one
   * makes somebody a correspondent, this one hands over the private keys.
   */
  createSyncCode(): string {
    return this.node.createSyncCode();
  }

  requestToSync(code: string): Promise<void> {
    return this.node.requestToSync(code);
  }

  revokeDevice(deviceId: string): Promise<void> {
    return this.node.revokeDevice(deviceId);
  }

  setProfileImage(image: OutgoingAttachment): Promise<void> {
    return this.node.setProfileImage(toNative(image));
  }

  setGroupImage(groupId: string, image: OutgoingAttachment): Promise<void> {
    return this.node.setGroupImage(groupId, toNative(image));
  }


  setMutedUntil(conversation: string, until: number): Promise<void> {
    return this.node.setMutedUntil(conversation, until);
  }

  setUserBlocked(userId: string, blocked: boolean): Promise<void> {
    return this.node.setUserBlocked(userId, blocked);
  }

  /** Show or hide a direct conversation, on this device and its siblings. */
  setOpenDm(userId: string, open: boolean): Promise<void> {
    return this.node.setOpenDm(userId, open);
  }

  setUserAlias(userId: string, alias: string): Promise<void> {
    return this.node.setUserAlias(userId, alias);
  }

  setUserNotes(userId: string, notes: string): Promise<void> {
    return this.node.setUserNotes(userId, notes);
  }

  setRetention(conversation: string, seconds: number): Promise<void> {
    return this.node.setRetention(conversation, seconds);
  }

  clearHistory(conversation: string): Promise<void> {
    return this.node.clearHistory(conversation);
  }

  setReadReceipts(conversation: string, setting: boolean | null): Promise<void> {
    return this.node.setReadReceipts(conversation, setting);
  }

  setTypingIndicators(conversation: string, setting: boolean | null): Promise<void> {
    return this.node.setTypingIndicators(conversation, setting);
  }

  /** Stamp a conversation as opened. Local; nothing is sent. */
  setLastOpened(conversation: string): void {
    this.node.setLastOpened(conversation);
  }

  removeFromGroup(groupId: string, userId: string): Promise<void> {
    return this.node.removeFromGroup(groupId, userId);
  }

  revokeInvite(groupId: string, userId: string): Promise<void> {
    return this.node.revokeInvite(groupId, userId);
  }

  setGroupAdmin(groupId: string, userId: string, admin: boolean): Promise<void> {
    return this.node.setGroupAdmin(groupId, userId, admin);
  }

  deleteGroup(groupId: string): Promise<void> {
    return this.node.deleteGroup(groupId);
  }

  blockGroup(groupId: string): Promise<void> {
    return this.node.blockGroup(groupId);
  }

  setGroupPermission(groupId: string, permission: 'posting' | 'edits' | 'userManagement', restricted: boolean): Promise<void> {
    return this.node.setGroupPermission(groupId, permission, restricted);
  }

  updateProfileName(name: string): Promise<void> {
    return this.node.updateProfileName(name);
  }

  settings(): unknown {
    return JSON.parse(this.node.settings());
  }

  setDefaultRetention(seconds: number): void {
    this.node.setDefaultRetention(seconds);
  }

  setDefaultReadReceipts(enabled: boolean): void {
    this.node.setDefaultReadReceipts(enabled);
  }

  setDefaultTypingIndicators(enabled: boolean): void {
    this.node.setDefaultTypingIndicators(enabled);
  }

  /** The restriction a group created on this device is born with. */
  setDefaultGroupPermission(
    permission: 'posting' | 'edits' | 'userManagement',
    restricted: boolean,
  ): void {
    if (permission === 'posting') this.node.setNewGroupRestrictPosting(restricted);
    else if (permission === 'edits') this.node.setNewGroupRestrictEdits(restricted);
    else this.node.setNewGroupRestrictUserManagement(restricted);
  }

  setAutoJoinGroups(setting: number): void {
    this.node.setAutoJoinGroups(setting);
  }

  devices(): unknown {
    return JSON.parse(this.node.devices());
  }

  renameDevice(deviceId: string, name: string): void {
    this.node.renameDevice(deviceId, name);
  }

  /** Detach the event sink so the process can exit. */
  shutdown(): void {
    this.node.shutdown();
  }
}
