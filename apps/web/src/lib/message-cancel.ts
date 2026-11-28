/**
 * Cancel-send presentation and guard logic (pure).
 *
 * Bridge contract (task-30): WS command `cancel_message` and authenticated REST
 * `POST /api/messages/{msg_id}/cancel`. The bridge replies with `cancel_result`,
 * broadcasts a `message_update` carrying `cancelled: true`, and persists the
 * `cancelled` flag; this module only decides when the bubble action is offered.
 */

import type { DeliveryState } from '../types/api';

/** Channel sends have no delivery ack; only offer Cancel briefly (or while media moves). */
export const GROUP_CANCEL_WINDOW_MS = 120_000;

export type CancelAvailabilityState = 'hidden' | 'available' | 'requesting' | 'unavailable';

export interface CancelAvailability {
  state: CancelAvailabilityState;
  /** Text for the bubble action (`Cancel`, `Cancelling…`) — empty when hidden. */
  label: string;
  /** Tooltip / aria explanation. */
  title: string;
  /** True only when the action should be clickable right now. */
  canCancel: boolean;
  /** The bridge explicitly does not support cancel (hide the action). */
  bridgeUnsupported: boolean;
}

export interface CancelAvailabilityInput {
  outgoing: boolean;
  msgId: number;
  delivery: DeliveryState;
  /** Local optimistics while the cancel command is in flight. */
  cancelRequested?: boolean;
  error?: string | null;
  /** Channel messages: no delivered/read ack, so use a short window + media progress. */
  group?: boolean;
  sentAtMs?: number;
  nowMs?: number;
  hasAttachment?: boolean;
  attachmentProgress?: number | null;
  /** `false` after an unsupported-bridge attempt; `null`/`undefined` while unknown. */
  bridgeSupportsCancel?: boolean | null;
}

/** True while an outgoing attachment has not finished its radio transfer. */
export function isMediaTransferInFlight(
  input: Pick<CancelAvailabilityInput, 'hasAttachment' | 'attachmentProgress'>,
): boolean {
  if (!input.hasAttachment) return false;
  const progress = input.attachmentProgress;
  return typeof progress !== 'number' || !Number.isFinite(progress) ? true : progress < 1;
}

const HIDDEN: CancelAvailability = {
  state: 'hidden',
  label: '',
  title: '',
  canCancel: false,
  bridgeUnsupported: false,
};

/**
 * Decide whether the own-message Cancel action is shown (bridge task-30 rule:
 * `outgoing && !delivered && !cancelled`, plus our id/error guards).
 *
 * Cancel is offered for queued sends, in-flight media and stalled media alike;
 * it disappears the moment the bridge reports delivery/read/cancelled/failure.
 */
export function cancelAvailability(input: CancelAvailabilityInput): CancelAvailability {
  if (!input.outgoing || input.msgId <= 0) return HIDDEN;
  if (
    input.delivery === 'delivered' ||
    input.delivery === 'read' ||
    input.delivery === 'failed' ||
    input.delivery === 'cancelled'
  ) {
    return HIDDEN;
  }
  if (input.error) return HIDDEN;
  if (input.bridgeSupportsCancel === false) {
    return { ...HIDDEN, state: 'unavailable', bridgeUnsupported: true };
  }

  // Channel broadcasts never get a delivered/read receipt; keep Cancel for in-flight
  // media and a short post-send window instead of showing it on every old channel post.
  if (input.group) {
    const nowMs = input.nowMs ?? Date.now();
    const sentAtMs = input.sentAtMs ?? nowMs;
    const recent = Math.max(0, nowMs - sentAtMs) <= GROUP_CANCEL_WINDOW_MS;
    if (!recent && !isMediaTransferInFlight(input) && input.delivery !== 'pending') return HIDDEN;
  }

  if (input.cancelRequested) {
    return {
      state: 'requesting',
      label: 'Cancelling…',
      title: 'Waiting for the bridge to confirm the cancellation.',
      canCancel: false,
      bridgeUnsupported: false,
    };
  }

  return {
    state: 'available',
    label: 'Cancel',
    title: 'Stop this send and free the radio airtime. The message stays in the chat as cancelled.',
    canCancel: true,
    bridgeUnsupported: false,
  };
}
