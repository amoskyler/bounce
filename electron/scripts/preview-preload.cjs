/**
 * A stub of the preload bridge, backed by fixtures.
 *
 * Used by `scripts/preview.mjs` to render the interface with representative
 * conversations so the layout can be inspected and captured without a live
 * engine or a second device. It exposes exactly the same surface as the real
 * preload script, so the renderer cannot tell the difference.
 */

const { contextBridge, webUtils } = require('electron');
const { deflateSync } = require('node:zlib');

const HOUR = 3600;
const now = Math.floor(Date.now() / 1000);

const me = '00000000-0000-4000-8000-000000000001';
const ada = '00000000-0000-4000-8000-000000000002';
const grace = '00000000-0000-4000-8000-000000000003';
const alan = '00000000-0000-4000-8000-000000000004';
const katherine = '00000000-0000-4000-8000-000000000005';
const barbara = '00000000-0000-4000-8000-000000000006';
const edsger = '00000000-0000-4000-8000-000000000007';
const bookClub = '00000000-0000-4000-8000-000000000010';

function user(id, name, overrides = {}) {
  return {
    id,
    name,
    alias: '',
    images: [],
    blocked: false,
    accepted: true,
    introductionTime: now - 90 * 24 * HOUR,
    lastActivity: now,
    mutedUntil: 0,
    online: false,
    // Without this the sidebar filters every contact out — `conversations()`
    // treats the flag as the whole answer — and a preview of the left pane
    // shows two rows however many people the fixture defines.
    openDm: true,
    ...overrides,
  };
}

/**
 * A 400×300 PNG, built here rather than checked in.
 *
 * Image rows are the ones that break windowing arithmetic, so a preview that
 * cannot render one is a preview that cannot show the bug. Generating it costs
 * a few lines and keeps a binary blob out of the repository.
 */
let cachedPng = null;
function previewPng() {
  if (cachedPng) return cachedPng;

  const width = 400;
  const height = 300;

  const table = [];
  for (let index = 0; index < 256; index += 1) {
    let value = index;
    for (let bit = 0; bit < 8; bit += 1) value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
    table[index] = value >>> 0;
  }
  const crc = (buffer) => {
    let value = ~0;
    for (const byte of buffer) value = table[(value ^ byte) & 0xff] ^ (value >>> 8);
    return ~value >>> 0;
  };

  const chunk = (type, data) => {
    const length = Buffer.alloc(4);
    length.writeUInt32BE(data.length);
    const body = Buffer.concat([Buffer.from(type), data]);
    const check = Buffer.alloc(4);
    check.writeUInt32BE(crc(body));
    return Buffer.concat([length, body, check]);
  };

  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8; // bit depth
  header[9] = 2; // truecolour

  // A gradient, so the row is visibly an image rather than a flat block.
  const raw = Buffer.alloc(height * (1 + width * 3));
  for (let y = 0; y < height; y += 1) {
    const offset = y * (1 + width * 3);
    for (let x = 0; x < width; x += 1) {
      raw[offset + 1 + x * 3] = Math.floor((x * 255) / width);
      raw[offset + 2 + x * 3] = Math.floor((y * 255) / height);
      raw[offset + 3 + x * 3] = 140;
    }
  }

  cachedPng = new Uint8Array(
    Buffer.concat([
      Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
      chunk('IHDR', header),
      chunk('IDAT', deflateSync(raw)),
      chunk('IEND', Buffer.alloc(0)),
    ]),
  );
  return cachedPng;
}

function message(id, thread, author, text, writtenAt, overrides = {}) {
  return {
    id,
    thread,
    author,
    text,
    writtenAt,
    expiresAt: 0,
    seen: true,
    undeliverable: false,
    deliveredTo: [],
    readBy: [],
    attachments: [],
    outgoing: author === me,
    ...overrides,
  };
}

