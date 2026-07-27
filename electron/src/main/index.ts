/**
 * Electron main process.
 *
 * Owns the engine and the window. The renderer is fully sandboxed: no node
 * integration, context isolation on, and its only channel to the engine is the
 * enumerated set of IPC handlers registered here.
 */

import { app, BrowserWindow, ipcMain, nativeTheme, shell } from 'electron';
import { join } from 'node:path';

import { checkAttachments } from './attachments';
import { BounceEngine, type OutgoingAttachment } from './engine';

/*
 * Name the application before anything reads it.
 *
 * A packaged build takes its name from `productName` and is `Bounce.app`. An
 * unpackaged one runs inside Electron's own bundle, so the menu bar and the
 * about panel say "Electron" until told otherwise. The dock *title* in that
 * case still comes from the bundle and cannot be changed from here — only
 * packaging fixes that — but the icon can, and is, below.
 *
 * This must come before `app.getPath('userData')` is called, because the name
 * is part of that path. It already resolves to `Bounce`, since `productName`
 * is honoured for it, so setting the same name explicitly moves nothing.
 */
app.setName('Bounce');

// Running a second client on the same machine needs a separate profile
// directory, and separating it before anything else also makes the
// single-instance lock below distinguish the two. Development only.
const dataDirOverride = process.env.BOUNCE_DATA_DIR;
if (dataDirOverride) {
  app.setPath('userData', dataDirOverride);
}

/**
 * The icon for an unpackaged run.
 *
 * A packaged build takes its icon from the bundle, but `npm run dev` — which is
 * exactly when somebody sees the first-run screen — otherwise shows the default
 * Electron mark. `__dirname` is `dist/main` at runtime, so this resolves to
 * `electron/build/icon.png`.
 */
const DEVELOPMENT_ICON = join(__dirname, '..', '..', 'build', 'icon.png');

let mainWindow: BrowserWindow | null = null;
let engine: BounceEngine | null = null;

/**
 * Background colours used for the window itself, so that resizing and cold
 * start do not flash white before the renderer paints.
 */
const WINDOW_BACKGROUND = { light: '#ffffff', dark: '#1b1b1b' };

function createWindow(): void {
  mainWindow = new BrowserWindow({
    width: 1100,
    height: 760,
    minWidth: 640,
    minHeight: 480,
    backgroundColor: nativeTheme.shouldUseDarkColors
      ? WINDOW_BACKGROUND.dark
      : WINDOW_BACKGROUND.light,
    // A hidden title bar with inset traffic lights, matching Signal on macOS.
    titleBarStyle: process.platform === 'darwin' ? 'hiddenInset' : 'default',
    // Must match `--traffic-light-inset` in the renderer's styles.css: this
    // places the buttons, that reserves the space below them, and nothing
    // checks that the two agree.
    trafficLightPosition: { x: 10, y: 10 },
    // Windows and Linux take the task bar icon from the window.
    icon: app.isPackaged ? undefined : DEVELOPMENT_ICON,
    show: false,
    webPreferences: {
      preload: join(__dirname, '..', 'preload', 'index.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
    },
  });

  mainWindow.once('ready-to-show', () => mainWindow?.show());

  // Links open in the user's browser, never inside the app.
  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    // `shell.openExternal` launches whatever handler a scheme names, so a
    // `file:` or `ms-msdt:` url arriving here would be an arbitrary-launch
    // bug. The renderer already refuses to build such a link; this is the
    // second lock on the same door.
    let scheme: string;
    try {
      scheme = new URL(url).protocol;
    } catch {
      return { action: 'deny' };
    }
    if (scheme !== 'http:' && scheme !== 'https:') return { action: 'deny' };

    void shell.openExternal(url);
    return { action: 'deny' };
  });

  void mainWindow.loadFile(join(__dirname, '..', 'renderer', 'index.html'));
}

function startEngine(): void {
  // Tor is the point of the application, so it is the default. BOUNCE_NO_TOR
  // exists for local development, where waiting on a bootstrap for every
  // restart is not workable.
  const useTor = process.env.BOUNCE_NO_TOR !== '1';

  // Needed to dial a Go client, which expects the older handshake. See
  // libbounce::net for what accepting it costs.
  const goCompatible = process.env.BOUNCE_GO_COMPAT === '1';

  engine = BounceEngine.open(
    join(app.getPath('userData'), 'bounce'),
    useTor,
    goCompatible,
  );
  console.log(
    `Bounce is running on ${engine.transport} at ${engine.address}` +
      (engine.anonymous ? '' : ' — WITHOUT metadata protection'),
  );
  if (goCompatible) {
    console.warn(
      'BOUNCE_GO_COMPAT is on: outbound handshakes use the Go format, which ' +
        'lets any address you dial obtain a signature from this device.',
    );
  }

  engine.on('event', (event: unknown) => {
    mainWindow?.webContents.send('bounce:event', event);
  });
}

