import { describe, expect, it } from 'vitest';
import {
  BackboneApiError,
  BackboneRestClient,
  backboneRequestHeaders,
  buildBackboneApiUrl,
} from '../backbone-api';

interface RecordedCall {
  url: string;
  init?: RequestInit;
}

function jsonResponse(payload: unknown, status = 200): Response {
  return new Response(JSON.stringify(payload), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });
}

function fakeFetch(handler: (url: string, init?: RequestInit) => Response | Promise<Response>) {
  const calls: RecordedCall[] = [];
  const impl = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = typeof input === 'string' ? input : input instanceof URL ? input.toString() : input.url;
    calls.push({ url, init });
    return handler(url, init);
  }) as typeof fetch;
  return { calls, impl };
}

const BASE = 'http://127.0.0.1:8787';
const TOKEN = 'bridge-token';
const PEER_ID = '4a5b6c7d8e9f0a1b';

describe('backbone-api: url and header helpers', () => {
  it('joins base URL and path without duplicate slashes', () => {
    expect(buildBackboneApiUrl(BASE, '/api/backbone')).toBe('http://127.0.0.1:8787/api/backbone');
    expect(buildBackboneApiUrl(`${BASE}/`, 'api/backbone/peers')).toBe('http://127.0.0.1:8787/api/backbone/peers');
    expect(buildBackboneApiUrl('ws://127.0.0.1:8787', '/api/backbone')).toBe('http://127.0.0.1:8787/api/backbone');
  });

  it('adds bearer auth and JSON content type only when needed', () => {
    expect(backboneRequestHeaders(TOKEN)).toEqual({
      Accept: 'application/json',
      Authorization: 'Bearer bridge-token',
    });
    expect(backboneRequestHeaders(null, true)).toEqual({
      Accept: 'application/json',
      'Content-Type': 'application/json',
    });
  });
});

