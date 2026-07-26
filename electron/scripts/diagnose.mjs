/**
 * Collect everything needed to diagnose a failed transfer, into one file.
 *
 * ```bash
 * npm run diagnose                      # the default profile
 * npm run diagnose -- --dir /tmp/x      # a specific data directory
 * npm run diagnose -- --out report.txt
 * ```
 *
 * ## What it does and does not include
 *
 * Message text, draft text, aliases, notes and key material are never read.
 * What comes out is structure: schema version, which files exist, how many of
 * their chunks have bytes, who has offered what, which devices are known, and
 * the tail of the engine log.
 *
 * Onion addresses *are* included, truncated. They are public identifiers — a
 * device's address is its public key — but they are also the one thing that
 * links this report to a person, so they are shortened rather than printed in
 * full.
 */

import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';

const args = process.argv.slice(2);

function flag(name, fallback) {
  const index = args.indexOf(`--${name}`);
  return index !== -1 && args[index + 1] ? args[index + 1] : fallback;
}

/** Where Electron puts application data, unless it was overridden. */
function defaultDataDirectory() {
  if (process.env.BOUNCE_DATA_DIR) return join(process.env.BOUNCE_DATA_DIR, 'bounce');
  if (process.platform === 'darwin') {
    return join(homedir(), 'Library', 'Application Support', 'Bounce', 'bounce');
  }
  if (process.platform === 'win32') {
    return join(process.env.APPDATA ?? homedir(), 'Bounce', 'bounce');
  }
  return join(process.env.XDG_CONFIG_HOME ?? join(homedir(), '.config'), 'Bounce', 'bounce');
}

const directory = resolve(flag('dir', defaultDataDirectory()));
const output = resolve(flag('out', 'bounce-diagnostics.txt'));
const database = join(directory, 'bounce.db');
const logFile = join(directory, 'bounce.log');

const lines = [];
const say = (text = '') => lines.push(text);

/** Run a read-only query, returning its output or the error as text. */
function query(sql) {
  try {
    return execFileSync('sqlite3', ['-readonly', '-header', '-column', database, sql], {
      encoding: 'utf8',
      maxBuffer: 32 * 1024 * 1024,
    }).trimEnd();
  } catch (error) {
    return `(query failed: ${error.message.split('\n')[0]})`;
  }
}

function section(title, body) {
  say();
  say(`── ${title} ${'─'.repeat(Math.max(0, 66 - title.length))}`);
  say(body || '(nothing)');
}

say(`Bounce diagnostics — ${new Date().toISOString()}`);
say(`platform      ${process.platform} ${process.arch}`);
say(`data directory ${directory}`);

if (!existsSync(database)) {
  say();
  say(`No database at ${database}.`);
  say('Pass the right one with --dir, or check where the app is storing data.');
  writeFileSync(output, lines.join('\n'));
  console.log(`wrote ${output}`);
  process.exit(0);
}

say(`database      ${(statSync(database).size / 1024 / 1024).toFixed(1)} MB`);
say(`schema        version ${query('PRAGMA user_version;')}`);

section('Attachments: is the metadata here, and are the bytes?', query(`
  SELECT substr(hex(f.id), 1, 8)          AS file,
         substr(f.name, 1, 28)            AS name,
         f.size                           AS bytes,
         f.wanted, f.downloaded,
         (length(f.hash_list) + 1) / 65   AS want_chunks,
         (SELECT COUNT(*) FROM chunks c WHERE c.file_id = f.id)                        AS rows,
         (SELECT COUNT(*) FROM chunks c WHERE c.file_id = f.id AND c.data IS NOT NULL) AS with_bytes
  FROM files f
  ORDER BY f.saved_at DESC
  LIMIT 25;
`));

section('Chunks we are missing, and whether anyone has offered them', query(`
  SELECT substr(hex(c.file_id), 1, 8) AS file,
         c.idx,
         substr(c.hash, 1, 12)        AS hash,
         (SELECT COUNT(*) FROM chunk_locations l WHERE l.hash = c.hash) AS offers
  FROM chunks c
  WHERE c.data IS NULL
  ORDER BY c.file_id, c.idx
  LIMIT 40;
`));

section('Who has offered chunks', query(`
  SELECT substr(hash, 1, 12) AS hash,
         substr(address, 1, 16) || '…' AS device,
         offered_at
  FROM chunk_locations
  ORDER BY offered_at DESC
  LIMIT 25;
`));

section('Devices we know about', query(`
  SELECT substr(hex(d.user_id), 1, 8)     AS user,
         substr(d.address, 1, 16) || '…'  AS address,
         d.revoked_at, d.last_seen,
         (SELECT COUNT(*) FROM users u WHERE u.id = d.user_id AND u.profile = 1) AS is_me
  FROM devices d
  ORDER BY d.last_seen DESC
  LIMIT 25;
`));

section('Conversation activity, which is what peering dials on', query(`
  SELECT substr(hex(id), 1, 8) AS user, last_activity, blocked, accepted, open_dm
  FROM users WHERE profile = 0
  ORDER BY last_activity DESC LIMIT 20;
`));

section('Undelivered frames, by type', query(`
  SELECT 'direct_messages' AS kind, COUNT(*) AS total FROM direct_messages
  UNION ALL SELECT 'group_messages', COUNT(*) FROM group_messages
  UNION ALL SELECT 'files', COUNT(*) FROM files
  UNION ALL SELECT 'chunks', COUNT(*) FROM chunks
  UNION ALL SELECT 'chunks_with_bytes', COUNT(*) FROM chunks WHERE data IS NOT NULL
  UNION ALL SELECT 'delivery_records', COUNT(*) FROM delivery_records;
`));

if (existsSync(logFile)) {
  const raw = readFileSync(logFile, 'utf8');
  const all = raw.split('\n');
  // The transfer lines are the point; everything else is context around them.
  const relevant = all.filter((line) =>
    /file|chunk|broadcast|peer|dial|frame|too large|rejected/i.test(line),
  );
  section(
    `Engine log — last 400 transfer-related lines of ${all.length}`,
    relevant.slice(-400).join('\n'),
  );
} else {
  section(
    'Engine log',
    `No log at ${logFile}.\n\n` +
      'Restart the app with logging on, reproduce the failure, then run this again:\n' +
      '    BOUNCE_LOG=libbounce=debug npm run dev\n' +
      'or, for a packaged build:\n' +
      '    BOUNCE_LOG=libbounce=debug /Applications/Bounce.app/Contents/MacOS/Bounce',
  );
}

writeFileSync(output, lines.join('\n'));
console.log(`wrote ${output} (${(lines.join('\n').length / 1024).toFixed(0)} KB)`);
console.log('No message text, drafts, notes or key material are included.');
