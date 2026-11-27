/**
 * Display-name, peer-link and delivery presentation helpers (pure functions).
 *
 * Kept framework-free so the naming/fallback precedence, the quality vocabulary and the
 * delivery explanations are unit-testable and identical everywhere they are rendered:
 * conversation list, thread header, bubbles, node inspector, map labels and message details.
 *
 * Naming contract (task-13): prefer the peer's announced name, then the pinned contact
 * alias/name, then `node <first8>` of the self-certifying id. Own node renders as "You".
 */

import type { DeliveryState } from '../types/api';
import { formatLastSeen, signalBars } from './utils';

/* ------------------------------------------------------------------ */
/* Names                                                              */
/* ------------------------------------------------------------------ */

/** `abcdef0123…` short form used next to a friendly name, e.g. `node 68102c09`. */
export function shortNodeId(nodeId: string | null | undefined, length = 8): string {
  const clean = (nodeId ?? '').trim().toLowerCase();
  if (!clean) return 'unknown';
  return clean.slice(0, Math.max(4, length));
}

/** Best human name for a peer, with the task-13 fallback chain. */
export function displayName(parts: {
  announcedName?: string | null;
  contactName?: string | null;
  nodeId: string;
  own?: boolean;
}): string {
  if (parts.own) return 'You';
  const announced = (parts.announcedName ?? '').trim();
  if (announced) return announced;
  const alias = (parts.contactName ?? '').trim();
  if (alias) return alias;
  return `node ${shortNodeId(parts.nodeId)}`;
}

/** `Ada · node 68102c09` — name and short id together for peer details. */
export function nameWithShortId(name: string, nodeId: string | null | undefined): string {
  return `${name} · ${shortNodeId(nodeId)}`;
}

/* ------------------------------------------------------------------ */
/* Peer link quality                                                  */
/* ------------------------------------------------------------------ */

export type PeerQualityLevel = 'strong' | 'ok' | 'weak' | 'offline';
export type PeerQualityTone = 'emerald' | 'lime' | 'amber' | 'slate';

export interface PeerQuality {
  level: PeerQualityLevel;
  label: string;
  tone: PeerQualityTone;
  /** One-sentence explanation used in tooltips and the peer card. */
  hint: string;
}

/** A peer is considered online/current when heard within this window (matches the lists). */
export const PEER_ONLINE_WINDOW_MS = 10 * 60_000;

/**
 * Quality label derived from the reported RSSI plus how recently the peer was heard.
 *
 * Reuses the radio-link vocabulary: Strong / OK / Weak / Offline. A stale peer is Offline even
 * when the last RSSI was good; a freshly heard peer without an RSSI reading is "OK".
 */
export function peerQuality(input: {
  rssi: number | null | undefined;
  lastSeenMs?: number | null;
  nowMs?: number;
  onlineWindowMs?: number;
}): PeerQuality {
  const now = input.nowMs ?? Date.now();
  const windowMs = input.onlineWindowMs ?? PEER_ONLINE_WINDOW_MS;
  const lastSeen = input.lastSeenMs ?? null;
  const stale = lastSeen === null || lastSeen <= 0 || now - lastSeen > windowMs;
  if (stale) {
    const heard = lastSeen ? formatLastSeen(lastSeen, now) : 'never heard';
    return {
      level: 'offline',
      label: 'Offline',
      tone: 'slate',
      hint: `Not heard for a while (${heard}). Messages may queue until a route exists.`,
    };
  }
  const rssi = input.rssi;
  const bars = signalBars(rssi);
  const dbm = rssi === null || rssi === undefined ? 'no RSSI reported' : `${Math.round(rssi)} dBm`;
  if (bars >= 4) return { level: 'strong', label: 'Strong', tone: 'emerald', hint: `Strong signal (${dbm}), heard ${formatLastSeen(lastSeen, now)}.` };
  if (bars === 3) return { level: 'ok', label: 'OK', tone: 'lime', hint: `Usable signal (${dbm}), heard ${formatLastSeen(lastSeen, now)}.` };
  if (bars >= 1) return { level: 'weak', label: 'Weak', tone: 'amber', hint: `Weak signal (${dbm}), heard ${formatLastSeen(lastSeen, now)}. Expect slower transfers.` };
  return { level: 'ok', label: 'OK', tone: 'lime', hint: `Heard ${formatLastSeen(lastSeen, now)}, but the bridge reports no RSSI for this link.` };
}