/**
 * Register an IPC handler that reports failures back to the renderer as a
 * rejected promise rather than crashing the main process.
 */
function handle(channel: string, handler: (...args: never[]) => unknown): void {
  ipcMain.handle(channel, async (_event, ...args) => {
    if (!engine) {
      throw new Error('the engine is not running');
    }
    return handler(...(args as never[]));
  });
}

function registerHandlers(): void {
  handle('bounce:address', () => engine!.address);
  handle('bounce:transport', () => ({
    name: engine!.transport,
    anonymous: engine!.anonymous,
  }));

  handle('bounce:createPairingCode', () => engine!.createPairingCode());
  handle('bounce:requestToAddUser', (code: string) => engine!.requestToAddUser(code));

  handle('bounce:markAsRead', (messageId: string, isGroup: boolean) =>
    engine!.markAsRead(messageId, isGroup),
  );
  handle('bounce:typingIn', (thread: string, isGroup: boolean) =>
    engine!.typingIn(thread, isGroup),
  );
  handle('bounce:hasProfile', () => engine!.hasProfile());
  handle('bounce:initialState', () => engine!.initialState());

  handle('bounce:createProfile', (name: string, deviceName: string) =>
    engine!.createProfile(name, deviceName),
  );

  handle('bounce:sendDirectMessage', (recipient: string, text: string, replyTo?: string) =>
    engine!.sendDirectMessage(recipient, text, replyTo),
  );
  handle('bounce:sendGroupMessage', (groupId: string, text: string, replyTo?: string) =>
    engine!.sendGroupMessage(groupId, text, replyTo),
  );

  handle(
    'bounce:sendDirectMessageWithAttachments',
    (
      recipient: string,
      text: string,
      attachments: OutgoingAttachment[],
      replyTo?: string,
    ) => {
      checkAttachments(attachments);
      return engine!.sendDirectMessageWithAttachments(recipient, text, attachments, replyTo);
    },
  );
  handle(
    'bounce:sendGroupMessageWithAttachments',
    (groupId: string, text: string, attachments: OutgoingAttachment[], replyTo?: string) => {
      checkAttachments(attachments);
      return engine!.sendGroupMessageWithAttachments(groupId, text, attachments, replyTo);
    },
  );

  handle('bounce:react', (target: string, targetType: number, emoji: string) =>
    engine!.react(target, targetType, emoji),
  );
  handle('bounce:removeReaction', (target: string, targetType: number) =>
    engine!.removeReaction(target, targetType),
  );
  handle('bounce:deleteForMe', (target: string, targetType: number) =>
    engine!.deleteForMe(target, targetType),
  );
  handle('bounce:deleteForEveryone', (target: string, targetType: number) =>
    engine!.deleteForEveryone(target, targetType),
  );
  handle('bounce:mayDeleteForEveryone', (target: string, targetType: number) =>
    engine!.mayDeleteForEveryone(target, targetType),
  );
  handle('bounce:fileData', (fileId: string) => engine!.fileData(fileId));
  handle('bounce:messageInfo', (messageId: string) => engine!.messageInfo(messageId));

  handle('bounce:createGroup', (name: string, invites: string[]) =>
    engine!.createGroup(name, invites),
  );
  handle('bounce:inviteToGroup', (groupId: string, userId: string) =>
    engine!.inviteToGroup(groupId, userId),
  );
  handle('bounce:respondToInvite', (groupId: string, accept: boolean) =>
    engine!.respondToInvite(groupId, accept),
  );
  handle('bounce:renameGroup', (groupId: string, name: string) =>
    engine!.renameGroup(groupId, name),
  );
  handle('bounce:leaveGroup', (groupId: string) => engine!.leaveGroup(groupId));

  handle('bounce:saveDraft', (thread: string, text: string) =>
    engine!.saveDraft(thread, text),
  );
  handle('bounce:setMutedUntil', (conversation: any, until: any) =>
    engine!.setMutedUntil(conversation, until),
  );
  handle('bounce:setUserBlocked', (userId: any, blocked: any) =>
    engine!.setUserBlocked(userId, blocked),
  );
  handle('bounce:setOpenDm', (userId: any, open: any) =>
    engine!.setOpenDm(userId, open),
  );
  handle('bounce:setUserAlias', (userId: any, alias: any) =>
    engine!.setUserAlias(userId, alias),
  );
  handle('bounce:setUserNotes', (userId: any, notes: any) =>
    engine!.setUserNotes(userId, notes),
  );
  handle('bounce:setRetention', (conversation: any, seconds: any) =>
    engine!.setRetention(conversation, seconds),
  );
  handle('bounce:clearHistory', (conversation: any) =>
    engine!.clearHistory(conversation),
  );
  handle('bounce:setReadReceipts', (conversation: any, setting: any) =>
    engine!.setReadReceipts(conversation, setting),
  );
  handle('bounce:setTypingIndicators', (conversation: any, setting: any) =>
    engine!.setTypingIndicators(conversation, setting),
  );
  handle('bounce:setLastOpened', (conversation: any) =>
    engine!.setLastOpened(conversation),
  );
  handle('bounce:removeFromGroup', (groupId: any, userId: any) =>
    engine!.removeFromGroup(groupId, userId),
  );
  handle('bounce:revokeInvite', (groupId: any, userId: any) =>
    engine!.revokeInvite(groupId, userId),
  );
  handle('bounce:setGroupAdmin', (groupId: any, userId: any, admin: any) =>
    engine!.setGroupAdmin(groupId, userId, admin),
  );
  handle('bounce:deleteGroup', (groupId: any) =>
    engine!.deleteGroup(groupId),
  );
  handle('bounce:blockGroup', (groupId: any) =>
    engine!.blockGroup(groupId),
  );
  handle('bounce:setGroupPermission', (groupId: any, permission: any, restricted: any) =>
    engine!.setGroupPermission(groupId, permission, restricted),
  );
  handle('bounce:updateProfileName', (name: any) =>
    engine!.updateProfileName(name),
  );
  handle('bounce:connectToPeer', (address: string) => engine!.connectToPeer(address));
  handle('bounce:reachFor', (conversation: string) => engine!.reachFor(conversation));

  handle('bounce:createSyncCode', () => engine!.createSyncCode());
  handle('bounce:requestToSync', (code: string) => engine!.requestToSync(code));
  handle('bounce:revokeDevice', (deviceId: string) => engine!.revokeDevice(deviceId));
  handle('bounce:setProfileImage', (image: OutgoingAttachment) =>
    engine!.setProfileImage(image),
  );
  handle('bounce:setGroupImage', (groupId: string, image: OutgoingAttachment) =>
    engine!.setGroupImage(groupId, image),
  );

  handle('bounce:settings', () => engine!.settings());
  handle('bounce:setDefaultRetention', (seconds: number) =>
    engine!.setDefaultRetention(seconds),
  );
  handle('bounce:setDefaultReadReceipts', (enabled: boolean) =>
    engine!.setDefaultReadReceipts(enabled),
  );
  handle('bounce:setDefaultTypingIndicators', (enabled: boolean) =>
    engine!.setDefaultTypingIndicators(enabled),
  );
  handle(
    'bounce:setDefaultGroupPermission',
    (permission: 'posting' | 'edits' | 'userManagement', restricted: boolean) =>
      engine!.setDefaultGroupPermission(permission, restricted),
  );
  handle('bounce:setAutoJoinGroups', (setting: number) =>
    engine!.setAutoJoinGroups(setting),
  );
  handle('bounce:devices', () => engine!.devices());
  handle('bounce:renameDevice', (deviceId: string, name: string) =>
    engine!.renameDevice(deviceId, name),
  );
}

