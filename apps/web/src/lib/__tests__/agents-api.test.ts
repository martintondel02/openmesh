import { describe, expect, it } from 'vitest';
import { AgentsApiError, AgentsRestClient } from '../agents-api';

const BASE = 'http://127.0.0.1:8787';
const TOKEN = 'test-token-not-a-secret';

function fakeFetch(handler: (url: string, init?: RequestInit) => Response | Promise<Response>) {
  const calls: Array<{ url: string; init?: RequestInit }> = [];
  const impl = (async (input: RequestInfo | URL, init?: RequestInit) => {
    calls.push({ url: String(input), init });
    return handler(String(input), init);
  }) as typeof fetch;
  return { calls, impl };
}

describe('agents REST client', () => {
  it('fetches activity with attribution headers and clamps the limit', async () => {
    const { calls, impl } = fakeFetch(() =>
      new Response(JSON.stringify({ entries: [], capacity: 256, agent_connected: false, last_agent_at_ms: null, sources: [] }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      }),
    );
    const payload = (await new AgentsRestClient({ baseUrl: BASE, token: TOKEN, fetchImpl: impl }).getActivityPayload(999)) as Record<string, unknown>;
    expect(payload.capacity).toBe(256);
    expect(calls[0]!.url).toBe(`${BASE}/api/activity?limit=100`);
    const headers = calls[0]!.init?.headers as Record<string, string>;
    expect(headers['X-OpenMesh-Client']).toBe('web');
    expect(headers.Authorization).toBe(`Bearer ${TOKEN}`);
  });

  it('surfaces API errors with status and code', async () => {
    const { impl } = fakeFetch(() => new Response(JSON.stringify({ error: 'unauthorized', message: 'bad token' }), { status: 401 }));
    const error = await new AgentsRestClient({ baseUrl: BASE, fetchImpl: impl }).getActivity(10).catch((cause: unknown) => cause);
    expect(error).toBeInstanceOf(AgentsApiError);
    expect((error as AgentsApiError).status).toBe(401);
    expect((error as AgentsApiError).message).toBe('bad token');
  });

  it('never fabricates a token rotation endpoint', async () => {
    const { calls, impl } = fakeFetch(() => new Response(JSON.stringify({ error: 'not_found' }), { status: 404 }));
    const client = new AgentsRestClient({ baseUrl: BASE, fetchImpl: impl });
    // The v0.3 bridge API has no bridge-token rotation route; the panel shows instructions instead.
    expect((client as unknown as Record<string, unknown>).revokeApiToken).toBeUndefined();
    expect(calls).toHaveLength(0);
  });
});
