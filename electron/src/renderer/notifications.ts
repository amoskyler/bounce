/**
 * Desktop notifications, and the rules that decide whether to raise one.
 *
 * The decision is a port of the Fyne client's `shouldNotifyDM` and
 * `shouldNotifyGM` (`chat/direct_message.go`, `chat/group_message.go`), which
 * consult the same three things: who wrote the message, whether the
 * conversation is muted, and — through `chat/notification_context.go` — whether
 * the reader is already watching that thread arrive. Keeping the predicate a
 * pure function of that context, rather than something that reads state as it
 * fires, is what makes the rules checkable.
 *
 * Nothing here reaches for the engine. A notification is a fact about the
 * window, not about the protocol.
 */

import * as React from 'react';

import { snippet } from './format';

/**
 * The `mutedUntil` value meaning "muted with no end date".
 *
 * Mirrors `libbounce::MUTED_FOREVER`. It is negative rather than large, so a
 * plain comparison against the clock is not enough to detect it.
 */
export const MUTED_FOREVER = -1;

/** Everything a notification needs to be raised and to be clicked on. */
export type NotifyRequest = {
  title: string;
  body: string;
  /**
   * The thread the notification belongs to. Used as the platform tag, so a
   * burst from one conversation collapses into one entry rather than a stack,
   * and so it can be withdrawn when the conversation is opened.
   */
  conversationId: string;
  /** Run when the notification is clicked, before the window is raised. */
  onActivate?: () => void;
};

/**
 * The live notifications we raised, newest per conversation.
 *
 * The platform replaces same-tag notifications for us, so one entry per
 * conversation is all there is to remember; the map exists so opening a
 * conversation can withdraw its notification the way the Fyne client's
 * `clearNotification` does on a read receipt.
 */
const live = new Map<string, Notification>();

/**
 * Raise a desktop notification, returning it, or null if the platform refused.
 *
 * Suppression is deliberately not applied here: call {@link shouldNotify}
 * first. Splitting them keeps the rules testable without a Notification
 * implementation, and keeps this function free to be used for anything else
 * worth interrupting someone over.
 */
export function notify({
  title,
  body,
  conversationId,
  onActivate,
}: NotifyRequest): Notification | null {
  // The preview harness and any test renderer have no Notification API, and a
  // missing one must not take the event handler down with it.
  if (typeof Notification === 'undefined') return null;
  if (Notification.permission === 'denied') return null;

  // Electron grants its own pages notification permission, so 'default' means
  // we are running somewhere that wants to be asked. Asking is worthwhile even
  // though this first notification will be dropped: the next one will show.
  if (Notification.permission === 'default') {
    void Notification.requestPermission().catch(() => {
      // A platform that cannot answer is one where notifications do not work;
      // there is nothing to recover and nothing worth reporting.
    });
  }

  let notification: Notification;
  try {
    notification = new Notification(title, { body, tag: conversationId });
  } catch {
    // Some platforms throw rather than fail quietly (Safari without a service
    // worker, a headless renderer). Losing a notification is not worth losing
    // the message that triggered it.
    return null;
  }

  notification.onclick = () => {
    onActivate?.();
    // Raising the window from the renderer works for the focused-app case but
    // cannot restore a minimised one; that needs the main process.
    window.focus();
    notification.close();
  };

  const forget = () => {
    if (live.get(conversationId) === notification) live.delete(conversationId);
  };
  notification.onclose = forget;
  notification.onerror = forget;

  live.set(conversationId, notification);
  return notification;
}

/**
 * Withdraw any notification still showing for a conversation.
 *
 * Called when the conversation is opened: the reader has plainly seen the
 * message, so leaving the banner up is noise.
 */
export function dismissNotifications(conversationId: string): void {
  const notification = live.get(conversationId);
  if (!notification) return;
  live.delete(conversationId);
  notification.close();
}

