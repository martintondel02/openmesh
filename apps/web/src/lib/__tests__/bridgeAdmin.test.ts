import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  BridgeResetError,
  formatBridgeResetCounts,
  requestBridgeReset,
  type BridgeResetResult,
} from '../bridge-admin';

const SETTINGS = { bridgeUrl: 'http://127.0.0.1:8787', token: 'test-token' };

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });
}

const SUCCESS: BridgeResetResult = {
  reset: true,
  identity_changed: true,
  old_node_id: '1111111111111111',
  node_id: '2222222222222222',
  counts: { messages: 12, contacts: 3, channels: 2, nodes: 8, attachments: 4, activity: 19 },
  public_channel: { channel_id: 'a16d63142854cef4', name: 'Public Mesh', psk_set: true, public: true },
};

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('bridge admin: confirmed wipe', () => {
  it('posts WIPE + new identity with the bearer token and returns the parsed result', async () => {
    const fetchMock = vi.fn(async () => jsonResponse(SUCCESS));
    vi.stubGlobal('fetch', fetchMock);

    const result = await requestBridgeReset(SETTINGS);

    expect(result).toEqual(SUCCESS);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe('http://127.0.0.1:8787/api/admin/reset');
    expect(init.method).toBe('POST');
    const headers = init.headers as Record<string, string>;
    expect(headers.Authorization).toBe('Bearer test-token');
    expect(JSON.parse(String(init.body))).toEqual({ confirm: 'WIPE', new_identity: true });
  });

  it('honours newIdentity=false while keeping the confirmation literal', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ ...SUCCESS, identity_changed: false }));
    vi.stubGlobal('fetch', fetchMock);
    await requestBridgeReset(SETTINGS, { newIdentity: false });
    const [, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(JSON.parse(String(init.body))).toEqual({ confirm: 'WIPE', new_identity: false });
  });

  it('surfaces a wrong confirmation as a typed 400 error', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        jsonResponse({ error: 'confirmation_required', message: 'confirm must be the literal string WIPE' }, 400),
      ),
    );
    await expect(requestBridgeReset(SETTINGS)).rejects.toMatchObject({
      name: 'BridgeResetError',
      status: 400,
      code: 'confirmation_required',
      message: 'confirm must be the literal string WIPE',
    });
  });

  it('surfaces unauthorized and unexpected payloads', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ error: 'unauthorized' }, 401)));
    await expect(requestBridgeReset(SETTINGS)).rejects.toMatchObject({ status: 401, code: 'unauthorized' });

    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ reset: false }, 200)));
    await expect(requestBridgeReset(SETTINGS)).rejects.toMatchObject({
      status: 200,
      code: 'invalid_response',
    });
  });

  it('wraps network failures in BridgeResetError', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        throw new Error('connection refused');
      }),
    );
    const failure = await requestBridgeReset(SETTINGS).catch((cause: unknown) => cause);
    expect(failure).toBeInstanceOf(BridgeResetError);
    expect(failure).toMatchObject({ status: 0, code: 'network' });
    expect((failure as Error).message).toContain('connection refused');
  });

  it('formats the cleared counts for the result callout', () => {
    expect(formatBridgeResetCounts(SUCCESS.counts)).toBe(
      '12 messages · 3 contacts · 2 channels · 8 nodes · 4 attachments',
    );
    expect(
      formatBridgeResetCounts({ messages: 1, contacts: 0, channels: 1, nodes: 0, attachments: 0, activity: 0 }),
    ).toBe('1 message · 0 contacts · 1 channel · 0 nodes · 0 attachments');
  });
});
