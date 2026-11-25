import { describe, expect, it, vi } from 'vitest';
import {
  LiveTransport,
  buildHttpBaseUrl,
  buildWsUrl,
  isKnownEventType,
  parseEnvelope,
  validateEnvelope,
} from '../ws';
import type { Envelope } from '../../types/api';

describe('ws: envelope parser', () => {
  it('parses a valid server event', () => {
    const raw = JSON.stringify({ v: 1, type: 'hello', data: { bridge_version: '0.1.0' } });
    const result = parseEnvelope(raw);
    expect(result.ok).toBe(true);
    expect(result.envelope?.type).toBe('hello');
    expect(result.envelope?.data).toEqual({ bridge_version: '0.1.0' });
  });

  it('drops malformed JSON without throwing', () => {
    const result = parseEnvelope('{not json');
    expect(result.ok).toBe(false);
    expect(result.error).toMatch(/invalid JSON/i);
  });

  it('drops unknown protocol versions (spec §7)', () => {
    const result = parseEnvelope(JSON.stringify({ v: 2, type: 'hello', data: {} }));
    expect(result.ok).toBe(false);
    expect(result.error).toMatch(/version/i);
  });

  it('requires a type and an object data payload', () => {
    expect(validateEnvelope({ v: 1 }).ok).toBe(false);
    expect(validateEnvelope({ v: 1, type: '' }).ok).toBe(false);
    expect(validateEnvelope({ v: 1, type: 'status', data: [] }).ok).toBe(false);
    expect(validateEnvelope({ v: 1, type: 'status', data: 'nope' }).ok).toBe(false);
    expect(validateEnvelope({ v: 1, type: 'status', id: 7 }).ok).toBe(false);
    expect(validateEnvelope(null).ok).toBe(false);
    expect(validateEnvelope([]).ok).toBe(false);
  });

  it('keeps command ids for request/response correlation', () => {
    const result = parseEnvelope(JSON.stringify({ v: 1, type: 'send_result', id: 'm1', data: { msg_id: 7 } }));
    expect(result.ok).toBe(true);
    expect(result.envelope?.id).toBe('m1');
  });

  it('accepts data-less events', () => {
    const result = parseEnvelope(JSON.stringify({ v: 1, type: 'sync_done' }));
    expect(result.ok).toBe(true);
    expect(result.envelope?.data).toBeUndefined();
  });

  it('recognises every documented event type and ignores unknown ones', () => {
    for (const type of [
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
      'config',
      'sync_done',
      'nodes',
      'messages',
      'contacts',
      'channels',
      'channel_created',
      'send_result',
      'contact_url',
      'channel_url',
      'error',
    ]) {
      expect(isKnownEventType(type)).toBe(true);
    }
    expect(isKnownEventType('future_event')).toBe(false);
  });
});

describe('ws: urls', () => {
  it('derives the websocket URL from http base URLs', () => {
    expect(buildWsUrl('http://127.0.0.1:8787')).toBe('ws://127.0.0.1:8787/api/ws');
    expect(buildWsUrl('http://127.0.0.1:8787/')).toBe('ws://127.0.0.1:8787/api/ws');
    expect(buildWsUrl('https://bridge.example:9000')).toBe('wss://bridge.example:9000/api/ws');
  });

  it('keeps an explicit /api/ws path and appends the token', () => {
    expect(buildWsUrl('ws://127.0.0.1:8787/api/ws', 'secret')).toBe('ws://127.0.0.1:8787/api/ws?token=secret');
  });

  it('accepts a bare host:port', () => {
    expect(buildWsUrl('127.0.0.1:8787')).toBe('ws://127.0.0.1:8787/api/ws');
  });

  it('derives the REST base URL for attachment uploads', () => {
    expect(buildHttpBaseUrl('ws://127.0.0.1:8787')).toBe('http://127.0.0.1:8787');
    expect(buildHttpBaseUrl('wss://bridge.example')).toBe('https://bridge.example');
    expect(buildHttpBaseUrl('127.0.0.1:8787')).toBe('http://127.0.0.1:8787');
  });
});