/** Compact facts shared by the thread-header chip and the node inspector card. */
export interface PeerLinkFacts {
  name: string;
  shortId: string;
  nodeId: string;
  role: string | null;
  hops: number | null;
  rssi: number | null;
  snr: number | null;
  lastHeard: string;
  lastSeenMs: number | null;
  quality: PeerQuality;
  isContact: boolean;
}

export function peerLinkFacts(input: {
  nodeId: string;
  announcedName?: string | null;
  contactName?: string | null;
  isContact?: boolean;
  role?: string | null;
  hops?: number | null;
  rssi?: number | null;
  snr?: number | null;
  lastSeenMs?: number | null;
  nowMs?: number;
}): PeerLinkFacts {
  const now = input.nowMs ?? Date.now();
  return {
    name: displayName({ announcedName: input.announcedName, contactName: input.contactName, nodeId: input.nodeId }),
    shortId: shortNodeId(input.nodeId),
    nodeId: input.nodeId,
    role: input.role ?? null,
    hops: input.hops ?? null,
    rssi: input.rssi ?? null,
    snr: input.snr ?? null,
    lastSeenMs: input.lastSeenMs ?? null,
    lastHeard: input.lastSeenMs ? formatLastSeen(input.lastSeenMs, now) : 'never heard',
    quality: peerQuality({ rssi: input.rssi, lastSeenMs: input.lastSeenMs, nowMs: now }),
    isContact: Boolean(input.isContact),
  };
}

/* ------------------------------------------------------------------ */
/* Delivery presentation                                              */
/* ------------------------------------------------------------------ */

export interface DeliveryPresentation {
  state: DeliveryState;
  /** Short explicit label rendered next to the ticks. */
  label: string;
  /** Full explanation for tooltips and the message-details surface. */
  explanation: string;
  tone: 'slate' | 'sky' | 'emerald' | 'amber' | 'rose';
  /** Whether a per-recipient receipt is meaningful for this message kind. */
  hasReceipt: boolean;
}

/**
 * Explicit delivery wording, with channel-vs-direct semantics.
 *
 * Channel messages are broadcast to everyone holding the key; the bridge cannot know which
 * members received or read them, so they never get delivered/read ticks.
 */
export function deliveryPresentation(input: {
  state: DeliveryState;
  group?: boolean;
  peerName?: string | null;
  error?: string | null;
  hasReceipts?: boolean;
}): DeliveryPresentation {
  if (input.group) {
    const failed = input.state === 'failed' || Boolean(input.error);
    return {
      state: input.state,
      label: failed ? 'Channel send failed' : 'Sent to channel',
      explanation: failed
        ? 'The channel message could not be handed to the radio. Retry when the link is back.'
        : 'Channel messages are broadcast to everyone with the key. There is no per-recipient delivered or read receipt.',
      tone: failed ? 'rose' : 'slate',
      hasReceipt: false,
    };
  }
  const peer = (input.peerName ?? '').trim() || 'this peer';
  const hasReceipts = input.hasReceipts ?? true;
  switch (input.state) {
    case 'pending':
      return {
        state: 'pending',
        label: 'Sending…',
        explanation: 'The message is queued for the bridge and your radio. No airtime is spent until it is sent.',
        tone: 'amber',
        hasReceipt: hasReceipts,
      };
    case 'sent':
      return {
        state: 'sent',
        label: 'Sent to bridge',
        explanation: hasReceipts
          ? `Your bridge accepted the message. Waiting for ${peer} to acknowledge delivery over the mesh.`
          : `Sent to the bridge. Direct delivery receipts are not available for this message.`,
        tone: 'slate',
        hasReceipt: hasReceipts,
      };
    case 'delivered':
      return {
        state: 'delivered',
        label: `Delivered to ${peer}`,
        explanation: `${peer}'s node acknowledged the message (single tick). It has not been opened yet.`,
        tone: 'emerald',
        hasReceipt: true,
      };
    case 'read':
      return {
        state: 'read',
        label: `Read by ${peer}`,
        explanation: `${peer}'s client marked the message read (double tick).`,
        tone: 'sky',
        hasReceipt: true,
      };
    case 'failed':
    default:
      return {
        state: 'failed',
        label: 'Not delivered',
        explanation: input.error
          ? `${input.error} Use Retry to send it again.`
          : `The bridge could not deliver this message to ${peer}. Use Retry to send it again.`,
        tone: 'rose',
        hasReceipt: hasReceipts,
      };
  }
}
