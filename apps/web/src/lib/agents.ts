/** AI-agent helpers: MCP config snippets, prompt packs, activity rendering (AI-NATIVE.md). */
import type {
  ActivityEntry,
  ActivityPayloadMeta,
  AgentClient,
  AgentStatus,
  McpConfigOptions,
  McpSnippet,
  PromptTier,
} from '../types/agents';

export const ACTIVITY_LIMIT = 100;

export const AGENT_CLIENTS: Array<{ id: AgentClient; label: string; description: string }> = [
  { id: 'claude', label: 'Claude', description: 'Claude Desktop and Claude Code' },
  { id: 'cursor', label: 'Cursor', description: 'Cursor’s MCP servers list' },
  { id: 'dsh', label: 'DSH', description: 'DeepSeek Harness agents' },
  { id: 'generic', label: 'Generic', description: 'Any MCP client with stdio servers' },
];

export function mcpServerTarget(options: McpConfigOptions): { command: string; args: string[]; env: Record<string, string>; type: 'stdio' } {
  return {
    type: 'stdio',
    command: 'openmesh-bridge',
    args: ['mcp'],
    env: {
      OPENMESH_API: options.bridgeUrl || 'http://127.0.0.1:8787',
      // The server reads the token itself; the config never contains the secret.
      OPENMESH_TOKEN_FILE: options.tokenFile ?? '~/.config/openmesh/bridge-token',
      OPENMESH_MCP_READ_ONLY: options.readOnly === false ? '0' : '1',
      OPENMESH_AGENT_NAME: options.agentName ?? 'web-agent',
    },
  };
}

export function buildMcpConfigSnippet(
  client: AgentClient,
  options: McpConfigOptions,
): McpSnippet {
  const target = mcpServerTarget(options);
  const claudeLike = { mcpServers: { openmesh: target } };
  const content =
    client === 'generic'
      ? JSON.stringify(target, null, 2)
      : JSON.stringify(claudeLike, null, 2);
  const meta = AGENT_CLIENTS.find((entry) => entry.id === client) ?? AGENT_CLIENTS[3]!;
  const notes: Record<AgentClient, string> = {
    claude: 'Paste into claude_desktop_config.json (Desktop) or .mcp.json (Claude Code), then restart the client. The token is read from the file, not the config.',
    cursor: 'Paste into ~/.cursor/mcp.json (or the project .cursor/mcp.json) and enable the server. The token is read from the file.',
    dsh: 'Paste into your DSH MCP configuration. `openmesh-cli agent config --client dsh` prints the same block.',
    generic: 'This is the server definition for a generic MCP client: a stdio command plus environment variables. The token stays in the file.',
  };
  return {
    client,
    label: meta.label,
    description: meta.description,
    content,
    note: notes[client],
  };
}

export function buildAllMcpSnippets(options: McpConfigOptions): McpSnippet[] {
  return AGENT_CLIENTS.map((client) => buildMcpConfigSnippet(client.id, options));
}

export const AGENT_SAFETY_RULES = [
  'Ask the human before any write action that changes the mesh, deletes data or rotates a token.',
  'Never print, paste or commit the bridge API token, channel keys or identity seeds.',
  'Start in read-only mode (OPENMESH_MCP_READ_ONLY=1) and only leave it when asked.',
  'Before sending a lot of media, check the airtime budget and warn about the cost.',
  'Report errors in plain language and link the matching help topic instead of guessing.',
];

export const READ_ONLY_EXPLAINER = {
  title: 'Read-only mode',
  body:
    'With read-only mode on, the MCP server exposes only tools that look at your node. Write tools are hidden and any write call is refused. It is the recommended way to try a new agent.',
  env: 'OPENMESH_MCP_READ_ONLY=1',
  note: 'This is a client-side environment setting. Turning it off is a deliberate, visible change — never do it silently.',
};

export const AGENT_SECURITY_NOTES = [
  'The agent talks to the local bridge API; it never receives your private identity keys.',
  'MCP reads the bridge token from a file, so the token never appears in a prompt or config snippet.',
  'Every write action appears in the activity log below with its source and result.',
  'Agents can act on anything the API allows. Read-only first, then narrow permissions on purpose.',
  'To revoke access, rotate the bridge token file and restart the bridge — the panel cannot rotate it through the API in v0.3.0.',
  'Help content and prompts live in the repo, so the app, docs and agents cannot drift apart.',
];

/** Steps shown by the panel's revoke flow (no fabricated CLI flags; see docs/INSTALL.md). */
export const API_TOKEN_ROTATION_STEPS = [
  'Stop the bridge using the same method you start it (systemd service, container or terminal).',
  'Generate a new random token with your password manager or `openssl rand -hex 32`.',
  'Replace the contents of the bridge token file (default `~/.config/openmesh/bridge-token`) with the new token.',
  'Start the bridge again, then paste the new token into Settings → Bridge token on each device that should keep access.',
  'Agents and other clients keep failing until their environment points at the new token file — this is the revocation.',
];

