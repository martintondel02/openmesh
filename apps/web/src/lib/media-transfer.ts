/**
 * Outgoing media transfer presentation (pure helpers).
 *
 * Web image/voice sends are two-step: the bytes go to the bridge first, then the
 * bridge pushes them over the radio as OMP/1 resource frames. The bubble must
 * tell the user which step is happening and never claim success for a transfer
 * that has not been acknowledged.
 *
 * The bridge reports real radio progress through `resource_progress`
 * (`direction: "out"`), and completion through `message_update` /
 * `resource_complete` acks. A direct message also gets a delivery ack; channel
 * media is broadcast and has no per-recipient receipt.
 */

import type { Attachment, DeliveryState } from '../types/api';
import { formatBytes } from './utils';

/**
 * No echo/progress/ack for this long while a media transfer is running → stalled.
 * The bridge releases resource chunks at the duty-aware pace (`task-18`), so a
 * single progress window can legitimately be tens of seconds apart; 75 s still
 * flags a genuinely dead transfer well inside the user's patience.
 */
export const MEDIA_STALL_MS = 75_000;
/** Estimated wall-clock transfer above this shows the "slow media" honesty hint. */
export const SLOW_MEDIA_WALL_CLOCK_MS = 60_000;

export type OutgoingMediaPhase = 'preparing' | 'transferring' | 'stalled' | 'sent' | 'delivered' | 'cancelled' | 'failed';

export interface OutgoingMediaState {
  phase: OutgoingMediaPhase;
  /** Radio transfer fraction reported by the bridge (0..1), or null when unknown. */
  progress: number | null;
  /** Short honest line shown under the bubble. */
  label: string;
  /** Longer explanation / next step; null when there is nothing useful to add. */
  hint: string | null;
  /** Show the Retry action (stalled/failed transfer with a bridge message id). */
  canRetry: boolean;
  /** The transfer is expected to take more than a minute at the current duty cycle. */
  slow: boolean;
}

export interface OutgoingMediaInput {
  attachment: Attachment | null | undefined;
  delivery: DeliveryState;
  error?: string | null;
  /** Bridge message id; 0 while the send command is still in flight. */
  msgId: number;
  group?: boolean;
  sentAtMs: number;
  /** Epoch ms of the last media activity seen locally (send, echo, progress or ack). */
  activityAtMs?: number | null;
  nowMs: number;
  /** Estimated wall-clock time at the region's duty cycle, ms (from `estimateAttachment`). */
  wallClockMs?: number | null;
  stallMs?: number;
}

/** Clamp a raw progress value; null/NaN stay null so the UI can say "unknown". */
export function clampMediaProgress(progress: number | null | undefined): number | null {
  if (typeof progress !== 'number' || !Number.isFinite(progress)) return null;
  return Math.min(1, Math.max(0, progress));
}

/**
 * Wall-clock expectation for a media transfer at the effective duty cycle.
 *
 * `estimateAttachment` uses the legal region table; the bridge can report a
 * different effective duty in the live config (e.g. a regional override), so
 * prefer that when it is known. Falls back to the estimate's own wall clock
 * (which already applies the region duty, or plain transfer time when the
 * region has no duty-cycle limit).
 */
export function mediaWallClockMs(
  estimate: { airtimeMs: number; wallClockMs: number },
  effectiveDutyPct: number | null | undefined,
): number {
  if (typeof effectiveDutyPct === 'number' && Number.isFinite(effectiveDutyPct) && effectiveDutyPct > 0 && effectiveDutyPct < 100) {
    return Math.round(estimate.airtimeMs / (effectiveDutyPct / 100));
  }
  return estimate.wallClockMs;
}

/** Milliseconds since the last media activity (send, echo, progress or ack). */
export function mediaIdleMs(input: Pick<OutgoingMediaInput, 'nowMs' | 'sentAtMs' | 'activityAtMs'>): number {
  const last = typeof input.activityAtMs === 'number' && Number.isFinite(input.activityAtMs) ? input.activityAtMs : input.sentAtMs;
  return Math.max(0, input.nowMs - last);
}

