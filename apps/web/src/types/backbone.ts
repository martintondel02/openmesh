/**
 * Types for OpenMesh Backbone mode (OBB/1).
 *
 * Normative sources:
 *  - docs/architecture/BACKBONE.md §5 (config), §7 (control surface), §8 (setup)
 *  - docs/architecture/WEBSOCKET-API.md §8 (backbone control surface)
 *
 * These mirror the JSON payloads exactly; fields are snake_case because they
 * come straight off the bridge.
 */

export type BackboneAuthMode = 'token' | 'open';

/** One connected backbone peer (GET /api/backbone/peers item, §7.1). */
export interface BackbonePeer {
  node_id: string;
  name: string;
  /** `inbound` when the peer dialled us, `outbound` for links we dialled. */
  direction: 'inbound' | 'outbound' | string;
  remote_addr: string;
  connected_at_ms: number;
  last_seen_ms: number;
  frames_in: number;
  frames_out: number;
  bytes_in: number;
  bytes_out: number;
  /** Null until the first ping/pong round-trip. */
  rtt_ms: number | null;
  app_version: string;
  auth: BackboneAuthMode | string;
}

/**
 * Live statistics pushed via the `backbone_status` event (§7.2).
 *
 * The bridge implementation extends this event with optional response fields:
 * `config` (redacted config snapshot, reply to get_backbone/set_backbone_config),
 * `restarted` (listener moved) and `disconnected` (reply to backbone_kick).
 * Clients must ignore fields they do not understand.
 */
export interface BackboneStatus {
  running: boolean;
  peers: number;
  uptime_s: number;
  frames_in: number;
  frames_out: number;
  bytes_in: number;
  bytes_out: number;
  throughput_in_bps: number;
  throughput_out_bps: number;
  store_forward_envelopes: number;
  auth_failures: number;
  last_error: string | null;
}

export interface BackboneTotals {
  frames_in?: number;
  frames_out?: number;
  bytes_in?: number;
  bytes_out?: number;
  peers?: number;
  auth_failures?: number;
  uptime_s?: number;
}

/**
 * Backbone configuration (GET /api/backbone, §5). The token itself is never
 * returned by the API; only `token_set` is.
 */
export interface BackboneConfig {
  enabled: boolean;
  running: boolean;
  bind: string;
  port: number;
  auth: BackboneAuthMode;
  token_set: boolean;
  max_peers: number;
  allowlist: string[];
  store_forward: boolean;
  name: string;
  public_host: string;
  ping_interval_s: number;
  totals?: BackboneTotals;
}

/** Response of POST /api/backbone/config (§7.1). */
export interface BackboneConfigResponse extends BackboneConfig {
  restarted?: boolean;
}

/** Partial update accepted by POST /api/backbone/config. */
export type BackboneConfigPatch = Partial<
  Pick<
    BackboneConfig,
    | 'enabled'
    | 'bind'
    | 'port'
    | 'auth'
    | 'max_peers'
    | 'allowlist'
    | 'store_forward'
    | 'name'
    | 'public_host'
    | 'ping_interval_s'
  >
>;

/** `backbone_peer` event payload (§7.2). */
export interface BackboneStatusEvent extends BackboneStatus {
  config?: BackboneConfig;
  restarted?: boolean;
  disconnected?: boolean;
}

export interface BackbonePeerEvent {
  action: 'joined' | 'left' | 'updated' | string;
  peer: BackbonePeer;
}

/** `backbone_token` event payload for the WS rotate command (API §4.5). */
export interface BackboneTokenEvent {
  token: string;
}

/** One numbered onboarding step from GET /api/backbone/setup (§8). */
export interface BackboneSetupStep {
  title: string;
  commands: string[];
  note?: string | null;
}

export interface BackboneFirewallCommands {
  firewalld: string[];
  ufw: string[];
}

/** Public-exposure provider (BACKBONE.md §8/§9b). */
export interface BackboneExposureProvider {
  name: string;
  url: string;
  description?: string;
}

/**
 * Optional exposure guidance added to the v0.4 setup payload. Older bridges omit
 * it entirely; the client then renders the built-in Openflare-style fallback.
 */
export interface BackboneExposure {
  /** e.g. `openflare`; informational — the UI always offers the tunnel path. */
  recommended?: string | null;
  provider?: BackboneExposureProvider | null;
  when_to_use?: string | null;
  steps?: string[];
  alternatives?: string[];
  /** Optional command override from the bridge; generated locally when absent. */
  bind_command?: string | null;
  /** Optional guide URL override; defaults to docs/guides/backbone-exposure.md on GitHub. */
  guide_url?: string | null;
}

/** GET /api/backbone/setup payload (§8). */
export interface BackboneSetup {
  listen: string;
  public_host: string;
  connection_string: string;
  token_set: boolean;
  auth: BackboneAuthMode;
  steps: BackboneSetupStep[];
  systemd_unit: string;
  firewall: BackboneFirewallCommands;
  notes: string[];
  panel_url: string;
  /** Public-exposure guidance (v0.4); optional for backwards compatibility. */
  exposure?: BackboneExposure | null;
}

/** Response shape of GET /api/backbone/peers is a bare array. */
export type BackbonePeerList = BackbonePeer[];

/** GET /api/backbone/qr.svg returns SVG markup as text. */
export type BackboneQrSvg = string;

/**
 * GET /api/backbone when the bridge is not in backbone mode returns
 * `{enabled:false}` with HTTP 200 (WEBSOCKET-API §8).
 */
export interface BackboneDisabledResponse {
  enabled: false;
}

export type BackboneConfigResponseRaw = BackboneConfig | BackboneDisabledResponse;
