/**
 * Slash commands in the composer.
 *
 * Deliberately built as a table rather than one special case for `/shrug`. The
 * useful ones are all the same shape — type a name, get text — and a registry
 * means the next one is a line of data rather than a branch in the composer.
 *
 * The rules mirror the emoji typeahead next door, for the obvious reason that
 * a person typing into this box should not have to remember which kind of
 * completion they started. A slash has to open a word, one character is enough
 * to suggest on, and the match is against the name and its aliases.
 *
 * Where it differs is what happens on the way out. A shortcode is *part* of a
 * message — `:tada:` sits inside a sentence — so completing it replaces those
 * characters and leaves the rest alone. A command is not part of a message; it
 * is an instruction about the whole of it. `/shrug hello` sends "hello ¯\\_(ツ)_/¯",
 * so running one rewrites the entire draft, and it can only run when the slash
 * is the very first thing typed.
 */

/** What a command does to the draft it was invoked on. */
export interface SlashCommand {
  /** The name typed after the slash, lower case. */
  name: string;
  /** Other names that reach the same command. */
  aliases?: readonly string[];
  /** Shown beside the name in the suggestion list. */
  description: string;
  /**
   * Rewrite the draft.
   *
   * `rest` is whatever followed the command, already trimmed. Returning the
   * finished message text is the whole contract — a command cannot send, close
   * the window, or reach the engine, which keeps this table something that can
   * be read and trusted at a glance.
   */
  run: (rest: string) => string;
}

/** The shrug, as Slack spells it. The backslash is escaped in source only. */
export const SHRUG = '¯\\_(ツ)_/¯';

export const SLASH_COMMANDS: readonly SlashCommand[] = [
  {
    name: 'shrug',
    description: `Append ${SHRUG}`,
    // Appended rather than prepended, and joined with a space only when there
    // is something to join to, so a bare `/shrug` is not sent with a leading
    // space that shows up as an odd indent in the bubble.
    run: (rest) => (rest ? `${rest} ${SHRUG}` : SHRUG),
  },
  {
    name: 'tableflip',
    description: 'Append (╯°□°)╯︵ ┻━┻',
    run: (rest) => (rest ? `${rest} (╯°□°)╯︵ ┻━┻` : '(╯°□°)╯︵ ┻━┻'),
  },
  {
    name: 'unflip',
    description: 'Append ┬─┬ ノ( ゜-゜ノ)',
    run: (rest) => (rest ? `${rest} ┬─┬ ノ( ゜-゜ノ)` : '┬─┬ ノ( ゜-゜ノ)'),
  },
];

/** Characters allowed in a command name. */
const NAME_CHARACTERS = /^[a-z0-9-]*$/;

/** A `/name` being typed at the start of the draft. */
export interface SlashQuery {
  /** Always 0: a command only counts as one when it opens the message. */
  start: number;
  /** Index just past the last character of the name. */
  end: number;
  /** What has been typed after the slash, lower cased. */
  query: string;
}

/**
 * Find the command being typed, if the draft is one.
 *
 * Only at the very start. A slash mid-sentence is a slash — "and/or", a URL, a
 * date — and opening a command list over it would be both wrong and constant.
 */
export function findSlashQuery(text: string, caret: number): SlashQuery | null {
  if (!text.startsWith('/')) return null;

  // The name ends at the first space; past that the user is typing arguments
  // and the list has nothing left to offer.
  const end = text.indexOf(' ');
  if (end !== -1) return null;

  // The caret has to be in the name, not parked somewhere else in the draft.
  if (caret < 1 || caret > text.length) return null;

  const query = text.slice(1).toLowerCase();
  if (!NAME_CHARACTERS.test(query)) return null;

  return { start: 0, end: text.length, query };
}

/** Commands matching a partial name, best first. */
export function searchSlashCommands(query: string): SlashCommand[] {
  const needle = query.toLowerCase();

  const scored: { command: SlashCommand; score: number }[] = [];
  for (const command of SLASH_COMMANDS) {
    const names = [command.name, ...(command.aliases ?? [])];

    let best: number | null = null;
    for (const name of names) {
      if (name === needle) best = 0;
      else if (name.startsWith(needle)) best = Math.min(best ?? 2, 1);
      else if (name.includes(needle)) best = Math.min(best ?? 3, 2);
    }

    if (best !== null) scored.push({ command, score: best });
  }

  // Stable within a tier, so the table's own order is what breaks ties.
  scored.sort((a, b) => a.score - b.score);
  return scored.map((entry) => entry.command);
}

/** The command a name refers to exactly, or undefined. */
export function slashCommandNamed(name: string): SlashCommand | undefined {
  const needle = name.toLowerCase();
  return SLASH_COMMANDS.find(
    (command) => command.name === needle || command.aliases?.includes(needle),
  );
}

/**
 * Run a command against the draft it was typed into.
 *
 * Returns the new draft, or null when the text is not a command — an unknown
 * name is left alone rather than swallowed, so a message that happens to start
 * with a slash is still sendable.
 */
export function runSlashCommand(text: string): string | null {
  if (!text.startsWith('/')) return null;

  const space = text.indexOf(' ');
  const name = (space === -1 ? text.slice(1) : text.slice(1, space)).toLowerCase();
  const rest = space === -1 ? '' : text.slice(space + 1).trim();

  const command = slashCommandNamed(name);
  if (!command) return null;

  return command.run(rest);
}