/**
 * `about 3 minutes`, `about 40 seconds`, `under 10 seconds` — used in the slow
 * media hint so the estimate is honest instead of a raw airtime number.
 */
export function formatMediaExpectation(wallClockMs: number | null | undefined): string | null {
  if (typeof wallClockMs !== 'number' || !Number.isFinite(wallClockMs) || wallClockMs <= 0) return null;
  if (wallClockMs < 10_000) return 'under 10 seconds';
  if (wallClockMs < 60_000) return `about ${Math.max(10, Math.round(wallClockMs / 10_000) * 10)} seconds`;
  const minutes = Math.round(wallClockMs / 60_000);
  return minutes === 1 ? 'about 1 minute' : `about ${minutes} minutes`;
}

function slowHint(attachment: Attachment, wallClockMs: number | null | undefined): string | null {
  const expectation = formatMediaExpectation(wallClockMs);
  if (!expectation) return null;
  const size = Math.max(1, Math.round(attachment.size || 0));
  return `This ${formatBytes(size)} can take ${expectation} at the radio's duty cycle. It keeps sending in the background — you can carry on.`;
}

const RETRY_HINT = 'Retrying reuses the copy already uploaded to the bridge, so it does not upload from this browser again.';

/**
 * Describe an outgoing attachment bubble. Returns null for non-attachment or
 * incoming messages so callers can use one guard.
 */
export function describeOutgoingMedia(input: OutgoingMediaInput): OutgoingMediaState | null {
  const attachment = input.attachment;
  if (!attachment) return null;

  const progress = clampMediaProgress(attachment.progress ?? null);
  const isSlow = typeof input.wallClockMs === 'number' && Number.isFinite(input.wallClockMs) && input.wallClockMs >= SLOW_MEDIA_WALL_CLOCK_MS;
  const slow = isSlow ? slowHint(attachment, input.wallClockMs) : null;
  const canRetry = input.msgId > 0;

  if (input.error || input.delivery === 'failed') {
    return {
      phase: 'failed',
      progress,
      label: 'Upload failed',
      hint: input.error ? `${input.error} ${RETRY_HINT}` : RETRY_HINT,
      canRetry,
      slow: isSlow,
    };
  }

  if (input.delivery === 'cancelled') {
    return {
      phase: 'cancelled',
      progress,
      label: 'Cancelled — no more airtime will be used',
      hint: null,
      canRetry: false,
      slow: isSlow,
    };
  }

  if (input.delivery === 'delivered' || input.delivery === 'read') {
    return {
      phase: 'delivered',
      progress: progress ?? 1,
      label: input.group ? 'Sent to the channel' : 'Delivered',
      hint: null,
      canRetry: false,
      slow: isSlow,
    };
  }

  if (input.msgId === 0 && input.delivery === 'pending') {
    return {
      phase: 'preparing',
      progress,
      label: 'Preparing the upload…',
      hint: slow,
      canRetry: false,
      slow: isSlow,
    };
  }

  if (progress !== null && progress >= 1) {
    return {
      phase: 'sent',
      progress: 1,
      label: input.group
        ? 'Sent to the channel — broadcast, no per-person receipt'
        : 'Sent over the radio — waiting for delivery confirmation',
      hint: null,
      canRetry: false,
      slow: isSlow,
    };
  }

  const idleMs = mediaIdleMs(input);
  const stallMs = input.stallMs ?? MEDIA_STALL_MS;
  if (idleMs >= stallMs) {
    return {
      phase: 'stalled',
      progress,
      label: 'Upload stalled — check the radio link; retry',
      hint: [slow, RETRY_HINT].filter(Boolean).join(' ') || RETRY_HINT,
      canRetry,
      slow: isSlow,
    };
  }

  const percent = progress === null ? null : Math.round(progress * 100);
  return {
    phase: 'transferring',
    progress,
    label: percent === null || percent === 0 ? 'Waiting for the radio…' : `Sending over the radio… ${percent}%`,
    hint: slow,
    canRetry: false,
    slow: isSlow,
  };
}