export const API_TOKEN_ROTATION_HELP_TOPIC = 'security';

const READ_TOOLS = [
  'openmesh_health',
  'openmesh_status',
  'openmesh_nodes',
  'openmesh_messages',
  'openmesh_channels',
  'openmesh_contacts',
  'openmesh_backbone_status',
  'openmesh_backbone_peers',
  'openmesh_setup_state',
  'openmesh_activity',
  'openmesh_help',
];

const WRITE_TOOLS = [
  'openmesh_send_message',
  'openmesh_send_channel_message',
  'openmesh_share_position',
  'openmesh_announce',
  'openmesh_set_config',
  'openmesh_setup_quick',
  'openmesh_create_channel',
  'openmesh_join_channel',
  'openmesh_add_contact',
  'openmesh_backbone_config',
  'openmesh_rotate_backbone_token ⚠',
  'openmesh_kick_backbone_peer ⚠',
];

export interface AgentPromptContext {
  tier: PromptTier;
  nodeName: string;
  nodeId?: string | null;
  bridgeUrl: string;
  readOnly?: boolean;
  region?: string | null;
  profile?: string | null;
  role?: string | null;
  helpTopics?: string[];
}

/** Bundled system prompt pack; mirrors `openmesh-cli agent prompt`. */
export function buildAgentPrompt(context: AgentPromptContext): string {
  const tier = context.tier;
  const beginner = tier === 'beginner';
  const stateLines = [
    `- Node name: ${context.nodeName || 'unnamed node'}`,
    context.nodeId ? `- Node id: ${context.nodeId}` : null,
    context.role ? `- Role: ${context.role}` : null,
    context.region ? `- Radio region: ${context.region}` : null,
    context.profile ? `- Radio profile: ${context.profile}` : null,
    `- Bridge API: ${context.bridgeUrl}`,
    `- Mode: ${context.readOnly === false ? 'read and write (you must still ask before destructive actions)' : 'read-only'}`,
    '- MCP token source: the bridge token file (never paste the token into this chat)',
  ].filter((line): line is string => Boolean(line));

  const lines: string[] = [
    `# OpenMesh assistant — ${beginner ? 'beginner' : 'advanced'} tier`,
    '',
    'You are an assistant for a local LoRa mesh node, connected through the OpenMesh MCP server.',
    'The human in front of you owns this node. Your job is to explain what is happening and to help safely — not to show off.',
    '',
    '## Current node',
    ...stateLines,
    '',
    '## Safety rules (non-negotiable)',
    ...AGENT_SAFETY_RULES.map((rule, index) => `${index + 1}. ${rule}`),
    '',
    '## How to work',
    beginner
      ? [
          '1. Call `openmesh_status` first and explain the result in everyday words.',
          '2. Ask one question at a time when you need information.',
          '3. Prefer the guided `openmesh_setup_quick` preset over manual radio settings.',
          '4. When the human asks “why”, point them to the matching help topic: `openmesh_help` with a topic such as getting-started, messaging or security.',
          '5. Say clearly when something costs battery, radio time (airtime) or privacy.',
        ].join('\n')
      : [
          '1. Call `openmesh_status` first; include exact values (region, profile, airtime %, queues).',
          '2. Read the relevant help topic via `openmesh_help` before changing radio parameters.',
          '3. Batch read calls, then propose a minimal patch; destructive calls need explicit confirmation.',
          '4. Quote exact numbers and units. If a value is missing, say “not reported” instead of guessing.',
          '5. Record what you changed in your reply so the human can verify it in the activity log.',
        ].join('\n'),
    '',
    '## Available tools',
    'Read tools (always available):',
    ...READ_TOOLS.map((tool) => `- ${tool}`),
    '',
    context.readOnly === false ? 'Write tools (enabled):' : 'Write tools (hidden while read-only mode is on):',
    ...WRITE_TOOLS.map((tool) => `- ${tool}`),
    '',
    '## Help topics',
    ...(context.helpTopics?.length
      ? context.helpTopics.map((topic) => `- ${topic}`)
      : ['- getting-started', '- messaging', '- security', '- troubleshooting']),
    '',
    '## MCP configuration',
    '```json',
    buildMcpConfigSnippet('generic', {
      bridgeUrl: context.bridgeUrl,
      readOnly: context.readOnly !== false,
      agentName: 'openmesh-assistant',
    }).content,
    '```',
    '',
    beginner
      ? 'Reply in short paragraphs. Avoid radio jargon; if you must use a term, explain it in one sentence.'
      : 'You may use protocol terms (SF, TTL, MTU, OBB/1) but always attach the exact value and unit.',
    'If you cannot do something, explain the next small step and link a help topic instead of a vague error.',
  ];
  return lines.join('\n');
}

