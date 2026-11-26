/**
 * Remote-operations helpers (gateways / remote MCP / multi-backbone links).
 *
 * Normative: docs/design/remote-operations.md §1–§3. Everything here is pure and
 * defensive: older bridges may return 404 or omit the new fields, and the UI must
 * keep rendering guidance instead of failing.
 */
import { buildHttpBaseUrl } from './ws';
import type { BackboneConfig } from '../types/backbone';
import type {
  BackboneLink,
  BackboneLinkInput,
  GatewayConfigInput,
  GatewayConnector,
  GatewayOutboundConfig,
  GatewayRuntimeStatus,
  GatewayTestResult,
  McpHttpStatus,
} from '../types/ops';

export const GATEWAYS_HELP_TOPIC = 'gateways-and-remote-agents';
export const REMOTE_OPERATIONS_HELP_TOPIC = 'gateways-and-remote-agents';
export const MCP_DOCS_PATH = 'docs/agents/mcp.md';

export const GATEWAY_ADAPTERS = [
  { id: 'matrix', label: 'Matrix bot', hint: 'Bridge a Matrix room to one OpenMesh channel with a small webhook bot.' },
  { id: 'sms', label: 'SMS provider', hint: 'Forward channel traffic to an SMS gateway webhook (Twilio, MessageBird, local provider).' },
  { id: 'satellite', label: 'Satellite messenger', hint: 'Garmin inReach/Zoleo-style satellite services that accept HTTPS webhooks.' },
  { id: 'email', label: 'Email forwarder', hint: 'Forward to an email-to-webhook relay, or inject email text back into a channel.' },
  { id: 'ntfy', label: 'ntfy / push', hint: 'Send a notification to a phone or desktop through ntfy or your own push service.' },
] as const;

export const BACKBONE_LINK_DEDUP_NOTE =
  'One reconnect loop per enabled link. The engine’s (src,seq) dedup suppresses duplicate frames across links, so redundancy is safe — but one slow link can still add latency to announcements.';

export const MCP_HTTP_WARNINGS = [
  'Loopback only by default. If you change the bind address, protect it with WireGuard/Tailscale or an authenticated tunnel — never expose the bridge directly to the internet.',
  'The bearer token grants full local API access to anyone who has it. Treat it like the node’s control key and use read-only mode unless writes are required.',
  'Read-only is recommended for remote agents; enable write access only for a trusted operator and keep confirm:true.',
  'The HTTP MCP transport is opt-in. stdio remains the default, fully local path.',
] as const;

export const MCP_CONFIG_DEFAULTS = {
  enabled: false,
  path: '/mcp',
  allow_write: false,
  require_confirm: true,
  rate_limit_per_min: 60,
  max_sessions: 8,
} as const;

export function gatewayIdPatternForUi(): RegExp {
  return /^[a-z0-9][a-z0-9_-]{0,63}$/;
}

export function envNamePattern(): RegExp {
  return /^[A-Z][A-Z0-9_]{0,63}$/;
}

export function channelIdPattern(): RegExp {
  return /^[0-9a-f]{16}$/;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : null;
}

function stringOrNull(value: unknown): string | null {
  return typeof value === 'string' && value.length > 0 ? value : null;
}

function numberOr(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
}

