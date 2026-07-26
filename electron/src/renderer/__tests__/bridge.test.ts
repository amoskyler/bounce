/**
 * The bridge is four layers of hand-written names — napi, the main-process
 * adapter, the IPC channel table, the preload surface — and only one of the
 * three seams is checked by the compiler. `tsc` will notice if `BounceEngine`
 * calls a `NativeNode` method that was never declared, and nothing at all
 * notices if a channel is invoked but never handled, or if a `#[napi]` method
 * is renamed in Rust. Both of those fail at runtime, in the one place with no
 * stack trace worth reading: a rejected `ipcRenderer.invoke`.
 *
 * So the seams are checked here, by reading the sources rather than by loading
 * them — `electron` cannot be imported outside an Electron process, and the
 * Rust side is not importable at all.
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

/**
 * Sources are read relative to the working directory rather than `__dirname`:
 * this file is bundled before it runs, so `__dirname` is wherever the bundle
 * landed, while `npm test` always runs from the package root.
 */
function source(...parts: string[]): string {
  const path = join(process.cwd(), ...parts);
  try {
    return readFileSync(path, 'utf8');
  } catch {
    throw new Error(`could not read ${path} — run the tests from electron/`);
  }
}

function matches(text: string, pattern: RegExp): string[] {
  return [...text.matchAll(pattern)].map((match) => match[1]!);
}

/** The body of a `interface Name {` / `}` block, braces balanced. */
function interfaceBody(text: string, name: string): string {
  const start = text.indexOf(`interface ${name} {`);
  assert.notEqual(start, -1, `no interface ${name}`);

  let depth = 0;
  for (let index = text.indexOf('{', start); index < text.length; index += 1) {
    if (text[index] === '{') depth += 1;
    else if (text[index] === '}') {
      depth -= 1;
      if (depth === 0) return text.slice(text.indexOf('{', start) + 1, index);
    }
  }
  throw new Error(`interface ${name} is not closed`);
}

/**
 * The member names of a TypeScript interface body.
 *
 * Members with parameters span several lines, and the parameter names look
 * exactly like members, so lines inside an open parenthesis are skipped.
 */
function memberNames(body: string): string[] {
  const names: string[] = [];
  let depth = 0;

  for (const line of body.split('\n')) {
    if (depth === 0) {
      const member = /^\s*(?:readonly\s+)?([A-Za-z_]\w*)\s*[(:]/.exec(line);
      if (member) names.push(member[1]!);
    }
    for (const character of line) {
      if (character === '(') depth += 1;
      else if (character === ')') depth -= 1;
    }
  }

  return names;
}

const SNAKE_SEGMENT = /_([a-z0-9])/g;

function camelCase(name: string): string {
  return name.replace(SNAKE_SEGMENT, (_all, character: string) => character.toUpperCase());
}

test('every channel the preload invokes has a handler in the main process', () => {
  // Both sides wrap a long call, so the channel is not always on the same
  // line as the function name.
  const invoked = new Set(
    matches(source('src', 'preload', 'index.ts'), /ipcRenderer\.invoke\(\s*'(bounce:[A-Za-z]+)'/g),
  );
  const handled = new Set(
    matches(source('src', 'main', 'index.ts'), /\bhandle\(\s*'(bounce:[A-Za-z]+)'/g),
  );

  // Not a smoke test: an empty set on either side would pass every assertion
  // below while telling us the regex stopped matching.
  assert.ok(invoked.size > 30, `only ${invoked.size} channels found in the preload`);

  const unhandled = [...invoked].filter((channel) => !handled.has(channel));
  assert.deepEqual(unhandled, [], 'preload invokes channels the main process does not handle');

  // The other direction matters too. A handler nothing can reach is either a
  // half-finished bridge method or IPC surface the renderer was never meant to
  // have, and both are worth being told about.
  const unreachable = [...handled].filter((channel) => !invoked.has(channel));
  assert.deepEqual(unreachable, [], 'main process handles channels the preload cannot invoke');
});

test('every NativeNode member is a #[napi] export on BounceNode', () => {
  const rust = readFileSync(
    join(process.cwd(), '..', 'rust', 'bounce-node', 'src', 'lib.rs'),
    'utf8',
  );

  // napi-rs camelCases the Rust name unless told otherwise, so the Rust
  // spelling is what the seam actually turns on.
  const exported = new Set(
    matches(rust, /#\[napi[^\]]*\]\s*pub (?:async )?fn ([a-z_0-9]+)/g).map(camelCase),
  );
  assert.ok(exported.size > 30, `only ${exported.size} #[napi] methods found`);

  const declared = memberNames(interfaceBody(source('src', 'main', 'engine.ts'), 'NativeNode'));
  const missing = declared.filter((member) => !exported.has(member));
  assert.deepEqual(missing, [], 'NativeNode declares methods bounce-node does not export');
});

test('setOpenDm reaches the engine through every layer', () => {
  // The one method the parity work adds to the bridge, spelled out rather than
  // left to the sweeps above: the renderer tracks are coding against this name
  // right now, and a typo in any layer is silent until someone hides a chat.
  assert.match(source('src', 'preload', 'index.ts'), /setOpenDm: \(userId: string, open: boolean\)/);
  assert.match(source('src', 'preload', 'index.ts'), /invoke\('bounce:setOpenDm', userId, open\)/);
  assert.match(source('src', 'main', 'index.ts'), /handle\('bounce:setOpenDm'/);
  assert.match(source('src', 'main', 'engine.ts'), /setOpenDm\(userId: string, open: boolean\)/);
  assert.match(
    readFileSync(join(process.cwd(), '..', 'rust', 'bounce-node', 'src', 'lib.rs'), 'utf8'),
    /pub async fn set_open_dm\(&self, user_id: String, open: bool\)/,
  );
});

test('the User view carries the per-conversation settings the store holds', () => {
  // These eight are settings the engine already persists and the renderer read
  // back as defaults, which is worse than not having them: a retention policy
  // that displays as "Off" is a claim about where plaintext went.
  const preload = source('src', 'preload', 'index.ts');
  const user = memberNames(interfaceBody(preload, 'User'));

  for (const field of [
    'retention',
    'clearBefore',
    'openDm',
    'notes',
    'readReceiptsOverridden',
    'readReceiptsEnabled',
    'typingIndicatorsOverridden',
    'typingIndicatorsEnabled',
  ]) {
    assert.ok(user.includes(field), `User is missing ${field}`);
  }

  // Sidebar ordering reads this on both kinds of conversation, so a group
  // missing it sorts as though it had never been opened.
  assert.ok(user.includes('lastOpened'), 'User is missing lastOpened');
  const group = memberNames(interfaceBody(preload, 'Group'));
  assert.ok(group.includes('lastOpened'), 'Group is missing lastOpened');
});
