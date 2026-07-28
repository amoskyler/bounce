/**
 * What the tick marks on an outgoing message mean.
 *
 * Signal draws four rungs of a ladder — sending, sent, delivered, read — and
 * the shapes are cumulative on purpose: a turning ring is nowhere yet, one
 * circled check is somewhere, two is the person, two filled is the person
 * having looked. Someone reads the shape before they read the tooltip, so the
 * rungs have to stay in that order even when one of them cannot happen yet.
 *
 * Bounce differs from Signal in what fills them. Signal's first rung is its
 * server accepting the message; there is no server here, so `sent` means a
 * device of the recipient's that is not their client is holding the bytes —
 * an encrypted storage device. That flow is unimplemented (parity P24), so
 * `sent` is unreachable today and a message steps straight from `sending` to
 * `delivered`. It is defined anyway, because the alternative is shipping a
 * three-rung ladder whose single check means "delivered" and then silently
 * redefining that check later.
 */

/** Just enough of a message to say where it is. */
export interface Deliverable {
  outgoing: boolean;
  undeliverable: boolean;
  /** Users with a device that has acknowledged the frame. */
  deliveredTo: readonly string[];
  /** Users who have sent a read receipt. */
  readBy: readonly string[];
}

export type DeliveryState = 'sending' | 'sent' | 'delivered' | 'read' | 'undeliverable';

/** The label the tick carries, as its tooltip and its accessible name. */
export const DELIVERY_LABELS: Record<DeliveryState, string> = {
  sending: 'Sending',
  sent: 'Sent',
  delivered: 'Delivered',
  read: 'Read',
  undeliverable: 'Not delivered',
};

/**
 * Where an outgoing message has got to.
 *
 * Read outranks delivered outranks sending, and each is derived from a
 * different kind of evidence: a read receipt the person's client chose to send,
 * an acknowledgement their device sent automatically, or neither. Nothing here
 * is inferred — an unacknowledged message is `sending` however long ago it was
 * written, which is what `undeliverable` eventually replaces.
 */
export function deliveryState(message: Deliverable): DeliveryState {
  if (message.undeliverable) return 'undeliverable';
  if (message.readBy.length > 0) return 'read';
  if (message.deliveredTo.length > 0) return 'delivered';
  return 'sending';
}

/**
 * Whether an outgoing message has a delivery state worth showing at all.
 *
 * Every rung of the ladder is evidence that somebody *else* has the message:
 * the engine drops the author from the list of who was reached, because a copy
 * landing on the sender's own second device is the same person twice. A note to
 * self has nobody else in it, so the honest answer is not `sending` — it is
 * that the question does not apply. Asking it anyway left a ring turning
 * forever under a message that had arrived the moment it was written.
 */
export function showsDeliveryState(
  message: { thread: string },
  myId: string | undefined,
): boolean {
  return myId === undefined || message.thread !== myId;
}
