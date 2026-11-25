import { describe, expect, it } from 'vitest';
import { SetupApiError, SetupRestClient } from '../setup-api';

const BASE = 'http://127.0.0.1:8787';
const TOKEN = 'test-token-not-a-secret';

function fakeFetch(handler: (url: string, init?: RequestInit) => Response | Promise<Response>) {
  const calls: Array<{ url: string; init?: RequestInit }> = [];
  const impl = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    calls.push({ url, init });
    return handler(url, init);
  }) as typeof fetch;
  return { calls, impl };
}

function json(payload: unknown, status = 200): Response {
  return new Response(JSON.stringify(payload), { status, headers: { 'Content-Type': 'application/json' } });
}

describe('setup REST client', () => {
  it('GETs /api/setup/state with the bearer token and the web attribution header', async () => {
    const { calls, impl } = fakeFetch(() =>
      json({
        first_run: false,
        setup_completed: true,
        checks: { identity: { ok: true }, radio: { ok: true }, api_token: { ok: true }, channel: { ok: true }, backbone: { ok: false }, agent: { ok: false } },
        recommended: { region: 'EU868', profile: 'MEDIUM_FAST', role: 'client' },
        counts: { nodes: 4 },
      }),
    );
    const state = await new SetupRestClient({ baseUrl: BASE, token: TOKEN, fetchImpl: impl }).getState();
    expect(state.setup_completed).toBe(true);
    expect(state.checks.identity).toBe(true);
    expect(calls[0]!.url).toBe(`${BASE}/api/setup/state`);
    const headers = calls[0]!.init?.headers as Record<string, string>;
    expect(headers.Authorization).toBe(`Bearer ${TOKEN}`);
    expect(headers['X-OpenMesh-Client']).toBe('web');
  });

  it('POSTs the quick-setup body and normalises the one-time channel', async () => {
    const { calls, impl } = fakeFetch(() =>
      json({
        applied: { node_name: 'my-phone', channel_created: true },
        channel: { channel_id: 'abc123', name: 'Familien', invite_url: 'openmesh://channel?psk=demo', psk_once: 'demo-once' },
        checklist: [{ id: 'identity', label: 'Identity created', ok: true }],
        next_steps: [{ title: 'Say hello', detail: 'Open Chats', help_topic: 'messaging' }],
      }),
    );
    const result = await new SetupRestClient({ baseUrl: BASE, fetchImpl: impl }).quickSetup({
      role: 'client',
      country: 'NO',
      privacy: 'private',
      create_channel: true,
    });
    expect(result.channel?.psk_once).toBe('demo-once');
    expect(calls[0]!.init?.method).toBe('POST');
    expect(calls[0]!.url).toBe(`${BASE}/api/setup/quick`);
    expect(JSON.parse(String(calls[0]!.init?.body))).toMatchObject({ role: 'client', country: 'NO', privacy: 'private' });
  });

  it('GETs the recommendation for a country and role', async () => {
    const { calls, impl } = fakeFetch(() =>
      json({ country: 'US', role: 'tracker', region: 'US915', profile: 'MEDIUM_FAST', store_forward: false, position_sharing: true, announce_interval_s: 3600, warning: null }),
    );
    const recommendation = await new SetupRestClient({ baseUrl: BASE, fetchImpl: impl }).getRecommendation('US', 'tracker');
    expect(recommendation.region).toBe('US915');
    expect(calls[0]!.url).toBe(`${BASE}/api/setup/recommend?country=US&role=tracker`);
  });

  it('surfaces friendly errors with code and help topic', async () => {
    const { impl } = fakeFetch(() => json({ error: 'bad_request', code: 'bad_country', message: 'Unknown country; using EU868.' }, 400));
    const error = await new SetupRestClient({ baseUrl: BASE, fetchImpl: impl }).getState().catch((cause: unknown) => cause);
    expect(error).toBeInstanceOf(SetupApiError);
    expect((error as SetupApiError).status).toBe(400);
    expect((error as SetupApiError).message).toContain('Unknown country');
  });
});