/* A minimal WebSocket double: enough to exercise connect/send/close handling. */
class FakeSocket {
  static instances: FakeSocket[] = [];
  readyState = 0;
  sent: string[] = [];
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  onclose: ((event: CloseEvent) => void) | null = null;
  url: string;

  constructor(url: string) {
    this.url = url;
    FakeSocket.instances.push(this);
  }

  send(data: string) {
    this.sent.push(data);
  }

  close() {
    this.readyState = 3;
  }

  open() {
    this.readyState = 1;
    this.onopen?.(new Event('open'));
  }

  message(data: string) {
    this.onmessage?.({ data } as MessageEvent);
  }
}

describe('ws: live transport', () => {
  it('connects with the token, sends sync and forwards known events', () => {
    FakeSocket.instances = [];
    const events: Envelope[] = [];
    const statuses: string[] = [];
    const transport = new LiveTransport({
      baseUrl: 'http://127.0.0.1:8787',
      token: 'tok_123',
      WebSocketImpl: FakeSocket as unknown as typeof WebSocket,
    });
    transport.onEvent((event) => events.push(event));
    transport.onStatus((status) => statuses.push(status.state));
    transport.connect();

    const socket = FakeSocket.instances[0]!;
    expect(socket.url).toBe('ws://127.0.0.1:8787/api/ws?token=tok_123');
    expect(statuses).toContain('connecting');

    socket.open();
    expect(statuses).toContain('connected');
    expect(socket.sent.length).toBe(1);
    const sync = JSON.parse(socket.sent[0]!) as Envelope;
    expect(sync.v).toBe(1);
    expect(sync.type).toBe('sync');
    expect(sync.id).toBeTruthy();
    expect(sync.data).toEqual({ since_ms: 0 });

    socket.message(JSON.stringify({ v: 1, type: 'hello', data: { bridge_version: '0.1.0' } }));
    expect(events.length).toBe(1);
    expect(events[0]!.type).toBe('hello');

    socket.message('garbage');
    socket.message(JSON.stringify({ v: 1, type: 'future_event', data: {} }));
    expect(events.length).toBe(1);

    transport.dispose();
  });

  it('sends envelope-shaped commands with generated ids', () => {
    FakeSocket.instances = [];
    const transport = new LiveTransport({
      baseUrl: 'http://127.0.0.1:8787',
      WebSocketImpl: FakeSocket as unknown as typeof WebSocket,
    });
    transport.connect();
    const socket = FakeSocket.instances[0]!;
    socket.open();
    const id = transport.send('send_message', { to: 'abcd', text: 'hi' });
    expect(id).toBeTruthy();
    const envelope = JSON.parse(socket.sent[1]!) as Envelope;
    expect(envelope.id).toBe(id);
    expect(envelope.type).toBe('send_message');
    expect(envelope.data).toEqual({ to: 'abcd', text: 'hi' });
    transport.dispose();
  });

  it('reports unauthorized closes without looping forever', () => {
    FakeSocket.instances = [];
    const statuses: string[] = [];
    const transport = new LiveTransport({
      baseUrl: 'http://127.0.0.1:8787',
      WebSocketImpl: FakeSocket as unknown as typeof WebSocket,
    });
    transport.onStatus((status) => statuses.push(status.state));
    transport.connect();
    const socket = FakeSocket.instances[0]!;
    socket.readyState = 3;
    socket.onclose?.({ code: 4401 } as CloseEvent);
    expect(statuses).toContain('reconnecting');
    transport.dispose();
  });

  it('does not throw when posting to a closed socket', () => {
    const transport = new LiveTransport({
      baseUrl: 'http://127.0.0.1:8787',
      WebSocketImpl: FakeSocket as unknown as typeof WebSocket,
    });
    const spy = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    expect(() => transport.send('announce', {})).not.toThrow();
    spy.mockRestore();
    transport.dispose();
  });
});
