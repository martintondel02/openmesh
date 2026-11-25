/**
 * Transport abstraction shared by the live bridge WebSocket client and the
 * seeded demo transport. The store only ever talks to this interface.
 */

import type { ConnectionState, Envelope, TransportMode } from '../types/api';

export interface TransportStatus {
  state: ConnectionState;
  /** Human-readable detail, e.g. "bridge unreachable — demo data". */
  detail?: string;
  /** Reconnect attempt number, 1-based. */
  attempt?: number;
}

export interface UploadedAttachment {
  id: string;
  size: number;
  sha256: string;
}

export interface MeshTransport {
  readonly mode: TransportMode;
  /** Idempotent: safe to call twice. */
  connect(): void;
  disconnect(): void;
  /** Serialise and send one envelope; returns the request id. */
  send(type: string, data?: Record<string, unknown>, id?: string): string;
  /** Subscribe to server→client envelopes. Returns an unsubscribe function. */
  onEvent(listener: (event: Envelope) => void): () => void;
  /** Subscribe to connection status changes. Returns an unsubscribe function. */
  onStatus(listener: (status: TransportStatus) => void): () => void;
  /** Upload raw attachment bytes; resolves with the bridge attachment id. */
  uploadAttachment(blob: Blob, filename?: string): Promise<UploadedAttachment>;
  /** Resolve a displayable URL for a bridge attachment id (object URLs). */
  resolveAttachmentUrl(attachmentId: string, mime?: string | null): Promise<string>;
  dispose(): void;
}

let commandCounter = 0;

/** Short unique id for request/response correlation (`id` field in the envelope). */
export function nextCommandId(prefix = 'c'): string {
  commandCounter += 1;
  return `${prefix}${Date.now().toString(36)}${commandCounter.toString(36)}`;
}