const state = {
  profile: user(me, 'Hayden Parker'),
  networkOnline: true,
  deviceRevoked: false,
  syncDevices: [
    {
      id: 'device-1',
      name: 'Mac',
      address: 'df7wwi7bnsctfrvlza4pvtk6u6e34ddwwkjagnadtp5iwpjwrvq5bpad',
      createdAt: now - 30 * 24 * HOUR,
      lastSeen: now,
      local: true,
      online: true,
      revoked: false,
    },
  ],
  users: [
    user(ada, 'Ada Lovelace', { online: true, lastActivity: now - 120 }),
    user(grace, 'Grace Hopper', { lastActivity: now - 5 * HOUR }),
    user(alan, 'Alan Turing', { lastActivity: now - 30 * HOUR }),
    // A muted thread, a name long enough to truncate, and somebody with no
    // history at all — the three row shapes the ordinary fixtures never reach.
    user(katherine, 'Katherine Johnson', {
      lastActivity: now - 8 * HOUR,
      mutedUntil: now + 30 * 24 * HOUR,
    }),
    user(barbara, 'Barbara Liskov (Substitution Principle)', {
      lastActivity: now - 3 * 24 * HOUR,
    }),
    user(edsger, 'Edsger Dijkstra', { lastActivity: now - 9 * 24 * HOUR }),
  ],
  groups: [
    {
      id: bookClub,
      name: 'Book Club',
      images: [],
      members: [me, ada, grace, alan],
      admins: [me],
      invites: [],
      createdBy: me,
      createdAt: now - 20 * 24 * HOUR,
      lastActivity: now - 40 * 60,
      mutedUntil: 0,
      retention: 0,
      restrictPosting: false,
      restrictGroupEdits: false,
      restrictUserManagement: true,
    },
  ],
  messages: [
    // A one-to-one conversation, with a grouped run and delivery states.
    message('m1', ada, ada, 'Did the analytical engine notes ever reach you?', now - 26 * HOUR),
    message('m2', ada, me, 'They did — reading them now.', now - 25 * HOUR, {
      deliveredTo: [ada],
      readBy: [ada],
    }),
    message('m3', ada, me, 'The note on Bernoulli numbers is remarkable.', now - 25 * HOUR + 30, {
      deliveredTo: [ada],
      readBy: [ada],
    }),
    message('m4', ada, ada, 'That one took the longest to write.', now - 24 * HOUR),
    message(
      'm5',
      ada,
      ada,
      'Let me know what you think of the second half when you get there.',
      now - 200,
      { seen: false },
    ),
    message('m6', ada, me, 'Will do — probably tonight.', now - 120, {
      deliveredTo: [ada],
    }),

    // A group thread, to show author labels and avatars.
    message('g1', bookClub, grace, 'Are we still meeting Tuesday?', now - 3 * HOUR),
    message('g2', bookClub, alan, 'Tuesday works for me.', now - 2.5 * HOUR),
    message('g3', bookClub, me, 'Tuesday it is. Same place, 7pm.', now - 2 * HOUR, {
      deliveredTo: [grace, alan],
      readBy: [grace],
    }),
    message('g4', bookClub, ada, 'I will bring the second volume.', now - 40 * 60),

    // Sidebar rows: an unread run, a delivered outgoing last message, and a
    // preview long enough to need the second line.
    message('k1', katherine, katherine, 'Numbers check out.', now - 8 * HOUR, { seen: false }),
    message('k2', katherine, katherine, 'Running it once more to be sure.', now - 8 * HOUR + 60, {
      seen: false,
    }),
    message(
      'b1',
      barbara,
      me,
      'The substitution rule only bites when the subtype narrows a precondition — which is exactly what happened here.',
      now - 3 * 24 * HOUR,
      { deliveredTo: [barbara] },
    ),

    // Every reachable delivery state at once, so one capture shows the ladder
    // rather than whichever rungs the ordinary fixtures happen to hit. `sent`
    // is missing because it is genuinely unreachable — it needs an encrypted
    // device to acknowledge — and faking a field for it would make the capture
    // a picture of the fixture rather than of the app.
    ...(process.env.BOUNCE_PREVIEW_TICKS === '1'
      ? [
          ['Sending — nobody has acknowledged it yet.', {}],
          ['Delivered — their own device has it.', { deliveredTo: [grace] }],
          ['Read — and they have looked at it.', { deliveredTo: [grace], readBy: [grace] }],
          ['Not delivered — given up on.', { undeliverable: true }],
        ].map(([text, overrides], index) =>
          message(`k${index}`, bookClub, me, text, now - 30 + index, overrides),
        )
      : []),

    // Disappearing messages at several points around the dial, so a capture
    // shows the whole sweep at once rather than one frame of it.
    ...(process.env.BOUNCE_PREVIEW_TIMERS === '1'
      ? [0, 1, 3, 6, 9, 11, 12].map((twelfths, index) =>
          message(
            `t${index}`,
            bookClub,
            index % 2 ? ada : me,
            `${twelfths}/12 left`,
            // Written one hour before it expires, so the dial's length is an
            // hour and the fraction remaining is exactly `twelfths`.
            now - 3600 + Math.round((3600 * twelfths) / 12),
            { expiresAt: now + Math.round((3600 * twelfths) / 12) },
          ),
        )
      : []),

    message('d1', grace, grace, 'Sending the compiler notes over shortly.', now - 5 * HOUR),
    message('d2', alan, alan, 'Thanks for the paper.', now - 30 * HOUR),
  ],
  drafts: [{ thread: grace, text: 'Sounds good, I will take a look at' }],
};

