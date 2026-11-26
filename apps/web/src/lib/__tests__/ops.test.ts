import { describe, expect, it } from 'vitest';
import {
  BACKBONE_LINK_DEDUP_NOTE,
  MCP_HTTP_WARNINGS,
  MCP_CONFIG_DEFAULTS,
  channelIdPattern,
  envNamePattern,
  gatewayDraftFromConnector,
  gatewayStatusView,
  gatewayTestSummary,
  hasLegacyBackboneLink,
  mcpClientCommand,
  mcpConfigSnippet,
  mcpEndpointFromBridgeUrl,
  mcpScopeLabel,
  mcpStatusOrDefault,
  mcpTransportLabel,
  normalizeBackboneLinksResponse,
  normalizeGatewayConnector,
  normalizeGatewaysResponse,
  normalizeGatewayTestResult,
  normalizeMcpStatus,
  redactBackboneLinkUrl,
  sanitizeSecretEnvName,
  validateBackboneLinkDraft,
  validateGatewayDraft,
  deriveInboundUrl,
  type GatewayDraft,
} from '../ops';
import { OpsApiError, OpsRestClient } from '../ops-api';
import { createDemoDataset } from '../mock/data';
import type { BackboneConfig } from '../../types/backbone';
import type { GatewayConnector } from '../../types/ops';

const CHANNEL_ID = 'dba1337a96acc686';
const OTHER_CHANNEL = 'c1a2b3d4e5f60718';

function rawGateway(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: 'sms-ops',
    kind: 'webhook',
    enabled: true,
    outbound: {
      url: 'https://gateway.example/openmesh',
      secret_env: 'OPENMESH_SMS_OUT_SECRET',
      channel_ids: [CHANNEL_ID],
      include_direct: false,
      include_attachments: false,
      timeout_ms: 10000,
      max_retries: 3,
    },
    inbound: {
      enabled: true,
      secret_env: 'OPENMESH_SMS_IN_SECRET',
      channel_id: CHANNEL_ID,
      max_chars: 400,
    },
    status: {
      last_success_ms: 1_700_000_000_000,
      last_failure_ms: 1_699_999_000_000,
      last_error: null,
      retry_count: 0,
      secrets: { outbound: true, inbound: true },
    },
    ...overrides,
  };
}

function validDraft(overrides: Partial<GatewayDraft> = {}): GatewayDraft {
  return {
    id: 'sms-ops',
    url: 'https://gateway.example/openmesh',
    outSecretEnv: 'OPENMESH_SMS_OUT_SECRET',
    inSecretEnv: 'OPENMESH_SMS_IN_SECRET',
    channelIds: '',
    includeDirect: false,
    enabled: true,
    inboundEnabled: true,
    inboundChannelId: CHANNEL_ID,
    inboundMaxChars: '400',
    ...overrides,
  };
}

