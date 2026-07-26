/**
 * A stub of the preload bridge, backed by fixtures.
 *
 * Used by `scripts/preview.mjs` to render the interface with representative
 * conversations so the layout can be inspected and captured without a live
 * engine or a second device. It exposes exactly the same surface as the real
 * preload script, so the renderer cannot tell the difference.
 */

const { contextBridge } = require('electron');

const HOUR = 3600;
const now = Math.floor(Date.now() / 1000);

const me = '00000000-0000-4000-8000-000000000001';
const ada = '00000000-0000-4000-8000-000000000002';
const grace = '00000000-0000-4000-8000-000000000003';
const alan = '00000000-0000-4000-8000-000000000004';
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
    ...overrides,
  };
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

    message('d1', grace, grace, 'Sending the compiler notes over shortly.', now - 5 * HOUR),
    message('d2', alan, alan, 'Thanks for the paper.', now - 30 * HOUR),
  ],
  drafts: [{ thread: grace, text: 'Sounds good, I will take a look at' }],
};

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
  typingIn: async () => undefined,
  hasProfile: async () => true,
  initialState: async () => state,
  createProfile: async () => me,
  sendDirectMessage: async () => state.messages[0],
  sendGroupMessage: async () => state.messages[0],
  createGroup: async () => state.groups[0],
  inviteToGroup: async () => undefined,
  respondToInvite: async () => undefined,
  renameGroup: async () => undefined,
  leaveGroup: async () => undefined,
  saveDraft: async () => undefined,
  connectToPeer: async () => undefined,
  onEvent: (listener) => {
    // Optionally inject a typing indicator so the rendered state can be
    // inspected without a second device.
    if (process.env.BOUNCE_PREVIEW_TYPING === '1') {
      setTimeout(
        () => listener({ type: 'typingStarted', userId: ada, thread: ada }),
        50,
      );
    }
    return () => undefined;
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
});
