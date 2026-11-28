/**
 * Live bridge client: WebSocket envelope parsing + reconnecting transport.
 *
 * Normative source: `docs/architecture/WEBSOCKET-API.md`
 *  - connect to `ws://host:8787/api/ws?token=<token>` (§1)
 *  - envelope `{v,type,id?,data?}` (§2)
 *  - clients MUST ignore unknown `type`s and unknown fields (§7)
 */

import type { Envelope, TransportMode } from '../types/api';
import type { MeshTransport, TransportStatus, UploadedAttachment } from './transport';
import { nextCommandId } from './transport';

export const PROTOCOL_VERSION = 1;

const KNOWN_WS_TYPES = new Set([
  'hello',
  'status',
  'node',
  'message',
  'message_update',
  'group_message',
  'ack',
  'position',
  'telemetry',
  'store_offer',
  'resource_progress',
  'backbone_status',
  'backbone_peer',
  'backbone_token',
  'config',
  'sync_done',
  'nodes',
  'messages',
  'contacts',
  'channels',
  'channel_created',
  'send_result',
  'cancel_result',
  'message_cancelled',
  'contact_url',
  'channel_url',
  'error',
]);

export function isKnownEventType(type: string): boolean {
  return KNOWN_WS_TYPES.has(type);
}

export interface ParseResult {
  ok: boolean;
  envelope?: Envelope;
  error?: string;
}

/**
 * Parse one server frame. Never throws: malformed JSON or a malformed envelope
 * yields `{ ok: false }` so the socket handler can drop it and keep going.
 */
export function parseEnvelope(raw: unknown): ParseResult {
  if (typeof raw !== 'string') return { ok: false, error: 'frame is not text' };
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return { ok: false, error: 'invalid JSON' };
  }
  return validateEnvelope(value);
}

export function validateEnvelope(value: unknown): ParseResult {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    return { ok: false, error: 'envelope is not an object' };
  }
  const obj = value as Record<string, unknown>;
  if (typeof obj.type !== 'string' || obj.type.length === 0) {
    return { ok: false, error: 'missing type' };
  }
  if (typeof obj.v !== 'number') {
    return { ok: false, error: 'missing protocol version' };
  }
  if (obj.v !== PROTOCOL_VERSION) {
    return { ok: false, error: `unsupported protocol version ${String(obj.v)}` };
  }
  if (obj.id !== undefined && typeof obj.id !== 'string') {
    return { ok: false, error: 'id must be a string' };
  }
  if (obj.data !== undefined && (typeof obj.data !== 'object' || obj.data === null || Array.isArray(obj.data))) {
    return { ok: false, error: 'data must be an object' };
  }
  return {
    ok: true,
    envelope: {
      v: PROTOCOL_VERSION,
      type: obj.type,
      ...(obj.id !== undefined ? { id: obj.id as string } : {}),
      ...(obj.data !== undefined ? { data: obj.data as Record<string, unknown> } : {}),
    },
  };
}

/** Build the WS URL from a user-supplied bridge base URL + token. */
export function buildWsUrl(baseUrl: string, token = ''): string {
  let normalized = (baseUrl || 'http://127.0.0.1:8787').trim();
  normalized = normalized.replace(/\/+$/, '');
  let url: URL;
  if (/^wss?:\/\//i.test(normalized)) {
    url = new URL(normalized);
  } else if (/^https?:\/\//i.test(normalized)) {
    url = new URL(normalized);
    url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  } else {
    url = new URL(`ws://${normalized}`);
  }
  // Accept users pasting the full /api/ws endpoint too.
  if (!/\/api\/ws\/?$/.test(url.pathname)) {
    url.pathname = `${url.pathname.replace(/\/+$/, '')}/api/ws`.replace(/\/{2,}/g, '/');
  }
  if (token) url.searchParams.set('token', token);
  return url.toString();
}