describe('gateway normalization and redaction', () => {
  it('normalizes array and wrapped responses, keeping only env-var names for secrets', () => {
    const raw = rawGateway({
      outbound: {
        ...(rawGateway().outbound as Record<string, unknown>),
        secret: 'leaked-secret-value',
        secret_value: 'another-leak',
      },
      secret: 'top-level-leak',
      status: { last_success_ms: 5, last_failure_ms: 7, last_error: 'timeout', retries: 2, secret: 'nope' },
    });
    const connectors = normalizeGatewaysResponse({ connectors: [raw] });
    expect(connectors).toHaveLength(1);
    const connector = connectors[0]!;
    expect(connector.id).toBe('sms-ops');
    expect(connector.outbound.secret_env).toBe('OPENMESH_SMS_OUT_SECRET');
    expect(connector.inbound.secret_env).toBe('OPENMESH_SMS_IN_SECRET');
    expect(connector.status.retries).toBe(2);
    expect(connector.status.last_error).toBe('timeout');
    expect(connector.status.out_secret_present).toBe(true);
    expect(connector.status.in_secret_present).toBe(true);
    expect(JSON.stringify(connector)).not.toContain('leaked-secret-value');
    expect(JSON.stringify(connector)).not.toContain('another-leak');
    expect(JSON.stringify(connector)).not.toContain('top-level-leak');
    expect(normalizeGatewaysResponse([raw])[0]!.id).toBe('sms-ops');
  });

  it('drops invalid ids and rejects inline secret values masquerading as env names', () => {
    expect(normalizeGatewayConnector({ id: 'Bad Id!' })).toBeNull();
    expect(normalizeGatewayConnector(null)).toBeNull();
    expect(sanitizeSecretEnvName('OPENMESH_OUT_SECRET')).toBe('OPENMESH_OUT_SECRET');
    expect(sanitizeSecretEnvName('lowercase-env')).toBeNull();
    expect(sanitizeSecretEnvName('a secret value with spaces')).toBeNull();
    expect(sanitizeSecretEnvName(null)).toBeNull();
    expect(normalizeGatewaysResponse({ connectors: [{ id: 'x' }, { id: 'bad id' }] })[0]!.id).toBe('x');
  });

  it('describes status with retries, failures and disabled state', () => {
    const ok = normalizeGatewayConnector(rawGateway())!;
    expect(gatewayStatusView(ok)).toMatchObject({ tone: 'accent', label: 'delivering', failing: false });
    const failing = normalizeGatewayConnector(rawGateway({ status: { last_error: 'adapter timeout', retry_count: 3 } }))!;
    expect(gatewayStatusView(failing)).toMatchObject({ tone: 'rose', failing: true });
    expect(gatewayStatusView(failing).label).toContain('3 retries');
    const disabled = normalizeGatewayConnector(rawGateway({ enabled: false }))!;
    expect(gatewayStatusView(disabled)).toMatchObject({ tone: 'slate', label: 'disabled' });
    const noSecret = normalizeGatewayConnector(rawGateway({ outbound: { ...(rawGateway().outbound as Record<string, unknown>), secret_env: null }, status: { out_secret_present: true } }))!;
    expect(gatewayStatusView(noSecret).tone).toBe('amber');
  });
});

describe('gateway form validation and inbound URL', () => {
  it('accepts a valid env-name-only form and never emits secret values', () => {
    const validation = validateGatewayDraft(validDraft({ channelIds: `${CHANNEL_ID}, ${OTHER_CHANNEL}` }));
    expect(validation.errors).toEqual({});
    expect(validation.config).toMatchObject({
      id: 'sms-ops',
      kind: 'webhook',
      enabled: true,
      outbound: {
        secret_env: 'OPENMESH_SMS_OUT_SECRET',
        channel_ids: [CHANNEL_ID, OTHER_CHANNEL],
        include_attachments: false,
      },
      inbound: {
        enabled: true,
        secret_env: 'OPENMESH_SMS_IN_SECRET',
        channel_id: CHANNEL_ID,
        max_chars: 400,
      },
    });
    const body = JSON.stringify(validation.config);
    expect(body).not.toContain('leaked');
    expect(body).not.toContain('OPENMESH_SMS_OUT_SECRET=');
  });

  it('rejects inline secrets, bad URLs, bad channel ids and bad inbound fields', () => {
    const badSecret = validateGatewayDraft(validDraft({ outSecretEnv: 'super-secret-value' }));
    expect(badSecret.config).toBeNull();
    expect(badSecret.errors.outSecretEnv).toContain('NAME');
    const badUrl = validateGatewayDraft(validDraft({ url: 'ftp://example.com/hook' }));
    expect(badUrl.errors.url).toContain('http(s)');
    expect(validateGatewayDraft(validDraft({ url: 'not a url' })).errors.url).toBeTruthy();
    const badChannel = validateGatewayDraft(validDraft({ channelIds: 'nope' }));
    expect(badChannel.errors.channelIds).toContain('16 hex');
    const noInboundSecret = validateGatewayDraft(validDraft({ inSecretEnv: '' }));
    expect(noInboundSecret.errors.inSecretEnv).toContain('Inbound needs');
    const noInboundChannel = validateGatewayDraft(validDraft({ inboundChannelId: '' }));
    expect(noInboundChannel.errors.inboundChannelId).toContain('channel');
    const badMax = validateGatewayDraft(validDraft({ inboundMaxChars: '0' }));
    expect(badMax.errors.inboundMaxChars).toContain('between 1 and 4000');
    const duplicate = validateGatewayDraft(validDraft(), { existingIds: ['sms-ops'] });
    expect(duplicate.errors.id).toContain('already exists');
    expect(validateGatewayDraft(validDraft({ id: 'Bad Id' })).errors.id).toBeTruthy();
  });

  it('round-trips a connector to a draft and derives the inbound URL', () => {
    const connector = normalizeGatewayConnector(rawGateway())!;
    const draft = gatewayDraftFromConnector(connector);
    expect(draft).toMatchObject({
      id: 'sms-ops',
      url: 'https://gateway.example/openmesh',
      outSecretEnv: 'OPENMESH_SMS_OUT_SECRET',
      inboundEnabled: true,
      inboundChannelId: CHANNEL_ID,
      inboundMaxChars: '400',
    });
    expect(validateGatewayDraft(draft, { editing: true }).errors).toEqual({});
    expect(deriveInboundUrl('http://127.0.0.1:8787', 'sms-ops')).toBe('http://127.0.0.1:8787/api/gateways/sms-ops/inbound');
    expect(deriveInboundUrl('wss://bridge.example:8787/', 'sms ops')).toBe('https://bridge.example:8787/api/gateways/sms%20ops/inbound');
    expect(deriveInboundUrl('bridge.local:8787', 'x')).toBe('http://bridge.local:8787/api/gateways/x/inbound');
  });

  it('normalizes test results and summary text', () => {
    const ok = normalizeGatewayTestResult({ ok: true, status: 204, message: 'accepted' });
    expect(gatewayTestSummary(ok)).toContain('HTTP 204');
    expect(gatewayTestSummary(ok)).toContain('accepted');
    const failed = normalizeGatewayTestResult({ success: false, error: 'adapter refused' });
    expect(failed.ok).toBe(false);
    expect(gatewayTestSummary(failed)).toContain('adapter refused');
    expect(normalizeGatewayTestResult({}).ok).toBe(false);
  });
});

