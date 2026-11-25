import { describe, expect, it } from 'vitest';
import {
  ACTIVITY_LIMIT,
  AGENT_CLIENTS,
  AGENT_SAFETY_RULES,
  AGENT_SECURITY_NOTES,
  API_TOKEN_ROTATION_STEPS,
  READ_ONLY_EXPLAINER,
  activityEmptyState,
  activitySourceMeta,
  agentPromptTiers,
  agentStatusFromActivity,
  buildAgentPrompt,
  buildAllMcpSnippets,
  buildMcpConfigSnippet,
  formatActivityTime,
  mcpServerTarget,
  normalizeActivityPayload,
  normalizeActivityResponse,
} from '../agents';
import { bannedTermsInCopy } from '../mode';

const BRIDGE = 'http://127.0.0.1:8787';
const now = 1_760_000_000_000;

function entry(overrides: Record<string, unknown> = {}) {
  return {
    id: 12,
    at_ms: now - 60_000,
    source: 'agent:claude',
    method: 'POST',
    path: '/api/messages',
    summary: 'send message to 6b1f… (14 bytes)',
    ok: true,
    error: null,
    ...overrides,
  };
}

describe('MCP config snippets', () => {
  it('builds valid JSON for all four clients', () => {
    expect(AGENT_CLIENTS.map((client) => client.id)).toEqual(['claude', 'cursor', 'dsh', 'generic']);
    const snippets = buildAllMcpSnippets({ bridgeUrl: BRIDGE, readOnly: true, agentName: 'claude' });
    expect(snippets).toHaveLength(4);
    for (const snippet of snippets) {
      const parsed = JSON.parse(snippet.content) as Record<string, unknown>;
      expect(parsed, snippet.client).toBeTypeOf('object');
      expect(snippet.note.length).toBeGreaterThan(20);
    }
    const claude = JSON.parse(snippets[0]!.content) as { mcpServers: { openmesh: { command: string; args: string[]; env: Record<string, string> } } };
    expect(claude.mcpServers.openmesh.command).toBe('openmesh-bridge');
    expect(claude.mcpServers.openmesh.args).toEqual(['mcp']);
    expect(claude.mcpServers.openmesh.env.OPENMESH_API).toBe(BRIDGE);
    expect(claude.mcpServers.openmesh.env.OPENMESH_TOKEN_FILE).toContain('bridge-token');
    expect(claude.mcpServers.openmesh.env.OPENMESH_MCP_READ_ONLY).toBe('1');
    expect(JSON.stringify(claude)).not.toContain('OPENMESH_TOKEN"');
    const generic = JSON.parse(snippets[3]!.content) as Record<string, unknown>;
    expect(generic.type).toBe('stdio');
  });

  it('flips read-only off only when explicitly asked and never embeds a token value', () => {
    expect(mcpServerTarget({ bridgeUrl: BRIDGE, readOnly: false }).env.OPENMESH_MCP_READ_ONLY).toBe('0');
    expect(mcpServerTarget({ bridgeUrl: BRIDGE }).env.OPENMESH_MCP_READ_ONLY).toBe('1');
    const snippet = buildMcpConfigSnippet('cursor', { bridgeUrl: BRIDGE, readOnly: true });
    expect(snippet.content).not.toMatch(/Bearer|secret|psk/i);
  });
});

describe('agent prompts', () => {
  const base = {
    nodeName: 'oslo-node',
    nodeId: 'a1b2c3d4e5f60718',
    bridgeUrl: BRIDGE,
    readOnly: true,
    region: 'EU868',
    profile: 'MEDIUM_FAST',
    role: 'client',
  };

  it('includes node state, safety rules and MCP config', () => {
    const prompt = buildAgentPrompt({ ...base, tier: 'advanced' });
    expect(prompt).toContain('oslo-node');
    expect(prompt).toContain('a1b2c3d4e5f60718');
    expect(prompt).toContain('EU868');
    expect(prompt).toContain('MEDIUM_FAST');
    expect(prompt).toContain('read-only');
    expect(prompt).toContain('OPENMESH_TOKEN_FILE');
    for (const rule of AGENT_SAFETY_RULES) expect(prompt).toContain(rule);
    expect(AGENT_SAFETY_RULES.join(' ')).toContain('before any write action');
  });

  it('keeps beginner copy free of unexplained protocol jargon while advanced includes exact values', () => {
    const beginner = buildAgentPrompt({ ...base, tier: 'beginner' });
    const advanced = buildAgentPrompt({ ...base, tier: 'advanced' });
    expect(bannedTermsInCopy(beginner.split('## MCP configuration')[0]!)).toEqual([]);
    expect(beginner).toContain('everyday words');
    expect(advanced).toContain('exact values');
    expect(advanced).toMatch(/SF|TTL|MTU|OBB/);
    expect(agentPromptTiers().map((tier) => tier.id)).toEqual(['beginner', 'advanced']);
  });

  it('never embeds a bearer token', () => {
    for (const tier of ['beginner', 'advanced'] as const) {
      const prompt = buildAgentPrompt({ ...base, tier });
      expect(prompt).not.toMatch(/Bearer\s+[A-Za-z0-9_-]{16,}/);
      expect(prompt).toContain('token file');
    }
  });
});