function numberOrNull(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

function boolOr(value: unknown, fallback: boolean): boolean {
  return typeof value === 'boolean' ? value : fallback;
}

/** Never keep anything beyond an env-var name in secret fields. */
export function sanitizeSecretEnvName(value: unknown): string | null {
  const raw = stringOrNull(value)?.trim() ?? null;
  if (!raw) return null;
  // The API only returns names; accept only the conventional env-name shape and
  // drop anything that looks like an inline secret value.
  if (!envNamePattern().test(raw)) return null;
  return raw;
}

function parseChannelIds(value: unknown): string[] {
  if (Array.isArray(value)) {
    return [...new Set(value.filter((item): item is string => typeof item === 'string' && channelIdPattern().test(item)))];
  }
  if (typeof value === 'string') {
    return [...new Set(value.split(/[\s,;]+/).filter((item) => channelIdPattern().test(item)))];
  }
  return [];
}

function normalizeRuntimeStatus(raw: unknown, connectorRaw: Record<string, unknown> | null): GatewayRuntimeStatus {
  const status = asRecord(raw) ?? {};
  const secrets = asRecord(status.secrets) ?? asRecord(connectorRaw?.secrets) ?? {};
  const outboundRaw = asRecord(connectorRaw?.outbound) ?? {};
  const inboundRaw = asRecord(connectorRaw?.inbound) ?? {};
  const outEnvHint = Boolean(sanitizeSecretEnvName(outboundRaw.secret_env));
  const inEnvHint = Boolean(sanitizeSecretEnvName(inboundRaw.secret_env));
  const pick = (...keys: string[]): unknown => {
    for (const key of keys) {
      if (status[key] !== undefined) return status[key];
      if (connectorRaw && connectorRaw[key] !== undefined) return connectorRaw[key];
    }
    return undefined;
  };
  return {
    last_success_ms: numberOrNull(pick('last_success_ms', 'last_success_at_ms', 'last_success')),
    last_failure_ms: numberOrNull(pick('last_failure_ms', 'last_failure_at_ms', 'last_failure')),
    last_error: stringOrNull(pick('last_error', 'error')),
    retries: Math.max(0, Math.round(numberOr(pick('retries', 'retry_count', 'consecutive_failures'), 0))),
    out_secret_present: boolOr(
      pick('out_secret_present', 'outbound_secret_present', 'secret_out_present'),
      boolOr(secrets.outbound, outEnvHint),
    ),
    in_secret_present: boolOr(
      pick('in_secret_present', 'inbound_secret_present', 'secret_in_present'),
      boolOr(secrets.inbound, inEnvHint),
    ),
  };
}

export function normalizeGatewayConnector(raw: unknown): GatewayConnector | null {
  const record = asRecord(raw);
  if (!record) return null;
  const id = stringOrNull(record.id)?.trim();
  if (!id || !gatewayIdPatternForUi().test(id)) return null;
  const outboundRaw = asRecord(record.outbound) ?? {};
  const inboundRaw = asRecord(record.inbound) ?? {};
  const enabledRaw = record.enabled;
  const outbound: GatewayOutboundConfig = {
    url: stringOrNull(outboundRaw.url) ?? '',
    secret_env: sanitizeSecretEnvName(outboundRaw.secret_env),
    channel_ids: parseChannelIds(outboundRaw.channel_ids),
    include_direct: boolOr(outboundRaw.include_direct, false),
    include_attachments: boolOr(outboundRaw.include_attachments, false),
    timeout_ms: Math.max(1000, Math.round(numberOr(outboundRaw.timeout_ms, 10_000))),
    max_retries: Math.max(0, Math.round(numberOr(outboundRaw.max_retries, 3))),
  };
  const inbound = {
    enabled: boolOr(inboundRaw.enabled, false),
    secret_env: sanitizeSecretEnvName(inboundRaw.secret_env),
    channel_id: stringOrNull(inboundRaw.channel_id),
    max_chars: Math.max(1, Math.round(numberOr(inboundRaw.max_chars, 400))),
  };
  return {
    id,
    kind: stringOrNull(record.kind) ?? 'webhook',
    enabled: boolOr(enabledRaw, true),
    outbound,
    inbound,
    status: normalizeRuntimeStatus(record.status, record),
  };
}

/** GET /api/gateways may return an array or `{connectors:[…]}`. */
export function normalizeGatewaysResponse(payload: unknown): GatewayConnector[] {
  const record = asRecord(payload);
  const rawList = Array.isArray(payload)
    ? payload
    : Array.isArray(record?.connectors)
      ? (record!.connectors as unknown[])
      : [];
  return rawList
    .map(normalizeGatewayConnector)
    .filter((item): item is GatewayConnector => item !== null)
    .sort((left, right) => left.id.localeCompare(right.id));
}

export function gatewayStatusView(connector: GatewayConnector): {
  tone: 'accent' | 'amber' | 'rose' | 'slate';
  label: string;
  failing: boolean;
  lastActivityMs: number | null;
} {
  if (!connector.enabled) return { tone: 'slate', label: 'disabled', failing: false, lastActivityMs: connector.status.last_success_ms };
  if (!connector.outbound.url) return { tone: 'rose', label: 'no outbound URL', failing: true, lastActivityMs: null };
  if (connector.status.last_error) {
    return {
      tone: 'rose',
      label: connector.status.retries > 0 ? `failing · ${connector.status.retries} retries` : 'last test failed',
      failing: true,
      lastActivityMs: connector.status.last_failure_ms ?? connector.status.last_success_ms,
    };
  }
  if (connector.status.out_secret_present && !connector.outbound.secret_env) {
    return { tone: 'amber', label: 'secret name missing', failing: false, lastActivityMs: connector.status.last_success_ms };
  }
  if (connector.status.last_success_ms) {
    return { tone: 'accent', label: 'delivering', failing: false, lastActivityMs: connector.status.last_success_ms };
  }
  return { tone: 'slate', label: 'configured', failing: false, lastActivityMs: null };
}

/** Derive the inbound webhook URL from the bridge base URL (contract §2). */
export function deriveInboundUrl(baseUrl: string, id: string): string {
  const base = buildHttpBaseUrl(baseUrl).replace(/\/+$/, '');
  return `${base}/api/gateways/${encodeURIComponent(id)}/inbound`;
}

export function gatewayDraftFromConnector(connector: GatewayConnector): GatewayDraft {
  return {
    id: connector.id,
    url: connector.outbound.url,
    outSecretEnv: connector.outbound.secret_env ?? '',
    inSecretEnv: connector.inbound.secret_env ?? '',
    channelIds: connector.outbound.channel_ids.join(', '),
    includeDirect: connector.outbound.include_direct,
    enabled: connector.enabled,
    inboundEnabled: connector.inbound.enabled,
    inboundChannelId: connector.inbound.channel_id ?? '',
    inboundMaxChars: String(connector.inbound.max_chars),
  };
}

/** Editable form state — secret fields contain env-var *names* only. */
export interface GatewayDraft {
  id: string;
  url: string;
  outSecretEnv: string;
  inSecretEnv: string;
  channelIds: string;
  includeDirect: boolean;
  enabled: boolean;
  inboundEnabled: boolean;
  inboundChannelId: string;
  inboundMaxChars: string;
}

export type GatewayDraftErrors = Partial<Record<keyof GatewayDraft, string>>;

export interface GatewayValidation {
  errors: GatewayDraftErrors;
  config: GatewayConfigInput | null;
}

export function validateGatewayDraft(draft: GatewayDraft, options: { existingIds?: string[]; editing?: boolean } = {}): GatewayValidation {
  const errors: GatewayDraftErrors = {};
  const id = draft.id.trim();
  if (!id) errors.id = 'Give the connector a short id, e.g. sms-ops.';
  else if (!gatewayIdPatternForUi().test(id)) {
    errors.id = 'Use lowercase letters, digits, dash or underscore (max 64 chars).';
  } else if (!options.editing && options.existingIds?.includes(id)) {
    errors.id = 'A connector with this id already exists.';
  }

  const url = draft.url.trim();
  if (!url) errors.url = 'Paste the adapter’s HTTPS webhook URL.';
  else {
    try {
      const parsed = new URL(url);
      if (parsed.protocol !== 'https:' && parsed.protocol !== 'http:') errors.url = 'Use an http(s) webhook URL.';
      else if (!parsed.hostname) errors.url = 'The URL is missing a host.';
    } catch {
      errors.url = 'That does not look like a valid URL.';
    }
  }

  const outEnv = draft.outSecretEnv.trim();
  if (outEnv && !envNamePattern().test(outEnv)) {
    errors.outSecretEnv = 'Use only the env-var NAME (uppercase letters/digits/underscore), never the secret value.';
  }
  const inEnv = draft.inSecretEnv.trim();
  if (inEnv && !envNamePattern().test(inEnv)) {
    errors.inSecretEnv = 'Use only the env-var NAME (uppercase letters/digits/underscore), never the secret value.';
  }

  const channelIds = [...new Set(draft.channelIds.split(/[\s,;]+/).map((item) => item.trim()).filter(Boolean))];
  const badChannels = channelIds.filter((item) => !channelIdPattern().test(item));
  if (badChannels.length > 0) {
    errors.channelIds = `Channel ids are 16 hex characters; check ${badChannels.slice(0, 2).join(', ')}.`;
  }

  const inboundChannel = draft.inboundChannelId.trim();
  if (inboundChannel && !channelIdPattern().test(inboundChannel)) {
    errors.inboundChannelId = 'Inbound channel ids are 16 hex characters.';
  }
  const maxChars = Number.parseInt(draft.inboundMaxChars, 10);
  if (!Number.isFinite(maxChars) || String(maxChars) !== draft.inboundMaxChars.trim() || maxChars < 1 || maxChars > 4000) {
    errors.inboundMaxChars = 'Max chars must be a whole number between 1 and 4000.';
  }
  if (draft.inboundEnabled) {
    if (!inEnv) errors.inSecretEnv = 'Inbound needs the env-var name of its HMAC secret.';
    if (!inboundChannel) errors.inboundChannelId = 'Pick the channel that inbound messages are injected into.';
  }

  const config: GatewayConfigInput | null = Object.keys(errors).length > 0
    ? null
    : {
        id,
        kind: 'webhook',
        enabled: draft.enabled,
        outbound: {
          url,
          secret_env: outEnv || null,
          channel_ids: channelIds,
          include_direct: draft.includeDirect,
          include_attachments: false,
          timeout_ms: 10_000,
          max_retries: 3,
        },
        inbound: {
          enabled: draft.inboundEnabled,
          secret_env: inEnv || null,
          channel_id: inboundChannel || null,
          max_chars: Number.isFinite(maxChars) ? maxChars : 400,
        },
      };
  return { errors, config };
}

/** Test POST response normalization; tolerant of older bridge shapes. */
export function normalizeGatewayTestResult(payload: unknown): GatewayTestResult {
  const record = asRecord(payload) ?? {};
  const status =
    numberOrNull(record.status) ??
    numberOrNull(record.status_code) ??
    numberOrNull(record.http_status) ??
    null;
  const ok = typeof record.ok === 'boolean' ? record.ok : typeof record.success === 'boolean' ? record.success : status !== null && status >= 200 && status < 300;
  const message = stringOrNull(record.message) ?? stringOrNull(record.error) ?? null;
  return { ok, status, message, at_ms: numberOrNull(record.at_ms) ?? Date.now() };
}

export function gatewayTestSummary(result: GatewayTestResult): string {
  if (result.status !== null) {
    return `${result.ok ? 'Test delivered' : 'Test failed'} · HTTP ${result.status}${result.message ? ` — ${result.message}` : ''}`;
  }
  return `${result.ok ? 'Test delivered' : 'Test failed'}${result.message ? ` — ${result.message}` : ''}`;
}

/** MCP status normalization. There is no invented REST endpoint: callers pass an optional payload. */
export function normalizeMcpStatus(raw: unknown, source: McpHttpStatus['source'] = 'unknown'): McpHttpStatus | null {
  const record = asRecord(raw);
  if (!record) return null;
  const path = stringOrNull(record.path) ?? MCP_CONFIG_DEFAULTS.path;
  const endpoint = stringOrNull(record.endpoint_url) ?? stringOrNull(record.endpoint);
  return {
    enabled: boolOr(record.enabled, false),
    path,
    allow_write: boolOr(record.allow_write, false),
    require_confirm: boolOr(record.require_confirm, true),
    rate_limit_per_min: Math.max(1, Math.round(numberOr(record.rate_limit_per_min, MCP_CONFIG_DEFAULTS.rate_limit_per_min))),
    max_sessions: Math.max(1, Math.round(numberOr(record.max_sessions, MCP_CONFIG_DEFAULTS.max_sessions))),
    endpoint_url: endpoint,
    source,
    active_sessions: numberOrNull(record.active_sessions ?? record.sessions),
    requests_per_min: numberOrNull(record.requests_per_min),
  };
}

export function mcpEndpointFromBridgeUrl(bridgeUrl: string, path: string = MCP_CONFIG_DEFAULTS.path): string {
  return `${buildHttpBaseUrl(bridgeUrl).replace(/\/+$/, '')}${path.startsWith('/') ? path : `/${path}`}`;
}

export function mcpStatusOrDefault(status: McpHttpStatus | null, bridgeUrl: string): McpHttpStatus {
  return status ?? {
    enabled: false,
    path: MCP_CONFIG_DEFAULTS.path,
    allow_write: false,
    require_confirm: true,
    rate_limit_per_min: MCP_CONFIG_DEFAULTS.rate_limit_per_min,
    max_sessions: MCP_CONFIG_DEFAULTS.max_sessions,
    endpoint_url: mcpEndpointFromBridgeUrl(bridgeUrl),
    source: 'unknown',
    active_sessions: null,
    requests_per_min: null,
  };
}

export function mcpTransportLabel(status: McpHttpStatus): string {
  return status.enabled ? 'HTTP over the bridge listener' : 'stdio (default)';
}

export function mcpScopeLabel(status: McpHttpStatus): string {
  return status.allow_write ? 'read + write' : 'read-only';
}

/** mcp-http.json snippet for copy/paste; does not contain a token. */
export function mcpConfigSnippet(status: McpHttpStatus): string {
  return JSON.stringify(
    {
      enabled: status.enabled,
      path: status.path || MCP_CONFIG_DEFAULTS.path,
      allow_write: status.allow_write,
      require_confirm: status.require_confirm,
      rate_limit_per_min: status.rate_limit_per_min,
      max_sessions: status.max_sessions,
    },
    null,
    2,
  );
}

export function mcpClientCommand(endpoint: string): string {
  return `openmesh-cli agent config --transport http --url ${endpoint} --token-file ~/.config/openmesh/token`;
}

export function normalizeBackboneLink(raw: unknown): BackboneLink | null {
  const record = asRecord(raw);
  if (!record) return null;
  const name = stringOrNull(record.name)?.trim();
  if (!name) return null;
  const statsRecord = asRecord(record.stats) ?? {};
  return {
    name,
    url: stringOrNull(record.url),
    mode: stringOrNull(record.mode) ?? 'client',
    enabled: boolOr(record.enabled, true),
    connected: boolOr(record.connected, false),
    last_error: stringOrNull(record.last_error),
    stats: Object.keys(statsRecord).length > 0
      ? {
          peers: numberOrNull(statsRecord.peers) ?? undefined,
          uptime_s: numberOrNull(statsRecord.uptime_s) ?? undefined,
          frames_in: numberOrNull(statsRecord.frames_in) ?? undefined,
          frames_out: numberOrNull(statsRecord.frames_out) ?? undefined,
          bytes_in: numberOrNull(statsRecord.bytes_in) ?? undefined,
          bytes_out: numberOrNull(statsRecord.bytes_out) ?? undefined,
        }
      : null,
  };
}

export function normalizeBackboneLinksResponse(payload: unknown): BackboneLink[] {
  const record = asRecord(payload);
  const rawList = Array.isArray(payload)
    ? payload
    : Array.isArray(record?.links)
      ? (record!.links as unknown[])
      : Array.isArray(record?.backbone_links)
        ? (record!.backbone_links as unknown[])
        : [];
  return rawList
    .map(normalizeBackboneLink)
    .filter((item): item is BackboneLink => item !== null)
    .sort((left, right) => left.name.localeCompare(right.name));
}

/** An enabled legacy single-link config means the UI should show the fallback card. */
export function hasLegacyBackboneLink(config: BackboneConfig | null): boolean {
  return Boolean(config?.enabled);
}

export function redactBackboneLinkUrl(url: string | null | undefined): string {
  if (!url) return '';
  return url.replace(/([?&]token=)[^&]*/i, '$1••••••');
}

export function validateBackboneLinkDraft(
  draft: { name: string; url: string },
  options: { existingNames?: string[]; editing?: boolean } = {},
): { errors: Partial<Record<'name' | 'url', string>>; input: BackboneLinkInput | null } {
  const errors: Partial<Record<'name' | 'url', string>> = {};
  const name = draft.name.trim();
  if (!name) errors.name = 'Name this link, e.g. oslo or cabin.';
  else if (!/^[A-Za-z0-9][A-Za-z0-9_-]{0,31}$/.test(name)) {
    errors.name = 'Use letters/digits/dash/underscore (max 32 chars).';
  } else if (!options.editing && options.existingNames?.includes(name)) {
    errors.name = 'That link name is already in use.';
  }
  const url = draft.url.trim();
  if (!url) errors.url = 'Paste the openmesh://backbone?… invite link.';
  else {
    try {
      const parsed = new URL(url);
      if (parsed.protocol !== 'openmesh:' || !/^\/?backbone/.test(parsed.pathname + parsed.hostname)) {
        // URLs like openmesh://backbone?... parse host="backbone".
        if (!(parsed.protocol === 'openmesh:' && parsed.hostname === 'backbone')) {
          errors.url = 'Use the openmesh://backbone?… link from the gateway panel.';
        }
      } else if (!parsed.searchParams.get('host') || !parsed.searchParams.get('port')) {
        errors.url = 'The link needs at least host and port parameters.';
      }
    } catch {
      errors.url = 'That does not look like an openmesh:// invite link.';
    }
  }
  return {
    errors,
    input: Object.keys(errors).length > 0 ? null : { name, url, enabled: true },
  };
}

export function backboneLinkStatus(link: BackboneLink): { tone: 'accent' | 'amber' | 'rose' | 'slate'; label: string } {
  if (!link.enabled) return { tone: 'slate', label: 'disabled' };
  if (link.connected) return { tone: 'accent', label: 'connected' };
  if (link.last_error) return { tone: 'rose', label: 'error' };
  return { tone: 'amber', label: 'connecting' };
}