describe('remote MCP guidance', () => {
  it('normalizes a bridge status object and exposes transport/scope labels', () => {
    const status = normalizeMcpStatus(
      { enabled: true, path: '/mcp', allow_write: false, require_confirm: true, rate_limit_per_min: 30, max_sessions: 4, active_sessions: 2, requests_per_min: 12 },
      'bridge',
    )!;
    expect(status).toMatchObject({ enabled: true, allow_write: false, rate_limit_per_min: 30, max_sessions: 4, active_sessions: 2, source: 'bridge' });
    expect(mcpTransportLabel(status)).toContain('HTTP');
    expect(mcpScopeLabel(status)).toBe('read-only');
    expect(mcpScopeLabel({ ...status, allow_write: true })).toBe('read + write');
    expect(normalizeMcpStatus(null)).toBeNull();
  });

  it('falls back to safe file-config defaults and a token-free snippet', () => {
    const fallback = mcpStatusOrDefault(null, 'http://127.0.0.1:8787');
    expect(fallback.enabled).toBe(false);
    expect(fallback.allow_write).toBe(false);
    expect(fallback.endpoint_url).toBe(mcpEndpointFromBridgeUrl('http://127.0.0.1:8787'));
    expect(MCP_CONFIG_DEFAULTS.enabled).toBe(false);
    const snippet = mcpConfigSnippet(fallback);
    const parsed = JSON.parse(snippet);
    expect(parsed).toMatchObject({ enabled: false, path: '/mcp', allow_write: false, require_confirm: true });
    expect(snippet).not.toContain('token');
    expect(mcpClientCommand(fallback.endpoint_url!)).toContain('--token-file');
    expect(MCP_HTTP_WARNINGS.join(' ')).toContain('Loopback only');
    expect(MCP_HTTP_WARNINGS.join(' ')).toContain('read-only');
  });
});