/** Everything the suppression rules look at. */
export type NotificationContext = {
  /** The thread the message arrived in. */
  conversationId: string;
  /** True for a message this device sent. */
  outgoing: boolean;
  /** The conversation's `mutedUntil`, in Unix seconds. */
  mutedUntil: number;
  /** Whether the application window currently has focus. */
  windowFocused: boolean;
  /** The conversation on screen, or null when none is selected. */
  activeConversation: string | null;
  /** Whether that conversation's timeline is pinned to its newest message. */
  atBottom: boolean;
  /**
   * Whether a catch-up sync is in flight.
   *
   * The Fyne client suppresses while `waitingForInitialSyncFrom` is set, for
   * the obvious reason: a device that has been offline for a day would
   * otherwise raise a banner for every message it missed, all at once.
   */
  syncing?: boolean;
  /** Unix seconds; injectable so mute expiry can be tested. */
  now?: number;
};

/** Whether a mute is still in force at the given instant. */
export function isMuted(mutedUntil: number, now: number): boolean {
  if (mutedUntil === MUTED_FOREVER) return true;
  return mutedUntil > now;
}

/**
 * Whether an arriving message should interrupt the reader.
 *
 * The last rule is the one that matters most in practice: a message that lands
 * in the conversation you are looking at, at the bottom where you will watch it
 * appear, has already notified you. Scrolled back through history it has not,
 * because the timeline will not move.
 */
export function shouldNotify(context: NotificationContext): boolean {
  // Our own messages arrive here too, echoed from our other devices.
  if (context.outgoing) return false;
  if (context.syncing) return false;

  const now = context.now ?? Math.floor(Date.now() / 1000);
  if (isMuted(context.mutedUntil, now)) return false;

  const watching =
    context.windowFocused &&
    context.activeConversation === context.conversationId &&
    context.atBottom;

  return !watching;
}

/**
 * The title and body for an arriving message.
 *
 * Follows the Fyne client's `getDMNotificationContent` and
 * `getGMNotificationContent`: a direct message is titled with its author, a
 * group message with the group and prefixed with its author, so the two read
 * the same way on screen. The preview comes from [`snippet`], which is what the
 * conversation list shows, so an attachment reads identically in both places.
 */
export function messageNotification(
  message: { text: string; attachments: readonly unknown[] },
  context: { authorName: string; conversationName: string; isGroup: boolean },
): { title: string; body: string } {
  const preview = snippet(message.text, message.attachments.length) || 'New message';

  if (context.isGroup) {
    return { title: context.conversationName, body: `${context.authorName}: ${preview}` };
  }

  return { title: context.authorName, body: preview };
}

/**
 * Whether each thread's timeline is pinned to its newest message.
 *
 * This is the port of `chat/notification_context.go`'s `scrolledDown` map, and
 * exists for the same reason: the fact lives in the scroll position of one
 * view, and the code that needs it is nowhere near that view. A thread we have
 * never heard about counts as not pinned, which is the safe answer — it errs
 * towards notifying.
 */
const atBottomByThread = new Map<string, boolean>();

/** Record whether a thread's timeline is showing its newest message. */
export function setTimelineAtBottom(threadId: string, atBottom: boolean): void {
  atBottomByThread.set(threadId, atBottom);
}

/** Whether a thread's timeline was last seen pinned to its newest message. */
export function timelineIsAtBottom(threadId: string): boolean {
  return atBottomByThread.get(threadId) ?? false;
}

/**
 * Track whether the application window has focus.
 *
 * `document.hasFocus()` answers for the current instant but never announces a
 * change, and the focus and blur events announce changes but say nothing about
 * where we started — so both are needed. The initial read is repeated inside
 * the effect because focus can be lost between render and mount.
 */
export function useWindowFocus(): boolean {
  const [focused, setFocused] = React.useState(
    () => typeof document !== 'undefined' && document.hasFocus(),
  );

  React.useEffect(() => {
    const onFocus = () => setFocused(true);
    const onBlur = () => setFocused(false);

    window.addEventListener('focus', onFocus);
    window.addEventListener('blur', onBlur);
    setFocused(document.hasFocus());

    return () => {
      window.removeEventListener('focus', onFocus);
      window.removeEventListener('blur', onBlur);
    };
  }, []);

  return focused;
}