/*
 * A long thread of wildly uneven rows, for exercising the windowing.
 *
 * `BOUNCE_PREVIEW_BULK=400` prepends that many messages to the group thread.
 * The mix is the point: one-liners, paragraphs several times taller, and image
 * rows taller again. A list whose rows were all the same height would never
 * show a windowing bug, because the estimate would always be right.
 */
const bulk = Number(process.env.BOUNCE_PREVIEW_BULK || '0');
if (bulk > 0) {
  const authors = [grace, alan, ada, me];
  const extra = [];

  for (let index = 0; index < bulk; index += 1) {
    const author = authors[index % authors.length];
    const at = now - 3 * HOUR - (bulk - index) * 60;

    if (index % 11 === 5) {
      // An image row: several hundred pixels where the estimate says sixty.
      extra.push(
        message(`bulk${index}`, bookClub, author, '', at, {
          attachments: [
            {
              id: `att${index}`,
              fileId: `file${index}`,
              name: 'photo.png',
              size: 240_000,
              width: 1200,
              height: 900,
              blurHash: 'LEHV6nWB2yk8pyo0adR*.7kCMdnj',
              progress: 1,
            },
          ],
        }),
      );
    } else if (index % 7 === 3) {
      extra.push(
        message(
          `bulk${index}`,
          bookClub,
          author,
          `A longer thought, number ${index}. `.repeat(14),
          at,
        ),
      );
    } else {
      // A block of unread messages a long way up the thread, which is what
      // puts the timeline into its "reveal the first unread row" mode.
      const unread = process.env.BOUNCE_PREVIEW_UNREAD === '1' && index >= 40 && index < 60;
      extra.push(
        message(`bulk${index}`, bookClub, author, `Message ${index}.`, at, {
          seen: !unread,
          outgoing: unread ? false : author === me,
          author: unread ? grace : author,
        }),
      );
    }
  }

  state.messages = [...extra, ...state.messages];
}

const listeners = [];
const slowCalls = new Map();
let lastSend = null;

/** What an outgoing attachment looks like, without dumping the bytes. */
function describe(attachment) {
  return {
    name: attachment.name,
    dataLength: attachment.data ? attachment.data.length : null,
    dataType: attachment.data ? attachment.data.constructor.name : null,
    isImage: attachment.isImage,
    width: attachment.width,
    height: attachment.height,
    blurHashLength: (attachment.blurHash || '').length,
    path: attachment.path,
  };
}