describe('backbone-api: REST client', () => {
  it('fetches and normalizes GET /api/backbone', async () => {
    const { calls, impl } = fakeFetch(() =>
      jsonResponse({
        enabled: true,
        bind: '0.0.0.0',
        port: 9443,
        auth: 'token',
        token_set: true,
        max_peers: 32,
        allowlist: [],
        store_forward: true,
        name: 'oslo-backbone',
        public_host: 'mesh.example.org',
        ping_interval_s: 30,
      }),
    );
    const client = new BackboneRestClient({ baseUrl: BASE, token: TOKEN, fetchImpl: impl });
    const config = await client.getConfig();
    expect(config.enabled).toBe(true);
    expect(config.port).toBe(9443);
    expect(config.public_host).toBe('mesh.example.org');
    expect(calls[0]!.url).toBe(`${BASE}/api/backbone`);
    expect((calls[0]!.init?.headers as Record<string, string>).Authorization).toBe(`Bearer ${TOKEN}`);
  });

  it('handles the {enabled:false} response for non-backbone bridges', async () => {
    const { impl } = fakeFetch(() => jsonResponse({ enabled: false }));
    const client = new BackboneRestClient({ baseUrl: BASE, fetchImpl: impl });
    const config = await client.getConfig();
    expect(config.enabled).toBe(false);
    expect(config.running).toBe(false);
    expect(config.port).toBe(9443);
  });

  it('lists peers and degrades to [] when the endpoint is absent', async () => {
    const { impl } = fakeFetch((url) =>
      url.endsWith('/peers') ? jsonResponse([{ node_id: PEER_ID, name: 'lofoten', direction: 'inbound' }]) : jsonResponse({}),
    );
    const client = new BackboneRestClient({ baseUrl: BASE, token: TOKEN, fetchImpl: impl });
    const peers = await client.getPeers();
    expect(peers).toHaveLength(1);
    expect(peers[0]!.node_id).toBe(PEER_ID);

    const missing = fakeFetch(() => jsonResponse({ error: 'not found' }, 404));
    const missingClient = new BackboneRestClient({ baseUrl: BASE, token: TOKEN, fetchImpl: missing.impl });
    expect(await missingClient.getPeers()).toEqual([]);
  });

  it('posts a partial config and reports restarted', async () => {
    const { calls, impl } = fakeFetch(() =>
      jsonResponse({
        enabled: true,
        running: true,
        bind: '0.0.0.0',
        port: 9555,
        auth: 'token',
        token_set: true,
        max_peers: 32,
        allowlist: [],
        store_forward: true,
        name: 'renamed',
        public_host: 'mesh.example.org',
        ping_interval_s: 30,
        restarted: true,
      }),
    );
    const client = new BackboneRestClient({ baseUrl: BASE, token: TOKEN, fetchImpl: impl });
    const result = await client.updateConfig({ port: 9555, name: 'renamed' });
    expect(result.port).toBe(9555);
    expect(result.restarted).toBe(true);
    expect(calls[0]!.init?.method).toBe('POST');
    expect(calls[0]!.url).toBe(`${BASE}/api/backbone/config`);
    expect(JSON.parse(String(calls[0]!.init?.body))).toEqual({ port: 9555, name: 'renamed' });
    expect((calls[0]!.init?.headers as Record<string, string>)['Content-Type']).toBe('application/json');
  });

  it('rotates the token once', async () => {
    const { calls, impl } = fakeFetch(() => jsonResponse({ token: 'new-base64url-token' }));
    const client = new BackboneRestClient({ baseUrl: BASE, token: TOKEN, fetchImpl: impl });
    const token = await client.rotateToken();
    expect(token).toBe('new-base64url-token');
    expect(calls[0]!.init?.method).toBe('POST');
    expect(calls[0]!.url).toBe(`${BASE}/api/backbone/token/rotate`);
  });

  it('throws when rotation returns no token', async () => {
    const { impl } = fakeFetch(() => jsonResponse({}));
    const client = new BackboneRestClient({ baseUrl: BASE, token: TOKEN, fetchImpl: impl });
    await expect(client.rotateToken()).rejects.toBeInstanceOf(BackboneApiError);
  });

  it('kicks a peer via DELETE with an encoded node id', async () => {
    const { calls, impl } = fakeFetch(() => jsonResponse({ disconnected: true }));
    const client = new BackboneRestClient({ baseUrl: BASE, token: TOKEN, fetchImpl: impl });
    expect(await client.kickPeer(PEER_ID)).toBe(true);
    expect(calls[0]!.url).toBe(`${BASE}/api/backbone/peers/${PEER_ID}`);
    expect(calls[0]!.init?.method).toBe('DELETE');
  });

  it('fetches the setup payload and QR svg', async () => {
    const setupPayload = {
      listen: '0.0.0.0:9443',
      public_host: 'mesh.example.org',
      connection_string: 'openmesh://backbone?host=mesh.example.org&port=9443&v=1',
      token_set: true,
      auth: 'token',
      steps: [{ title: 'Install', commands: ['curl install.sh | sh'] }],
      systemd_unit: '[Unit]\nDescription=OpenMesh',
      firewall: { firewalld: ['sudo firewall-cmd'], ufw: ['sudo ufw allow 9443/tcp'] },
      notes: ['encrypted'],
      panel_url: 'http://127.0.0.1:8787/backbone',
    };
    const { impl } = fakeFetch((url) =>
      url.endsWith('/qr.svg')
        ? new Response('<svg xmlns="http://www.w3.org/2000/svg"></svg>', { status: 200, headers: { 'Content-Type': 'image/svg+xml' } })
        : jsonResponse(setupPayload),
    );
    const client = new BackboneRestClient({ baseUrl: BASE, token: TOKEN, fetchImpl: impl });
    const setup = await client.getSetup();
    expect(setup.steps[0]!.title).toBe('Install');
    expect(setup.firewall.ufw).toHaveLength(1);
    const svg = await client.getQrSvg();
    expect(svg.startsWith('<svg')).toBe(true);
  });

  it('surfaces API failures with status and code', async () => {
    const { impl } = fakeFetch(() => jsonResponse({ code: 'unauthorized', message: 'bad token' }, 401));
    const client = new BackboneRestClient({ baseUrl: BASE, token: 'wrong', fetchImpl: impl });
    const error = await client.getConfig().catch((cause: unknown) => cause);
    expect(error).toBeInstanceOf(BackboneApiError);
    expect((error as BackboneApiError).status).toBe(401);
    expect((error as BackboneApiError).message).toBe('bad token');
  });

  it('does not send an Authorization header without a token', async () => {
    const { calls, impl } = fakeFetch(() => jsonResponse({ enabled: false }));
    const client = new BackboneRestClient({ baseUrl: BASE, fetchImpl: impl });
    await client.getConfig();
    expect((calls[0]!.init?.headers as Record<string, string>).Authorization).toBeUndefined();
  });

  it('aborts slow requests via the configured timeout', async () => {
    const { impl } = fakeFetch(
      (_url, init) =>
        new Promise<Response>((_resolve, reject) => {
          init?.signal?.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')));
        }),
    );
    const client = new BackboneRestClient({ baseUrl: BASE, token: TOKEN, fetchImpl: impl, timeoutMs: 20 });
    await expect(client.getConfig()).rejects.toMatchObject({ name: 'AbortError' });
  });
});