// Only one instance may own the database and the device key.
if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on('second-instance', () => {
    if (mainWindow) {
      if (mainWindow.isMinimized()) mainWindow.restore();
      mainWindow.focus();
    }
  });

  void app.whenReady().then(() => {
    // macOS takes the dock icon from the bundle, which an unpackaged run does
    // not have.
    if (!app.isPackaged && process.platform === 'darwin') {
      app.dock?.setIcon(DEVELOPMENT_ICON);
    }

    registerHandlers();

    // The window comes up first. Bootstrapping Tor takes tens of seconds, and
    // doing it before the first paint leaves the user staring at nothing while
    // the OS reports the app as unresponsive.
    createWindow();

    // `setImmediate` yields to the event loop so the window actually paints
    // before the bootstrap takes the thread.
    setImmediate(() => {
      try {
        startEngine();
      } catch (error) {
        console.error('Failed to start the Bounce engine:', error);
        mainWindow?.webContents.send('bounce:event', {
          type: 'error',
          message: `The Bounce engine could not start: ${String(error)}`,
        });
      }
    });

    nativeTheme.on('updated', () => {
      mainWindow?.setBackgroundColor(
        nativeTheme.shouldUseDarkColors ? WINDOW_BACKGROUND.dark : WINDOW_BACKGROUND.light,
      );
      mainWindow?.webContents.send('bounce:theme', {
        dark: nativeTheme.shouldUseDarkColors,
      });
    });

    app.on('activate', () => {
      if (BrowserWindow.getAllWindows().length === 0) createWindow();
    });
  });

  app.on('window-all-closed', () => {
    if (process.platform !== 'darwin') app.quit();
  });

  // The engine's runtime holds tasks that never end on their own, so it has to
  // be told to let go before the process can exit.
  app.on('before-quit', () => {
    try {
      engine?.shutdown();
    } catch (error) {
      console.error('Error shutting down the Bounce engine:', error);
    }
  });
}