const api = {
  address: async () => 'df7wwi7bnsctfrvlza4pvtk6u6e34ddwwkjagnadtp5iwpjwrvq5bpad',
  transport: async () => ({
    name: process.env.BOUNCE_PREVIEW_TRANSPORT || 'tor',
    anonymous: (process.env.BOUNCE_PREVIEW_TRANSPORT || 'tor') === 'tor',
  }),
  createPairingCode: async () =>
    'bounce:df7wwi7bnsctfrvlza4pvtk6u6e34ddwwkjagnadtp5iwpjwrvq5bpad:0f8a1c3d5e7b9a2c4d6e8f0a1b2c3d4e',
  requestToAddUser: async () => undefined,
  markAsRead: async () => undefined,
  /*
   * A 4:3 PNG, so an image row is drawn at its real height rather than
   * collapsing to a broken frame.
   *
   * `BOUNCE_PREVIEW_SLOW_FILES=n` withholds it for the first n calls per file,
   * which is what the engine does between reporting a file complete and having
   * the bytes ready to serve. A client that asks once and gives up shows a
   * blurhash for the rest of the session.
   */
  // Enough shape for the info panel to be worth looking at: one reader, one
  // recipient who has not read it, and one member of the group who has neither.
  messageInfo: async (messageId) => {
    const message = state.messages.find((candidate) => candidate.id === messageId);
    if (!message) return null;
    const group = state.groups.find((candidate) => candidate.id === message.thread);
    return {
      messageId,
      writtenAt: message.writtenAt,
      expiresAt: message.expiresAt || now + 6 * HOUR,
      readBy: [{ userId: grace, at: message.writtenAt + 240 }],
      deliveredTo: [
        { userId: grace, at: message.writtenAt + 12 },
        { userId: alan, at: message.writtenAt + 40 },
      ],
      audience: group ? group.members : [],
    };
  },

  fileData: async (fileId) => {
    const withhold = Number(process.env.BOUNCE_PREVIEW_SLOW_FILES || '0');
    if (withhold > 0) {
      const seen = (slowCalls.get(fileId) || 0) + 1;
      slowCalls.set(fileId, seen);
      if (seen <= withhold) return null;
    }
    return previewPng();
  },
  typingIn: async () => undefined,
  hasProfile: async () => true,
  initialState: async () => state,
  createProfile: async () => me,
  sendDirectMessage: async (_to, text) => {
    lastSend = { text, attachments: [] };
    return state.messages[0];
  },
  sendGroupMessage: async (_group, text) => {
    lastSend = { text, attachments: [] };
    return state.messages[0];
  },
  // Records the shape the composer hands over, which is the only way to see
  // what the engine would have been given.
  sendDirectMessageWithAttachments: async (_to, text, attachments) => {
    lastSend = { text, attachments: attachments.map(describe) };
    return state.messages[0];
  },
  sendGroupMessageWithAttachments: async (_group, text, attachments) => {
    lastSend = { text, attachments: attachments.map(describe) };
    return state.messages[0];
  },
  createGroup: async () => state.groups[0],
  inviteToGroup: async () => undefined,
  respondToInvite: async () => undefined,
  renameGroup: async () => undefined,
  leaveGroup: async () => undefined,
  saveDraft: async () => undefined,
  connectToPeer: async () => undefined,
  onEvent: (listener) => {
    // Held so the harness can push engine events into the renderer — a burst
    // of file progress, a message arriving — which is the only way to exercise
    // what the interface does while something is streaming in.
    listeners.push(listener);

    // Optionally inject a typing indicator so the rendered state can be
    // inspected without a second device.
    if (process.env.BOUNCE_PREVIEW_TYPING === '1') {
      setTimeout(
        () => listener({ type: 'typingStarted', userId: ada, thread: ada }),
        50,
      );
    }
    return () => {
      const at = listeners.indexOf(listener);
      if (at >= 0) listeners.splice(at, 1);
    };
  },
  // The real implementation, not a stub: resolving a File to a path is the
  // one thing in this flow that only the preload can do, so faking it would
  // skip the part most likely to be wrong.
  pathForFile: (file) => {
    try {
      return webUtils.getPathForFile(file);
    } catch (error) {
      return 'THREW: ' + error.message;
    }
  },
  onThemeChange: () => () => undefined,
  platform: process.platform,
};

contextBridge.exposeInMainWorld('bounce', api);

// The preview harness tells the renderer which conversation to open and which
// theme to use, through a channel the real bridge does not have.
contextBridge.exposeInMainWorld('bouncePreview', {
  select: process.env.BOUNCE_PREVIEW_SELECT || bookClub,
  theme: process.env.BOUNCE_PREVIEW_THEME || 'light',
  conversationIds: { me, ada, grace, alan, bookClub },
  /** What the composer last tried to send. */
  lastSend: () => lastSend,
  /** Push an engine event at the renderer, as the real bridge would. */
  emit: (event) => {
    for (const listener of listeners) listener(event);
  },
});
