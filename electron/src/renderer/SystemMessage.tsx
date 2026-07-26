/**
 * System rows in the timeline: the group was renamed, somebody was invited,
 * somebody left.
 *
 * The sentences are the ones the Fyne client writes in `ui/thread_item.go`,
 * word for word. Two people watching the same conversation from the two
 * clients should read the same history, and a paraphrase here would quietly
 * make the Go build and this one disagree about what happened.
 */

import * as React from 'react';

import { messageTimestamp } from './format';
import './system-message.css';

/** A status change, flattened for display. */
export type SystemMessageView = {
  id: string;
  /** The conversation the change belongs to. */
  thread: string;
  /** The user who made the change; the sentence opens with their name. */
  actor: string;
  /** One of the kinds the core emits, listed in `SYSTEM_MESSAGE_KINDS`. */
  kind: string;
  /**
   * The user the change is about — the invitee, the promoted admin, the
   * removed member. Resolved through the name table when it is a user id, and
   * used verbatim otherwise, because an invitee is not always somebody we
   * already know by id.
   *
   * For `userRenamed` it carries the name the actor went by before the change,
   * since by the time the row renders the name table already holds the new one.
   */
  subject?: string;
  /** Free text the sentence needs: a new group name, a retention label. */
  value?: string;
  /** Unix seconds, as everywhere else in the timeline. */
  timestamp: number;
};

/**
 * Display names by user id.
 *
 * The caller resolves identity, and maps the local profile to `LOCAL_USER`, so
 * nothing here has to know which profile is ours.
 */
export type DisplayNames = Readonly<Record<string, string | undefined>>;

/** How the local user is named in a sentence, matching the Fyne client. */
export const LOCAL_USER = 'You';

/** Stand-in for a user the name table has no entry for. */
const UNKNOWN_USER = 'Someone';

/**
 * The kinds the Rust core emits, for callers that want to enumerate them.
 *
 * `SystemMessageView.kind` stays a plain string: a core newer than the
 * interface may send a kind this build has never heard of, and the right
 * answer to that is a vague sentence rather than a type error or a crash.
 */
export const SYSTEM_MESSAGE_KINDS = [
  'groupCreated',
  'groupRenamed',
  'retentionChanged',
  'groupImageChanged',
  'userInvited',
  'userRemoved',
  'userLeft',
  'adminPromoted',
  'adminDemoted',
  'inviteRevoked',
  'inviteAccepted',
  'inviteRejected',
  'userManagementRestricted',
  'userManagementUnrestricted',
  'groupEditsRestricted',
  'groupEditsUnrestricted',
  'postingRestricted',
  'postingUnrestricted',
  'groupBlocked',
  'historyCleared',
  'userRenamed',
  'userImageChanged',
] as const;

/** Kinds whose sentence is the actor's name followed by a fixed phrase. */
const FIXED_PHRASES: Readonly<Record<string, string | undefined>> = {
  groupCreated: 'created the group',
  groupImageChanged: 'changed the group image',
  userLeft: 'left the group',
  inviteAccepted: 'accepted the invite',
  inviteRejected: 'rejected the invite',
  userManagementRestricted: 'restricted user management',
  userManagementUnrestricted: 'unrestricted user management',
  groupEditsRestricted: 'restricted group edits',
  groupEditsUnrestricted: 'unrestricted group edits',
  postingRestricted: 'restricted posting',
  postingUnrestricted: 'unrestricted posting',
  groupBlocked: 'blocked the group',
  historyCleared: 'cleared the chat history',
};

/** The display name for a user id. */
function nameOf(id: string, names: DisplayNames): string {
  return names[id] ?? UNKNOWN_USER;
}

/** The name of the user a sentence is about, however the core identified them. */
function subjectName(message: SystemMessageView, names: DisplayNames): string {
  const { subject } = message;
  if (!subject) return UNKNOWN_USER;
  return names[subject] ?? subject;
}

/** Whatever the core put in `value`, or an empty string if it sent nothing. */
function valueOf(message: SystemMessageView): string {
  return message.value ?? '';
}

/**
 * The part of the sentence after the actor's name, or null for a kind this
 * build does not recognise.
 */
function actionPhrase(message: SystemMessageView, names: DisplayNames): string | null {
  const fixed = FIXED_PHRASES[message.kind];
  if (fixed !== undefined) return fixed;

  switch (message.kind) {
    case 'groupRenamed':
      return `changed the group name to ${valueOf(message)}`;
    case 'retentionChanged':
      return `changed the message retention to ${valueOf(message)}`;
    case 'userInvited':
      return `invited ${subjectName(message, names)} to the group`;
    case 'userRemoved':
      // A member removing themselves is a departure, and the Fyne client says
      // so rather than reporting that they removed themselves.
      if (message.subject === message.actor) return 'left the group';
      return `removed ${subjectName(message, names)} from the group`;
    case 'adminPromoted':
      return `made ${subjectName(message, names)} an admin`;
    case 'adminDemoted':
      return `removed ${subjectName(message, names)} as an admin`;
    case 'inviteRevoked':
      return `removed the invite for ${subjectName(message, names)}`;
    default:
      return null;
  }
}

/**
 * The sentence shown for a status change.
 *
 * Two kinds are phrased around the actor instead of prefixed with their name,
 * because "Ada changed the group name" and "Ada changed their name" put the
 * actor in different grammatical roles.
 */
export function describeSystemMessage(
  message: SystemMessageView,
  names: DisplayNames,
): string {
  const actorName = nameOf(message.actor, names);
  const isLocal = actorName === LOCAL_USER;

  if (message.kind === 'userRenamed') {
    if (isLocal) return `${LOCAL_USER} changed your name to ${valueOf(message)}`;

    // The actor's entry in the name table is already the new name, so the
    // sentence opens with the name they had before the change whenever the
    // core supplied it.
    const previous = message.subject ?? actorName;
    return `${previous} changed their name to ${valueOf(message)}`;
  }

  if (message.kind === 'userImageChanged') {
    return isLocal
      ? `${LOCAL_USER} changed your profile image`
      : `${actorName} changed their profile image`;
  }

  const phrase = actionPhrase(message, names);
  if (phrase === null) return `${actorName} updated the conversation`;

  return `${actorName} ${phrase}`;
}

type SystemMessageRowProps = {
  message: SystemMessageView;
  names: DisplayNames;
};

/**
 * One status change in the timeline.
 *
 * It is centred and muted rather than shaped like a bubble, so a run of
 * membership changes reads as a margin note between the conversation's turns
 * instead of competing with them.
 */
export function SystemMessageRow({ message, names }: SystemMessageRowProps) {
  return (
    <div className="system-message">
      <span className="system-message__body">{describeSystemMessage(message, names)}</span>
      <time
        className="system-message__timestamp"
        dateTime={new Date(message.timestamp * 1000).toISOString()}
      >
        {messageTimestamp(message.timestamp)}
      </time>
    </div>
  );
}
