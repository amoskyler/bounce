/**
 * The message info drawer: what actually happened to one message.
 *
 * Signal puts this behind a right-click, and it is the one place in a chat app
 * that has to be scrupulous rather than reassuring. "Sent" ticks are a summary;
 * this is the detail behind them, and a group is exactly where the summary
 * stops being enough — one tick can mean four people out of five.
 *
 * So the pending list is shown as prominently as the read list. A person who
 * has not received something is the useful fact; everyone else is the part you
 * already assumed.
 *
 * Delivery is recorded per device and reading per person. The engine folds
 * devices back into their owners, keeping the earliest time, because "delivered
 * to Ada at 14:02" is the honest summary of a frame that reached her laptop and
 * then her phone.
 */

import * as React from 'react';

import { Avatar } from './Avatar';
import { CloseIcon } from './icons';
import { messageTimestamp } from './format';
import type { MessageInfo as Info, Message, Receipt } from '../preload';
import type { State } from './state';

import './message-info.css';

/** A person and what is known about their copy of the message. */
type Row = { userId: string; at: number };

function displayName(state: State, userId: string): string {
  if (state.profile && userId === state.profile.id) return 'You';
  const user = state.users[userId];
  if (!user) return 'Someone you do not know';
  return user.alias || user.name || 'Unnamed';
}

/**
 * When something happened, in full.
 *
 * The timeline shows "2:55 PM" because the date separator above it already
 * said which day. Nothing here has that context, so every time carries its
 * date — a receipt from last Tuesday reading "2:55 PM" is worse than useless.
 */
function fullTime(at: number): string {
  if (!at) return 'time not recorded';
  return new Date(at * 1000).toLocaleString(undefined, {
    dateStyle: 'medium',
    timeStyle: 'short',
  });
}

function relativeExpiry(expiresAt: number, now: number): string {
  const seconds = expiresAt - now;
  if (seconds <= 0) return 'any moment now';

  const days = Math.floor(seconds / 86_400);
  if (days >= 1) return `in ${days} ${days === 1 ? 'day' : 'days'}`;

  const hours = Math.floor(seconds / 3_600);
  if (hours >= 1) return `in ${hours} ${hours === 1 ? 'hour' : 'hours'}`;

  const minutes = Math.max(1, Math.floor(seconds / 60));
  return `in ${minutes} ${minutes === 1 ? 'minute' : 'minutes'}`;
}

/**
 * Split an audience into read, delivered-but-unread, and neither.
 *
 * Exported because the three-way split is the whole substance of the panel and
 * the edge cases — a reader with no delivery record, an author counted among
 * their own recipients — are worth pinning down without a DOM.
 */
export function partition(
  info: Info,
  authorId: string,
): { read: Row[]; delivered: Row[]; pending: string[] } {
  const read: Row[] = info.readBy.map((receipt: Receipt) => ({
    userId: receipt.userId,
    at: receipt.at,
  }));
  const readIds = new Set(read.map((row) => row.userId));

  // A read receipt implies delivery, so somebody who read it never appears as
  // merely delivered — otherwise they would be counted twice.
  const delivered = info.deliveredTo
    .filter((receipt) => !readIds.has(receipt.userId))
    .map((receipt) => ({ userId: receipt.userId, at: receipt.at }));

  const accounted = new Set([...readIds, ...delivered.map((row) => row.userId), authorId]);

  // Only a group has an audience to be missing from. A direct message with no
  // delivery record shows nothing here rather than inventing a recipient.
  const pending = info.audience.filter((userId) => !accounted.has(userId));

  return { read, delivered, pending };
}

function PersonRow({
  state,
  userId,
  detail,
}: {
  state: State;
  userId: string;
  detail: string;
}) {
  const user = state.users[userId];

  return (
    <div className="message-info__person">
      <Avatar
        id={userId}
        name={displayName(state, userId)}
        images={user?.images ?? []}
        size={28}
      />
      <div className="message-info__person-text">
        <div className="message-info__person-name">{displayName(state, userId)}</div>
        <div className="message-info__person-detail">{detail}</div>
      </div>
    </div>
  );
}

function Section({
  title,
  count,
  children,
}: {
  title: string;
  count: number;
  children: React.ReactNode;
}) {
  if (count === 0) return null;

  return (
    <div className="message-info__section">
      <div className="message-info__heading">
        {title}
        <span className="message-info__count">{count}</span>
      </div>
      {children}
    </div>
  );
}