describe('backbone links', () => {
  it('normalizes link lists and stats from array/wrapped responses', () => {
    const links = normalizeBackboneLinksResponse({
      links: [
        { name: 'oslo', mode: 'client', enabled: true, connected: true, last_error: null, stats: { peers: 2, uptime_s: 3600 } },
        { name: 'cabin', enabled: true, connected: false, last_error: 'connection refused' },
        { bad: true },
      ],
    });
    expect(links.map((link) => link.name)).toEqual(['cabin', 'oslo']);
    expect(links[1]!.stats?.peers).toBe(2);
    expect(backboneLegacyHelper()).toBe(true);
  });

  it('validates openmesh invite links and redacts tokens in display copy', () => {
    const valid = validateBackboneLinkDraft({ name: 'oslo', url: 'openmesh://backbone?host=oslo.example&port=9443&token=abc' });
    expect(valid.errors).toEqual({});
    expect(valid.input).toMatchObject({ name: 'oslo', enabled: true });
    expect(validateBackboneLinkDraft({ name: 'bad name', url: 'https://example.com' }).errors.name).toBeTruthy();
    expect(validateBackboneLinkDraft({ name: 'ok', url: 'https://example.com' }).errors.url).toBeTruthy();
    expect(validateBackboneLinkDraft({ name: 'oslo', url: 'openmesh://backbone?host=x' }).errors.url).toContain('host and port');
    expect(validateBackboneLinkDraft({ name: 'oslo', url: 'openmesh://backbone?host=x&port=1' }, { existingNames: ['oslo'] }).errors.name).toContain('already');
    expect(redactBackboneLinkUrl('openmesh://backbone?host=x&port=1&token=secret-token')).not.toContain('secret-token');
    expect(redactBackboneLinkUrl(null)).toBe('');
    expect(BACKBONE_LINK_DEDUP_NOTE).toContain('dedup');
  });

  it('reports legacy single-link fallback from the backbone config', () => {
    const config = { enabled: true } as BackboneConfig;
    expect(hasLegacyBackboneLink(config)).toBe(true);
    expect(hasLegacyBackboneLink({ enabled: false } as BackboneConfig)).toBe(false);
    expect(hasLegacyBackboneLink(null)).toBe(false);
  });
});

describe('demo seeds', () => {
  it('seeds two connectors (one failing), two links and read-only remote MCP', () => {
    const dataset = createDemoDataset(new Date(2026, 2, 15, 12, 0, 0).getTime());
    expect(dataset.gateways).toHaveLength(2);
    expect(dataset.gateways.some((connector) => connector.status.last_error)).toBe(true);
    expect(dataset.gateways.filter((connector) => connector.inbound.enabled)).toHaveLength(1);
    expect(dataset.gateways.every((connector) => !('secret' in connector.outbound))).toBe(true);
    expect(dataset.backboneLinks).toHaveLength(2);
    expect(dataset.backboneLinks.some((link) => !link.connected && link.last_error)).toBe(true);
    expect(dataset.mcp).toMatchObject({ enabled: true, allow_write: false, require_confirm: true, source: 'demo' });
    const gateways = normalizeGatewaysResponse({ connectors: dataset.gateways });
    expect(gateways).toHaveLength(2);
    expect(normalizeBackboneLinksResponse(dataset.backboneLinks)).toHaveLength(2);
    expect(channelIdPattern().test(dataset.gateways[0]!.inbound.channel_id ?? '')).toBe(true);
    expect(envNamePattern().test(dataset.gateways[0]!.outbound.secret_env ?? '')).toBe(true);
    const serialized = JSON.stringify({ gateways: dataset.gateways, links: dataset.backboneLinks });
    expect(serialized).not.toContain('secret-token"');
  });
});