describe('activity log', () => {
  it('normalises arrays and wrapper payloads, newest first', () => {
    const older = entry({ id: 1, at_ms: now - 10 * 60_000 });
    const newer = entry({ id: 2, at_ms: now - 1_000 });
    expect(normalizeActivityResponse([older, newer]).map((item) => item.id)).toEqual([2, 1]);
    expect(normalizeActivityResponse({ entries: [older, newer] })).toHaveLength(2);
    expect(normalizeActivityResponse({ activity: [older] })).toHaveLength(1);
    expect(normalizeActivityResponse({ items: [older] })).toHaveLength(1);
    expect(normalizeActivityResponse('nope')).toEqual([]);
    expect(normalizeActivityResponse([null, { at_ms: 'bad' }])).toEqual([]);
  });

  it('normalises the full metadata payload', () => {
    const payload = normalizeActivityPayload({
      entries: [entry(), entry({ id: 2, source: 'user', ok: false, error: 'radio busy', at_ms: now - 2_000 })],
      capacity: 256,
      agent_connected: true,
      last_agent_at_ms: now - 60_000,
      sources: ['agent:claude', 'user'],
    });
    expect(payload.capacity).toBe(256);
    expect(payload.agentConnected).toBe(true);
    expect(payload.lastAgentAtMs).toBe(now - 60_000);
    expect(payload.sources).toEqual(['agent:claude', 'user']);
    expect(payload.entries).toHaveLength(2);
    expect(payload.entries[0]?.source).toBe('user');
    expect(payload.entries[0]?.ok).toBe(false);
    expect(payload.entries[0]?.error).toBe('radio busy');
  });

  it('labels sources for the activity badges', () => {
    expect(activitySourceMeta('agent:claude')).toEqual({ label: 'Agent · claude', kind: 'agent', agentName: 'claude' });
    expect(activitySourceMeta('user').kind).toBe('user');
    expect(activitySourceMeta('web').label).toBe('Web panel');
    expect(activitySourceMeta('cli').kind).toBe('cli');
    expect(activitySourceMeta('mystery').kind).toBe('other');
    expect(activitySourceMeta('agent:').agentName).toBe('agent');
  });

  it('derives the connected-agent status and formats times', () => {
    const status = agentStatusFromActivity(normalizeActivityResponse([entry(), entry({ id: 2, source: 'web' })]));
    expect(status).toMatchObject({ configured: true, name: 'claude', readOnly: true });
    expect(agentStatusFromActivity([]).configured).toBe(false);
    expect(formatActivityTime(now - 30_000, now)).toBe('just now');
    expect(formatActivityTime(now - 5 * 60_000, now)).toBe('5 min ago');
    expect(formatActivityTime(now - 3 * 60 * 60_000, now)).toBe('3 h ago');
    expect(formatActivityTime(now - 2 * 24 * 60 * 60_000, now)).toBe('2 d ago');
  });

  it('keeps the empty state and rotation guidance honest', () => {
    expect(ACTIVITY_LIMIT).toBe(100);
    const empty = activityEmptyState();
    expect(empty.title).toBe('No changes yet');
    expect(empty.helpTopic).toBe('agents');
    expect(API_TOKEN_ROTATION_STEPS).toHaveLength(5);
    expect(API_TOKEN_ROTATION_STEPS.join(' ')).toContain('bridge-token');
    expect(API_TOKEN_ROTATION_STEPS.join(' ').toLowerCase()).toContain('start the bridge again');
    expect(API_TOKEN_ROTATION_STEPS.join(' ')).not.toContain('token/rotate');
    expect(AGENT_SECURITY_NOTES.join(' ')).toContain('rotate the bridge token file');
    expect(READ_ONLY_EXPLAINER.env).toBe('OPENMESH_MCP_READ_ONLY=1');
  });
});