/** HTTP base for REST calls (attachments) derived from the WS URL. */
export function buildHttpBaseUrl(baseUrl: string): string {
  let normalized = (baseUrl || 'http://127.0.0.1:8787').trim().replace(/\/+$/, '');
  if (/^wss?:\/\//i.test(normalized)) normalized = normalized.replace(/^ws(s?):\/\//i, 'http$1://');
  if (!/^https?:\/\//i.test(normalized)) normalized = `http://${normalized}`;
  return normalized;
}

export interface LiveTransportOptions {
  /** Bridge base URL, e.g. `http://127.0.0.1:8787`. */
  baseUrl: string;
  token?: string;
  /** Injected in tests; defaults to the global WebSocket. */
  WebSocketImpl?: typeof WebSocket;
  /** Injected in tests; defaults to global fetch. */
  fetchImpl?: typeof fetch;
  /** First reconnect delay; doubles up to maxReconnectDelayMs. */
  reconnectDelayMs?: number;
  maxReconnectDelayMs?: number;
  onEvent?: (event: Envelope) => void;
  onStatus?: (status: TransportStatus) => void;
}

/** Browser WebSocket client with exponential reconnect. */
export class LiveTransport implements MeshTransport {
  readonly mode: TransportMode = 'live';

  private ws: WebSocket | null = null;
  private readonly eventListeners = new Set<(event: Envelope) => void>();
  private readonly statusListeners = new Set<(status: TransportStatus) => void>();
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private attempt = 0;
  private disposed = false;
  private manualClose = false;
  private readonly WebSocketImpl: typeof WebSocket;
  private readonly baseUrl: string;
  private readonly token: string;
  private readonly fetchImpl: typeof fetch;
  private readonly reconnectDelayMs: number;
  private readonly maxReconnectDelayMs: number;

  constructor(options: LiveTransportOptions) {
    this.baseUrl = options.baseUrl;
    this.token = options.token ?? '';
    this.WebSocketImpl = options.WebSocketImpl ?? globalThis.WebSocket;
    this.fetchImpl = options.fetchImpl ?? globalThis.fetch.bind(globalThis);
    this.reconnectDelayMs = options.reconnectDelayMs ?? 1200;
    this.maxReconnectDelayMs = options.maxReconnectDelayMs ?? 12_000;
    if (options.onEvent) this.eventListeners.add(options.onEvent);
    if (options.onStatus) this.statusListeners.add(options.onStatus);
  }

  onEvent(listener: (event: Envelope) => void): () => void {
    this.eventListeners.add(listener);
    return () => this.eventListeners.delete(listener);
  }

  onStatus(listener: (status: TransportStatus) => void): () => void {
    this.statusListeners.add(listener);
    return () => this.statusListeners.delete(listener);
  }

  private emitStatus(status: TransportStatus): void {
    for (const listener of this.statusListeners) listener(status);
  }

  private emitEvent(event: Envelope): void {
    for (const listener of this.eventListeners) listener(event);
  }

  connect(): void {
    if (this.disposed) return;
    if (this.ws && (this.ws.readyState === WebSocket.OPEN || this.ws.readyState === WebSocket.CONNECTING)) return;
    if (!this.WebSocketImpl) {
      this.emitStatus({ state: 'offline', detail: 'WebSocket is not available in this runtime' });
      return;
    }
    this.manualClose = false;
    this.emitStatus({ state: this.attempt > 0 ? 'reconnecting' : 'connecting', attempt: this.attempt });
    let socket: WebSocket;
    try {
      socket = new this.WebSocketImpl(buildWsUrl(this.baseUrl, this.token));
    } catch {
      this.scheduleReconnect('Could not open a WebSocket to the bridge');
      return;
    }
    this.ws = socket;

    socket.onopen = () => {
      this.attempt = 0;
      this.emitStatus({ state: 'connected' });
      this.send('sync', { since_ms: 0 }, nextCommandId('s'));
    };

    socket.onmessage = (message: MessageEvent) => {
      const result = parseEnvelope(message.data);
      if (!result.ok || !result.envelope) return;
      // Unknown event types are ignored per spec §7.
      if (!isKnownEventType(result.envelope.type)) return;
      this.emitEvent(result.envelope);
    };

    socket.onerror = () => {
      // `onclose` follows; avoid double status churn here.
    };

    socket.onclose = (event: CloseEvent) => {
      this.ws = null;
      if (this.disposed || this.manualClose) return;
      const unauthorized = event.code === 4401;
      this.attempt += 1;
      const detail = unauthorized
        ? 'Bridge rejected the token (HTTP 4401)'
        : event.code === 1006
          ? 'Bridge unreachable'
          : `Connection closed (${event.code || 'no code'})`;
      this.emitStatus({ state: 'reconnecting', detail, attempt: this.attempt });
      if (!unauthorized) this.scheduleReconnect(detail);
    };
  }

  private scheduleReconnect(detail: string): void {
    if (this.disposed || this.reconnectTimer) return;
    const delay = Math.min(this.reconnectDelayMs * 2 ** Math.max(0, this.attempt - 1), this.maxReconnectDelayMs);
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      this.attempt += 1;
      this.emitStatus({ state: 'reconnecting', detail, attempt: this.attempt });
      this.connect();
    }, delay);
  }

  send(type: string, data?: Record<string, unknown>, id?: string): string {
    const commandId = id ?? nextCommandId();
    const envelope: Envelope = {
      v: PROTOCOL_VERSION,
      type,
      id: commandId,
      ...(data !== undefined ? { data } : {}),
    };
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      try {
        this.ws.send(JSON.stringify(envelope));
      } catch {
        // Socket closed between state check and send; the reconnect will resync.
      }
    }
    return commandId;
  }

  async uploadAttachment(blob: Blob, filename?: string): Promise<UploadedAttachment> {
    const base = buildHttpBaseUrl(this.baseUrl);
    const headers: Record<string, string> = { 'Content-Type': blob.type || 'application/octet-stream' };
    if (this.token) headers.Authorization = `Bearer ${this.token}`;
    if (filename) headers['X-OpenMesh-Filename'] = encodeURIComponent(filename);
    const response = await this.fetchImpl(`${base}/api/attachments`, {
      method: 'POST',
      headers,
      body: blob,
    });
    if (!response.ok) throw new Error(`Attachment upload failed (${response.status})`);
    const json = (await response.json()) as { id?: string; size?: number; sha256?: string };
    if (!json.id) throw new Error('Bridge did not return an attachment id');
    return { id: json.id, size: json.size ?? blob.size, sha256: json.sha256 ?? '' };
  }

  private objectUrls = new Map<string, string>();

  async resolveAttachmentUrl(attachmentId: string, mime?: string | null): Promise<string> {
    const cached = this.objectUrls.get(attachmentId);
    if (cached) return cached;
    const base = buildHttpBaseUrl(this.baseUrl);
    const headers: Record<string, string> = {};
    if (this.token) headers.Authorization = `Bearer ${this.token}`;
    // Browsers cannot add Authorization to <img>/<audio> requests; fetch to a blob.
    const response = await this.fetchImpl(`${base}/api/attachments/${encodeURIComponent(attachmentId)}`, { headers });
    if (!response.ok) throw new Error(`Attachment ${attachmentId} unavailable (${response.status})`);
    const blob = await response.blob();
    const typed = mime && blob.type !== mime ? new Blob([blob], { type: mime }) : blob;
    const url = URL.createObjectURL(typed);
    this.objectUrls.set(attachmentId, url);
    return url;
  }

  disconnect(): void {
    this.manualClose = true;
    this.attempt = 0;
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    if (this.ws) {
      try {
        this.ws.close(1000, 'client closing');
      } catch {
        // ignore
      }
      this.ws = null;
    }
    this.emitStatus({ state: 'idle' });
  }

  dispose(): void {
    this.disposed = true;
    this.disconnect();
    for (const url of this.objectUrls.values()) URL.revokeObjectURL(url);
    this.objectUrls.clear();
    this.eventListeners.clear();
    this.statusListeners.clear();
  }
}