describe('ops REST client', () => {
  it('GETs gateways with bearer attribution headers', async () => {
    const calls: Array<{ url: string; init?: RequestInit }> = [];
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ url: String(input), init });
      return new Response(JSON.stringify({ connectors: [rawGateway()] }), { status: 200, headers: { 'Content-Type': 'application/json' } });
    }) as typeof fetch;
    const client = new OpsRestClient({ baseUrl: 'http://127.0.0.1:8787', token: 'test-token', fetchImpl });
    const gateways = await client.listGateways();
    expect(gateways[0]!.id).toBe('sms-ops');
    expect(calls[0]!.url).toBe('http://127.0.0.1:8787/api/gateways');
    const headers = calls[0]!.init?.headers as Record<string, string>;
    expect(headers.Authorization).toBe('Bearer test-token');
    expect(headers['X-OpenMesh-Client']).toBe('web');
  });

  it('PUTs only env-var names and no inline secret fields', async () => {
    const calls: Array<{ url: string; init?: RequestInit }> = [];
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ url: String(input), init });
      return new Response(JSON.stringify({ connector: rawGateway() }), { status: 200, headers: { 'Content-Type': 'application/json' } });
    }) as typeof fetch;
    const client = new OpsRestClient({ baseUrl: 'http://127.0.0.1:8787', fetchImpl });
    const validation = validateGatewayDraft(validDraft());
    const connector = await client.putGateway(validation.config!);
    expect(connector.id).toBe('sms-ops');
    expect(calls[0]!.url).toBe('http://127.0.0.1:8787/api/gateways/sms-ops');
    expect(calls[0]!.init?.method).toBe('PUT');
    const body = JSON.parse(String(calls[0]!.init?.body));
    expect(body.outbound.secret_env).toBe('OPENMESH_SMS_OUT_SECRET');
    expect(body.outbound).not.toHaveProperty('secret');
    expect(body.inbound.secret_env).toBe('OPENMESH_SMS_IN_SECRET');
    expect(JSON.stringify(body)).not.toContain('super-secret');
  });

  it('POSTs /test and normalizes its status payload', async () => {
    const calls: Array<{ url: string; init?: RequestInit }> = [];
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ url: String(input), init });
      return new Response(JSON.stringify({ ok: false, status: 502, message: 'upstream refused' }), { status: 200, headers: { 'Content-Type': 'application/json' } });
    }) as typeof fetch;
    const client = new OpsRestClient({ baseUrl: 'http://127.0.0.1:8787', fetchImpl });
    const result = await client.testGateway('sms-ops');
    expect(result).toMatchObject({ ok: false, status: 502, message: 'upstream refused' });
    expect(calls[0]!.url).toBe('http://127.0.0.1:8787/api/gateways/sms-ops/test');
    expect(calls[0]!.init?.method).toBe('POST');
  });

  it('surfaces 404 as OpsApiError for older-bridge fallbacks', async () => {
    const fetchImpl = (async () => new Response(JSON.stringify({ error: 'not found' }), { status: 404, headers: { 'Content-Type': 'application/json' } })) as typeof fetch;
    const client = new OpsRestClient({ baseUrl: 'http://127.0.0.1:8787', fetchImpl });
    const error = await client.listGateways().catch((cause: unknown) => cause);
    expect(error).toBeInstanceOf(OpsApiError);
    expect((error as OpsApiError).status).toBe(404);
    expect((error as OpsApiError).code).toBe('not found');
  });

  it('lists, adds and removes backbone links through the contract routes', async () => {
    const calls: Array<{ url: string; init?: RequestInit }> = [];
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ url: String(input), init });
      if (String(input).endsWith('/api/backbone/links') && (!init || init.method !== 'PUT')) {
        return new Response(JSON.stringify({ links: [{ name: 'oslo', enabled: true, connected: false }] }), { status: 200 });
      }
      return new Response(JSON.stringify({ link: { name: 'oslo', enabled: true, connected: false } }), { status: 200 });
    }) as typeof fetch;
    const client = new OpsRestClient({ baseUrl: 'wss://bridge.example:8787/', fetchImpl });
    const links = await client.listBackboneLinks();
    expect(links).toHaveLength(1);
    expect(calls[0]!.url).toBe('https://bridge.example:8787/api/backbone/links');
    await client.putBackboneLink({ name: 'oslo', url: 'openmesh://backbone?host=x&port=1', enabled: true });
    expect(calls[1]!.init?.method).toBe('PUT');
    expect(calls[1]!.url).toBe('https://bridge.example:8787/api/backbone/links/oslo');
    expect(JSON.parse(String(calls[1]!.init?.body))).toMatchObject({ enabled: true });
    await client.deleteBackboneLink('oslo');
    expect(calls[2]!.init?.method).toBe('DELETE');
  });
});

function backboneLegacyHelper(): boolean {
  return hasLegacyBackboneLink({ enabled: true } as BackboneConfig);
}

// Keep the helper used in a type-safe way.
const _typecheckGateway: GatewayConnector | null = null;
void _typecheckGateway;
