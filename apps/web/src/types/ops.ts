/**
 * Remote-operations types (v0.7).
 *
 * Normative: docs/design/remote-operations.md §1 (remote MCP HTTP), §2 (gateways),
 * §3 (multiple backbone links). The bridge owns these shapes; the web client only
 * reads redacted views (secret *values* never leave the bridge).
 */

export type GatewayKind = 'webhook' | string;

export interface GatewayOutboundConfig {
  url: string;
  /** Name of the environment variable holding the HMAC secret — never the value. */
  secret_env: string | null;
  channel_ids: string[];
  include_direct: boolean;
  /** v0.7 sends attachment metadata only; the toggle exists for forward compatibility. */
  include_attachments: boolean;
  timeout_ms: number;
  max_retries: number;
}

export interface GatewayInboundConfig {
  enabled: boolean;
  secret_env: string | null;
  channel_id: string | null;
  max_chars: number;
}

/** Runtime status fields as exposed by GET /api/gateways (redacted). */
export interface GatewayRuntimeStatus {
  last_success_ms: number | null;
  last_failure_ms: number | null;
  last_error: string | null;
  retries: number;
  out_secret_present: boolean;
  in_secret_present: boolean;
}

export interface GatewayConnector {
  id: string;
  kind: GatewayKind;
  enabled: boolean;
  outbound: GatewayOutboundConfig;
  inbound: GatewayInboundConfig;
  status: GatewayRuntimeStatus;
}

/** PUT /api/gateways/{id} body (no secrets, only env-var names). */
export interface GatewayConfigInput {
  id: string;
  kind: GatewayKind;
  enabled: boolean;
  outbound: Omit<GatewayOutboundConfig, never>;
  inbound: GatewayInboundConfig;
}

export interface GatewayTestResult {
  ok: boolean;
  status: number | null;
  message: string | null;
  at_ms: number;
}

/** One outbound backbone link (GET /api/backbone/links). */
export interface BackboneLink {
  name: string;
  url: string | null;
  mode: string;
  enabled: boolean;
  connected: boolean;
  last_error: string | null;
  stats: {
    peers?: number;
    uptime_s?: number;
    frames_in?: number;
    frames_out?: number;
    bytes_in?: number;
    bytes_out?: number;
  } | null;
}

export interface BackboneLinkInput {
  name: string;
  url: string;
  enabled: boolean;
}

/** HTTP MCP configuration/status (mcp-http.json §1). */
export interface McpHttpStatus {
  enabled: boolean;
  path: string;
  allow_write: boolean;
  require_confirm: boolean;
  rate_limit_per_min: number;
  max_sessions: number;
  /** Present only if the bridge exposes it; never invented by the web client. */
  endpoint_url: string | null;
  source: 'bridge' | 'demo' | 'config-file' | 'unknown';
  active_sessions: number | null;
  requests_per_min: number | null;
}
