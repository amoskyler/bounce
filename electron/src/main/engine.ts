/**
 * Loads the Rust core and adapts it for the main process.
 *
 * The native module is the only thing that ever touches key material or the
 * database. It is loaded here, in the main process, and never exposed to the
 * renderer — the renderer's entire view of the engine is the narrow, explicitly
 * enumerated surface in the preload script.
 */

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
  sendDirectMessage(recipient: string, text: string): Promise<string>;
  sendGroupMessage(groupId: string, text: string): Promise<string>;
  sendDirectMessageWithAttachments(
    recipient: string,
    text: string,
    attachments: NativeAttachment[],
  ): Promise<string>;
  sendGroupMessageWithAttachments(
    groupId: string,
    text: string,
    attachments: NativeAttachment[],
  ): Promise<string>;
  fileData(fileId: string): Buffer | null;
  createGroup(name: string, invites: string[]): Promise<string>;
  inviteToGroup(groupId: string, userId: string): Promise<void>;
  respondToInvite(groupId: string, accept: boolean): Promise<void>;
  renameGroup(groupId: string, name: string): Promise<void>;
  leaveGroup(groupId: string): Promise<void>;
  saveDraft(thread: string, text: string): Promise<void>;
  connectToPeer(address: string): Promise<void>;
  reachFor(conversation: string): Promise<void>;
  setMutedUntil(conversation: string, until: number): Promise<void>;
  setUserBlocked(userId: string, blocked: boolean): Promise<void>;
  setUserAlias(userId: string, alias: string): Promise<void>;
  setUserNotes(userId: string, notes: string): Promise<void>;
  setRetention(conversation: string, seconds: number): Promise<void>;
  clearHistory(conversation: string): Promise<void>;
  setReadReceipts(conversation: string, setting: boolean | null): Promise<void>;
  setTypingIndicators(conversation: string, setting: boolean | null): Promise<void>;
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

/** A file crossing into the engine, as napi-rs expects it. */
interface NativeAttachment {
  name: string;
  data: Buffer;
  isImage: boolean;
  width: number;
  height: number;
  blurHash: string;
}

/** The same, as it arrives over IPC from the renderer. */
export interface OutgoingAttachment {
  name: string;
  data: Uint8Array;
  isImage: boolean;
  width: number;
  height: number;
  blurHash?: string;
}

/**
 * Adapt a renderer attachment for the native module.
 *
 * Structured cloning delivers the bytes as a `Uint8Array`, and napi-rs wants a
 * `Buffer`. `Buffer.from(view)` would copy; the three-argument form wraps the
 * same memory.
 */
function toNative(attachment: OutgoingAttachment): NativeAttachment {
  const bytes = attachment.data;
  return {
    name: attachment.name,
    data: Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength),
    isImage: attachment.isImage,
    width: attachment.width,
    height: attachment.height,
    blurHash: attachment.blurHash ?? '',
  };
}

interface NativeModule {
  BounceNode: {
    open(dataDirectory: string, useTor: boolean, goCompatible: boolean): NativeNode;
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
   * `goCompatible` makes outbound handshakes match the Go client. It is needed
   * to dial a Go peer, and it lets every address dialled obtain a signature
   * from this device. Inbound connections from Go peers work either way.
   */
  static open(
    dataDirectory: string,
    useTor: boolean,
    goCompatible: boolean,
  ): BounceEngine {
    // eslint-disable-next-line @typescript-eslint/no-var-requires
    const native = require(resolveNativeModule()) as NativeModule;
    const node = native.BounceNode.open(dataDirectory, useTor, goCompatible);

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

  async sendDirectMessage(recipient: string, text: string): Promise<unknown> {
    return JSON.parse(await this.node.sendDirectMessage(recipient, text));
  }

  async sendGroupMessage(groupId: string, text: string): Promise<unknown> {
    return JSON.parse(await this.node.sendGroupMessage(groupId, text));
  }

  async sendDirectMessageWithAttachments(
    recipient: string,
    text: string,
    attachments: OutgoingAttachment[],
  ): Promise<unknown> {
    return JSON.parse(
      await this.node.sendDirectMessageWithAttachments(recipient, text, attachments.map(toNative)),
    );
  }

  async sendGroupMessageWithAttachments(
    groupId: string,
    text: string,
    attachments: OutgoingAttachment[],
  ): Promise<unknown> {
    return JSON.parse(
      await this.node.sendGroupMessageWithAttachments(groupId, text, attachments.map(toNative)),
    );
  }

  /** An attachment's bytes, or null while chunks are still missing. */
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


  setMutedUntil(conversation: string, until: number): Promise<void> {
    return this.node.setMutedUntil(conversation, until);
  }

  setUserBlocked(userId: string, blocked: boolean): Promise<void> {
    return this.node.setUserBlocked(userId, blocked);
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