export function activitySourceMeta(source: string): {
  label: string;
  kind: 'agent' | 'user' | 'web' | 'cli' | 'other';
  agentName: string | null;
} {
  const value = source.trim();
  if (value.startsWith('agent:')) {
    const name = value.slice(6).trim() || 'agent';
    return { label: `Agent · ${name}`, kind: 'agent', agentName: name };
  }
  if (value === 'user' || value === 'human') return { label: 'You', kind: 'user', agentName: null };
  if (value === 'web') return { label: 'Web panel', kind: 'web', agentName: null };
  if (value === 'cli') return { label: 'CLI', kind: 'cli', agentName: null };
  if (!value) return { label: 'Unknown', kind: 'other', agentName: null };
  return { label: value, kind: 'other', agentName: null };
}

function normalizeActivityEntry(raw: unknown, index: number): ActivityEntry | null {
  if (!raw || typeof raw !== 'object') return null;
  const record = raw as Record<string, unknown>;
  const atMs = typeof record.at_ms === 'number' ? record.at_ms : typeof record.at === 'number' ? record.at : NaN;
  if (!Number.isFinite(atMs)) return null;
  const source = typeof record.source === 'string' && record.source ? record.source : 'user';
  const ok = typeof record.ok === 'boolean' ? record.ok : record.error == null;
  return {
    id: typeof record.id === 'number' || typeof record.id === 'string' ? record.id : index,
    at_ms: atMs,
    source,
    method: typeof record.method === 'string' ? record.method : '—',
    path: typeof record.path === 'string' ? record.path : '',
    summary: typeof record.summary === 'string' ? record.summary : `${record.method ?? 'change'} ${record.path ?? ''}`.trim(),
    ok,
    error: typeof record.error === 'string' ? record.error : null,
  };
}

/** Accepts a bare array or the wrappers bridges use: {activity}/{entries}/{items}. */
export function normalizeActivityResponse(raw: unknown): ActivityEntry[] {
  const list = Array.isArray(raw)
    ? raw
    : raw && typeof raw === 'object'
      ? ((raw as Record<string, unknown>).activity ??
        (raw as Record<string, unknown>).entries ??
        (raw as Record<string, unknown>).items ??
        (raw as Record<string, unknown>).log)
      : null;
  if (!Array.isArray(list)) return [];
  return list
    .map((entry, index) => normalizeActivityEntry(entry, index))
    .filter((entry): entry is ActivityEntry => Boolean(entry))
    .sort((left, right) => right.at_ms - left.at_ms);
}

export interface NormalizedActivityPayload extends ActivityPayloadMeta {
  entries: ActivityEntry[];
}

/** Full GET /api/activity payload ({entries, capacity, agent_connected, last_agent_at_ms, sources}). */
export function normalizeActivityPayload(raw: unknown): NormalizedActivityPayload {
  const record = raw && typeof raw === 'object' && !Array.isArray(raw) ? (raw as Record<string, unknown>) : {};
  const entries = normalizeActivityResponse(raw);
  const latestAgent = entries.find((entry) => activitySourceMeta(entry.source).kind === 'agent') ?? null;
  const lastAgentAt = typeof record.last_agent_at_ms === 'number' ? record.last_agent_at_ms : latestAgent?.at_ms ?? null;
  return {
    entries,
    capacity: typeof record.capacity === 'number' ? record.capacity : 256,
    agentConnected: record.agent_connected === true || entries.some((entry) => activitySourceMeta(entry.source).kind === 'agent'),
    lastAgentAtMs: lastAgentAt,
    sources: Array.isArray(record.sources)
      ? record.sources.filter((source): source is string => typeof source === 'string')
      : [...new Set(entries.map((entry) => entry.source))],
  };
}

export function agentStatusFromActivity(entries: ActivityEntry[], configuredHint = false): AgentStatus {
  const agentEntries = entries.filter((entry) => activitySourceMeta(entry.source).kind === 'agent');
  const latest = agentEntries[0] ?? null;
  const name = latest ? activitySourceMeta(latest.source).agentName : null;
  return {
    configured: configuredHint || agentEntries.length > 0,
    name,
    lastActionMs: latest ? latest.at_ms : null,
    readOnly: true,
  };
}

export function formatActivityTime(atMs: number, nowMs = Date.now()): string {
  const seconds = Math.max(0, Math.round((nowMs - atMs) / 1000));
  if (seconds < 45) return 'just now';
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.round(hours / 24);
  if (days < 7) return `${days} d ago`;
  return new Date(atMs).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

export function formatActivityMethod(method: string): string {
  return method.toUpperCase();
}

export function activitySummaryLine(entry: ActivityEntry): string {
  return entry.summary || `${entry.method} ${entry.path}`;
}

export function activityEmptyState(): { title: string; body: string; helpTopic: string } {
  return {
    title: 'No changes yet',
    body: 'Reads are not recorded. When you or an agent change something, it appears here with the source and result.',
    helpTopic: 'agents',
  };
}

export function agentPromptTiers(): Array<{ id: PromptTier; label: string; description: string }> {
  return [
    { id: 'beginner', label: 'Beginner prompt', description: 'Plain-language assistant that explains jargon and asks one question at a time.' },
    { id: 'advanced', label: 'Advanced prompt', description: 'Protocol-aware operator that quotes exact values and minimal patches.' },
  ];
}