export function MessageInfoPanel({
  message,
  state,
  onClose,
}: {
  message: Message;
  state: State;
  onClose: () => void;
}) {
  const [info, setInfo] = React.useState<Info | null>(null);
  const [failed, setFailed] = React.useState(false);

  React.useEffect(() => {
    let cancelled = false;
    setInfo(null);
    setFailed(false);

    // Called through an optional chain: a renderer newer than the bridge it is
    // running against should show "no information", not take the window down
    // with a TypeError from inside an effect.
    void Promise.resolve(window.bounce.messageInfo?.(message.id) ?? null)
      .then((result) => {
        if (cancelled) return;
        if (result) setInfo(result);
        else setFailed(true);
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      });

    return () => {
      cancelled = true;
    };
  }, [message.id]);

  const now = Math.floor(Date.now() / 1000);
  const split = info ? partition(info, message.author) : null;

  return (
    <aside className="message-info" aria-label="Message information">
      <div className="message-info__header">
        <div className="message-info__title">Message info</div>
        <button className="icon-button" onClick={onClose} title="Close" aria-label="Close">
          <CloseIcon />
        </button>
      </div>

      <div className="message-info__body">
        {/* A quote of the message, so the panel is anchored to something the
            reader can recognise without looking back at the timeline. */}
        <div className="message-info__quote">
          {message.text || <span className="message-info__quote-empty">No text</span>}
        </div>

        <dl className="message-info__facts">
          <dt>Sent</dt>
          <dd>{fullTime(message.writtenAt)}</dd>

          <dt>From</dt>
          <dd>{message.outgoing ? 'You' : displayName(state, message.author)}</dd>

          {message.expiresAt > 0 && (
            <>
              <dt>Disappears</dt>
              <dd>
                {relativeExpiry(message.expiresAt, now)}
                <span className="message-info__muted"> · {fullTime(message.expiresAt)}</span>
              </dd>
            </>
          )}

          {message.undeliverable && (
            <>
              <dt>Status</dt>
              <dd className="message-info__warning">
                Could not be delivered to anyone
              </dd>
            </>
          )}
        </dl>

        {failed && (
          <div className="message-info__empty">
            This message is no longer in the database.
          </div>
        )}

        {!info && !failed && <div className="message-info__empty">Reading…</div>}

        {/*
          Reactions come from the message itself rather than from the info
          fetch: the engine already groups them onto every `MessageView`, so
          asking again would be a second source of the same truth — and a
          reaction arriving while the panel is open updates the message, which
          updates this, without another round trip.
        */}
        {message.reactions.length > 0 && (
          <Section
            title="Reactions"
            count={message.reactions.reduce((total, reaction) => total + reaction.users.length, 0)}
          >
            {message.reactions.flatMap((reaction) =>
              reaction.users.map((userId) => (
                <PersonRow
                  key={`${reaction.emoji}-${userId}`}
                  state={state}
                  userId={userId}
                  detail={reaction.emoji}
                />
              )),
            )}
          </Section>
        )}

        {split && (
          <>
            <Section title="Read by" count={split.read.length}>
              {split.read.map((row) => (
                <PersonRow
                  key={row.userId}
                  state={state}
                  userId={row.userId}
                  detail={fullTime(row.at)}
                />
              ))}
            </Section>

            <Section title="Delivered to" count={split.delivered.length}>
              {split.delivered.map((row) => (
                <PersonRow
                  key={row.userId}
                  state={state}
                  userId={row.userId}
                  detail={fullTime(row.at)}
                />
              ))}
            </Section>

            {/* Deliberately last and deliberately present: in a group this is
                the answer to the question that made you open the panel. */}
            <Section title="Not yet delivered" count={split.pending.length}>
              {split.pending.map((userId) => (
                <PersonRow
                  key={userId}
                  state={state}
                  userId={userId}
                  detail="Waiting for their device to come online"
                />
              ))}
            </Section>

            {split.read.length === 0 &&
              split.delivered.length === 0 &&
              split.pending.length === 0 && (
                <div className="message-info__empty">
                  {message.outgoing
                    ? 'Not delivered to anyone yet.'
                    : 'Nothing recorded for a message you received.'}
                </div>
              )}
          </>
        )}
      </div>
    </aside>
  );
}

/** Re-exported so the timeline can format a hover title the same way. */
export { fullTime as messageInfoTime, messageTimestamp };
