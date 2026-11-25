/**
 * Pure helpers for Backbone mode (OBB/1).
 *
 * Normative sources: docs/architecture/BACKBONE.md §5 (config), §6 (connection
 * string), §7 (control surface), §8 (setup payload) and WEBSOCKET-API.md §8.
 * Keep this file DOM-free so it is easy to unit test.
 */

import type {
  BackboneConfig,
  BackboneConfigPatch,
  BackbonePeer,
  BackbonePeerEvent,
  BackboneSetup,
  BackboneSetupStep,
  BackboneStatus,
} from '../types/backbone';
import { isValidNodeId, normalizeHex } from './utils';

export const BACKBONE_DEFAULT_PORT = 9443;
export const BACKBONE_DEFAULT_BIND = '0.0.0.0';
export const BACKBONE_DEFAULT_MAX_PEERS = 32;
export const BACKBONE_DEFAULT_PING_INTERVAL_S = 30;
export const BACKBONE_RELEASES_URL = 'https://github.com/martintondel02/openmesh/releases/latest';
export const BACKBONE_INSTALL_URL = 'https://github.com/martintondel02/openmesh/releases/latest/download/install.sh';

/* ------------------------------------------------------------------ */
/* Connection string (BACKBONE.md §6)                                 */
/* ------------------------------------------------------------------ */

export interface ConnectionStringOptions {
  host: string;
  port: number;
  /** Omit for open-auth backbones. */
  token?: string | null;
  /** Server node id (16 hex chars). */
  nodeId?: string | null;
  /** Human label for the backbone. */
  name?: string | null;
}

/**
 * `openmesh://backbone?host=…&port=9443&token=…&node=…&name=…&v=1`
 * Token is omitted in open mode, as required by §6.
 */
export function buildBackboneConnectionString(options: ConnectionStringOptions): string {
  const params = new URLSearchParams();
  params.set('host', options.host.trim() || '127.0.0.1');
  params.set('port', String(Math.round(options.port || BACKBONE_DEFAULT_PORT)));
  if (options.token) params.set('token', options.token);
  if (options.nodeId) params.set('node', normalizeHex(options.nodeId));
  if (options.name) params.set('name', options.name);
  params.set('v', '1');
  return `openmesh://backbone?${params.toString()}`;
}

/** Read the token out of a connection string, or null when absent. */
export function connectionStringToken(connectionString: string): string | null {
  try {
    const url = new URL(connectionString);
    return url.searchParams.get('token');
  } catch {
    return null;
  }
}

/** Replace a bearer token with bullets for display. */
export function maskToken(token: string | null | undefined, visible = 6): string {
  if (!token) return '—';
  if (token.length <= visible) return '•'.repeat(token.length);
  return `${token.slice(0, visible)}${'•'.repeat(Math.min(28, token.length - visible))}`;
}

/**
 * Redact the token inside a connection string so it can be shown on screen
 * without leaking the bearer secret. Other parameters stay readable.
 */
export function redactConnectionString(connectionString: string): string {
  const token = connectionStringToken(connectionString);
  if (!token) return connectionString;
  return connectionString.replace(`token=${encodeURIComponent(token)}`, `token=${'•'.repeat(12)}`);
}

/** True when the URL carries an auth token (i.e. token mode). */
export function connectionStringHasToken(connectionString: string): boolean {
  return Boolean(connectionStringToken(connectionString));
}

/* ------------------------------------------------------------------ */
/* Formatting                                                         */
/* ------------------------------------------------------------------ */

/** Throughput in bits/s, e.g. `812 b/s`, `1.2 kb/s`, `2.4 Mb/s`. */
export function formatThroughput(bps: number | null | undefined): string {
  if (bps === null || bps === undefined || !Number.isFinite(bps)) return '—';
  const value = Math.max(0, bps);
  if (value < 1000) return `${Math.round(value)} b/s`;
  if (value < 1_000_000) return `${(value / 1000).toFixed(value < 10_000 ? 1 : 0).replace(/\.0$/, '')} kb/s`;
  return `${(value / 1_000_000).toFixed(1).replace(/\.0$/, '')} Mb/s`;
}

/** Compact integer for stat cards: `987`, `42.1k`, `1.84M`. */
export function formatCount(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return '—';
  const count = Math.max(0, value);
  if (count < 1000) return String(Math.round(count));
  if (count < 1_000_000) return `${(count / 1000).toFixed(count < 10_000 ? 1 : 0).replace(/\.0$/, '')}k`;
  return `${(count / 1_000_000).toFixed(2).replace(/\.?0+$/, '')}M`;
}

/** Traffic counters pair, e.g. `120 / 98 frames`. */
export function formatCounterPair(first: number | null | undefined, second: number | null | undefined, unit: string): string {
  const a = Number.isFinite(first) ? Math.round(first as number) : 0;
  const b = Number.isFinite(second) ? Math.round(second as number) : 0;
  return `${a} / ${b} ${unit}`;
}

/** Peer direction label for the table. */
export function formatPeerDirection(direction: string | null | undefined): string {
  switch (direction) {
    case 'inbound':
      return 'Inbound';
    case 'outbound':
      return 'Outbound';
    default:
      return direction ? direction.replace(/_/g, ' ') : 'Unknown';
  }
}

export type RttTone = 'good' | 'fair' | 'poor' | 'unknown';

/** RTT quality buckets for the peer table badge. */
export function rttTone(rttMs: number | null | undefined): RttTone {
  if (rttMs === null || rttMs === undefined || !Number.isFinite(rttMs) || rttMs <= 0) return 'unknown';
  if (rttMs < 80) return 'good';
  if (rttMs < 250) return 'fair';
  return 'poor';
}

export function formatRtt(rttMs: number | null | undefined): string {
  if (rttMs === null || rttMs === undefined || !Number.isFinite(rttMs) || rttMs <= 0) return '—';
  return `${Math.round(rttMs)} ms`;
}

export function formatAuthMode(auth: string | null | undefined): string {
  return auth === 'open' ? 'Open (no auth)' : 'Token (HMAC-SHA256)';
}

/** Public endpoint shown in the panel header (public_host wins over bind). */
export function formatEndpoint(config: Pick<BackboneConfig, 'public_host' | 'bind' | 'port'> | null | undefined): string {
  if (!config) return '—';
  const host = config.public_host?.trim() || config.bind || BACKBONE_DEFAULT_BIND;
  return `${host}:${config.port || BACKBONE_DEFAULT_PORT}`;
}

export function formatListenAddress(config: Pick<BackboneConfig, 'bind' | 'port'> | null | undefined): string {
  if (!config) return '—';
  return `${config.bind || BACKBONE_DEFAULT_BIND}:${config.port || BACKBONE_DEFAULT_PORT}`;
}

/** Uptime for the status strip, e.g. `2d 4h`, `12m`. */
export function formatBackboneUptime(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds) || seconds < 0) return '—';
  const total = Math.floor(seconds);
  if (total < 60) return `${total}s`;
  const m = Math.floor(total / 60) % 60;
  const h = Math.floor(total / 3600) % 24;
  const d = Math.floor(total / 86400);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

export type BackbonePhase = 'running' | 'running-open' | 'stopped' | 'disabled' | 'loading';

export interface BackbonePill {
  phase: BackbonePhase;
  label: string;
  tone: 'accent' | 'amber' | 'rose' | 'slate' | 'sky';
}

/** Status pill model for the panel header. */
export function backbonePill(
  config: Pick<BackboneConfig, 'enabled' | 'running' | 'auth'> | null | undefined,
  status: Pick<BackboneStatus, 'running'> | null | undefined,
): BackbonePill {
  if (!config) return { phase: 'loading', label: 'Backbone: loading', tone: 'slate' };
  if (!config.enabled) return { phase: 'disabled', label: 'Backbone: off', tone: 'slate' };
  const running = status?.running ?? config.running;
  if (!running) return { phase: 'stopped', label: 'Backbone: stopped', tone: 'amber' };
  if (config.auth === 'open') {
    return { phase: 'running-open', label: 'Running · open LAN', tone: 'rose' };
  }
  return { phase: 'running', label: 'Backbone: running', tone: 'accent' };
}

/** True when the bridge advertises backbone support or is already running one. */
export function isBackboneRelevant(options: {
  features?: string[] | null;
  config?: Pick<BackboneConfig, 'enabled'> | null;
  demo?: boolean;
}): boolean {
  if (options.demo) return true;
  if (options.config?.enabled) return true;
  return Boolean(options.features?.includes('backbone'));
}

/* ------------------------------------------------------------------ */
/* Config normalization, diffing, validation                         */
/* ------------------------------------------------------------------ */

export function defaultBackboneConfig(overrides: Partial<BackboneConfig> = {}): BackboneConfig {
  return {
    enabled: false,
    running: false,
    bind: BACKBONE_DEFAULT_BIND,
    port: BACKBONE_DEFAULT_PORT,
    auth: 'token',
    token_set: false,
    max_peers: BACKBONE_DEFAULT_MAX_PEERS,
    allowlist: [],
    store_forward: true,
    name: 'openmesh-backbone',
    public_host: '',
    ping_interval_s: BACKBONE_DEFAULT_PING_INTERVAL_S,
    ...overrides,
  };
}

/** Normalize a GET /api/backbone payload, including the `{enabled:false}` form. */
export function normalizeBackboneConfig(raw: unknown): BackboneConfig {
  if (!raw || typeof raw !== 'object') return defaultBackboneConfig();
  const source = raw as Record<string, unknown>;
  if (source.enabled === false && Object.keys(source).length === 1) {
    return defaultBackboneConfig({ enabled: false, running: false });
  }
  const allowlist = Array.isArray(source.allowlist)
    ? source.allowlist.filter((entry): entry is string => typeof entry === 'string').map(normalizeHex)
    : [];
  const totals = source.totals && typeof source.totals === 'object' ? (source.totals as BackboneConfig['totals']) : undefined;
  return defaultBackboneConfig({
    enabled: Boolean(source.enabled),
    running: source.running === undefined ? Boolean(source.enabled) : Boolean(source.running),
    bind: typeof source.bind === 'string' && source.bind ? source.bind : BACKBONE_DEFAULT_BIND,
    port: typeof source.port === 'number' ? source.port : BACKBONE_DEFAULT_PORT,
    auth: source.auth === 'open' ? 'open' : 'token',
    token_set: Boolean(source.token_set),
    max_peers: typeof source.max_peers === 'number' ? source.max_peers : BACKBONE_DEFAULT_MAX_PEERS,
    allowlist,
    store_forward: source.store_forward === undefined ? true : Boolean(source.store_forward),
    name: typeof source.name === 'string' && source.name ? source.name : 'openmesh-backbone',
    public_host: typeof source.public_host === 'string' ? source.public_host : '',
    ping_interval_s:
      typeof source.ping_interval_s === 'number' ? source.ping_interval_s : BACKBONE_DEFAULT_PING_INTERVAL_S,
    ...(totals ? { totals } : {}),
  });
}

/** Merge a partial config patch into a config snapshot. */
export function mergeBackboneConfig(base: BackboneConfig, patch: BackboneConfigPatch): BackboneConfig {
  const next: BackboneConfig = { ...base };
  for (const [key, value] of Object.entries(patch)) {
    if (value === undefined) continue;
    (next as unknown as Record<string, unknown>)[key] = key === 'allowlist' ? [...(value as string[])].map(normalizeHex) : value;
  }
  if (next.running && !next.enabled) next.running = false;
  return next;
}

/**
 * Compute the minimal PATCH body between the saved config and a draft.
 * Returns `{}` when nothing changed (caller disables Save).
 */
export function diffBackboneConfig(base: BackboneConfig, draft: BackboneConfig): BackboneConfigPatch {
  const patch: BackboneConfigPatch = {};
  const scalarKeys = [
    'enabled',
    'bind',
    'port',
    'auth',
    'max_peers',
    'store_forward',
    'name',
    'public_host',
    'ping_interval_s',
  ] as const;
  for (const key of scalarKeys) {
    if (base[key] !== draft[key]) {
      (patch as Record<string, unknown>)[key] = draft[key];
    }
  }
  const baseAllow = [...base.allowlist].map(normalizeHex).sort();
  const draftAllow = [...draft.allowlist].map(normalizeHex).sort();
  if (baseAllow.join(',') !== draftAllow.join(',')) patch.allowlist = draftAllow;
  return patch;
}

export function hasBackboneChanges(base: BackboneConfig | null, draft: BackboneConfig | null): boolean {
  if (!base || !draft) return false;
  return Object.keys(diffBackboneConfig(base, draft)).length > 0;
}

export type AllowlistResult = { ok: true; nodeId: string } | { ok: false; error: string };

/** Validate one allowlist entry (8-byte node id, hex). */
export function validateAllowlistEntry(input: string): AllowlistResult {
  const normalized = normalizeHex(input);
  if (!normalized) return { ok: false, error: 'Paste a 16-character node id.' };
  if (!isValidNodeId(normalized)) return { ok: false, error: 'Node ids are 16 hex characters (8 bytes).' };
  return { ok: true, nodeId: normalized };
}

export function addAllowlistEntry(allowlist: string[], input: string): { allowlist: string[]; error: string | null } {
  const result = validateAllowlistEntry(input);
  if (!result.ok) return { allowlist, error: result.error };
  if (allowlist.map(normalizeHex).includes(result.nodeId)) {
    return { allowlist, error: 'That node id is already on the allowlist.' };
  }
  return { allowlist: [...allowlist, result.nodeId], error: null };
}

export function removeAllowlistEntry(allowlist: string[], nodeId: string): string[] {
  const normalized = normalizeHex(nodeId);
  return allowlist.filter((entry) => normalizeHex(entry) !== normalized);
}

/* ------------------------------------------------------------------ */
/* Peers                                                              */
/* ------------------------------------------------------------------ */

export function sortPeers(peers: BackbonePeer[]): BackbonePeer[] {
  return [...peers].sort((a, b) => {
    if (a.direction !== b.direction) return a.direction === 'inbound' ? -1 : 1;
    const nameDelta = a.name.localeCompare(b.name);
    if (nameDelta !== 0) return nameDelta;
    return a.node_id.localeCompare(b.node_id);
  });
}

/** Apply a `backbone_peer` event to the peer list. */
export function applyPeerEvent(peers: BackbonePeer[], event: BackbonePeerEvent): BackbonePeer[] {
  if (!event?.peer?.node_id) return peers;
  const id = normalizeHex(event.peer.node_id);
  if (event.action === 'left') {
    return peers.filter((peer) => normalizeHex(peer.node_id) !== id);
  }
  const index = peers.findIndex((peer) => normalizeHex(peer.node_id) === id);
  if (index < 0) return sortPeers([...peers, { ...event.peer, node_id: id }]);
  const next = peers.slice();
  next[index] = { ...next[index]!, ...event.peer, node_id: id };
  return sortPeers(next);
}

/** Total traffic across peers (for stat cards when status is not available yet). */
export function sumPeerTraffic(peers: BackbonePeer[]): { frames_in: number; frames_out: number; bytes_in: number; bytes_out: number } {
  return peers.reduce(
    (totals, peer) => ({
      frames_in: totals.frames_in + (Number.isFinite(peer.frames_in) ? peer.frames_in : 0),
      frames_out: totals.frames_out + (Number.isFinite(peer.frames_out) ? peer.frames_out : 0),
      bytes_in: totals.bytes_in + (Number.isFinite(peer.bytes_in) ? peer.bytes_in : 0),
      bytes_out: totals.bytes_out + (Number.isFinite(peer.bytes_out) ? peer.bytes_out : 0),
    }),
    { frames_in: 0, frames_out: 0, bytes_in: 0, bytes_out: 0 },
  );
}

/* ------------------------------------------------------------------ */
/* Setup payload (§8) and install section                             */
/* ------------------------------------------------------------------ */

export interface InstallOption {
  id: 'linux' | 'macos' | 'windows';
  label: string;
  hint: string;
  commands: string[];
  note: string;
}

/** Client-side install instructions (the setup payload does not include these). */
export const INSTALL_OPTIONS: InstallOption[] = [
  {
    id: 'linux',
    label: 'Linux',
    hint: 'Fedora · Debian · Ubuntu · Raspberry Pi OS',
    commands: [`curl -fsSL ${BACKBONE_INSTALL_URL} | sh`],
    note: 'Installs the release binary + web bundle, verifies the sha256, and adds openmesh-bridge to your PATH.',
  },
  {
    id: 'macos',
    label: 'macOS',
    hint: 'Apple silicon + Intel',
    commands: [
      `curl -fsSL ${BACKBONE_INSTALL_URL} | sh`,
      'openmesh-bridge backbone --print-setup   # prints a ready-to-run config',
    ],
    note: 'The same installer detects macOS (arm64/x86_64) and installs the universal release build.',
  },
  {
    id: 'windows',
    label: 'Windows',
    hint: 'Windows 10/11 · x86_64 · Git Bash / WSL',
    commands: [
      `curl -fsSL ${BACKBONE_INSTALL_URL} | sh`,
      `# or from PowerShell: download the windows-x86_64 release .tar.gz from ${BACKBONE_RELEASES_URL}`,
    ],
    note: 'Run the installer from Git Bash (it detects windows-x86_64 and unpacks the release .tar.gz), or unpack the tarball manually. WSL users can install the Linux build.',
  },
];

/** Firewall command fallbacks when GET /api/backbone/setup is unavailable. */
export function buildFallbackFirewall(port: number): { firewalld: string[]; ufw: string[] } {
  return {
    firewalld: [`sudo firewall-cmd --add-port=${port}/tcp --permanent`, 'sudo firewall-cmd --reload'],
    ufw: [`sudo ufw allow ${port}/tcp`, 'sudo ufw reload'],
  };
}

export function buildFallbackSystemdUnit(): string {
  return `[Unit]
Description=OpenMesh backbone
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=%h/.local/bin/openmesh-bridge backbone
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=read-only
PrivateTmp=true
ReadWritePaths=%h/.local/share/openmesh %h/.config/openmesh

[Install]
WantedBy=default.target`;
}

/** Build the onboarding payload locally when the bridge cannot provide one. */
export function buildFallbackSetup(options: {
  config: BackboneConfig | null;
  connectionString: string;
  panelUrl?: string;
}): BackboneSetup {
  const config = options.config ?? defaultBackboneConfig();
  const port = config.port || BACKBONE_DEFAULT_PORT;
  const connectionString = options.connectionString;
  const steps: BackboneSetupStep[] = [
    {
      title: 'Install OpenMesh',
      commands: [`curl -fsSL ${BACKBONE_INSTALL_URL} | sh`],
      note: 'The installer verifies the release checksum. On Windows, run it from Git Bash or unpack the release tarball (.tar.gz).',
    },
    {
      title: 'Start the backbone',
      commands: ['openmesh-bridge backbone --data-dir ~/.openmesh'],
      note: 'First run generates the 32-byte token, writes backbone.json (0600) and prints the onboarding guide.',
    },
    {
      title: 'Open the firewall',
      commands: buildFallbackFirewall(port).firewalld,
      note: 'Pick the tab for your firewall; port changes require another rule.',
    },
    {
      title: 'Keep it running',
      commands: ['openmesh-bridge install-service --backbone'],
      note: 'Installs a systemd user service (or launchd agent) that survives reboot.',
    },
    {
      title: 'Connect a gateway',
      commands: [`openmesh-bridge join '${connectionString}'`],
      note: 'Run this on a gateway or repeater on the far side of the link; it reconnects with backoff.',
    },
    {
      title: 'Verify it works',
      commands: ['curl -fsS -H "Authorization: Bearer $OPENMESH_TOKEN" http://127.0.0.1:8787/api/backbone/peers'],
      note: 'The gateway appears in the peer table above within ~30 s; the panel updates live over WebSocket.',
    },
  ];
  return {
    listen: formatListenAddress(config),
    public_host: config.public_host,
    connection_string: connectionString,
    token_set: config.token_set,
    auth: config.auth,
    steps,
    systemd_unit: buildFallbackSystemdUnit(),
    firewall: buildFallbackFirewall(port),
    notes: [
      'OpenMesh payloads stay end-to-end encrypted; the backbone relays ciphertext and routing metadata only.',
      'The token is a bearer secret. Rotate it from the panel if it ever leaks; stale peers are disconnected.',
      'Native TLS is not built in yet: front the port with Caddy/nginx/stunnel or run the link inside WireGuard/Tailscale.',
    ],
    panel_url: options.panelUrl ?? 'http://127.0.0.1:8787/backbone',
  };
}

/** Defensive normalization of a bridge-provided setup payload. */
export function normalizeBackboneSetup(raw: unknown, fallback: BackboneSetup): BackboneSetup {
  if (!raw || typeof raw !== 'object') return fallback;
  const source = raw as Record<string, unknown>;
  const steps = Array.isArray(source.steps)
    ? source.steps
        .filter((step): step is Record<string, unknown> => Boolean(step) && typeof step === 'object')
        .map((step) => ({
          title: typeof step.title === 'string' ? step.title : 'Step',
          commands: Array.isArray(step.commands) ? step.commands.filter((command): command is string => typeof command === 'string') : [],
          note: typeof step.note === 'string' ? step.note : null,
        }))
        .filter((step) => step.commands.length > 0)
    : fallback.steps;
  const firewall = source.firewall && typeof source.firewall === 'object' ? (source.firewall as Record<string, unknown>) : {};
  const firewalld = Array.isArray(firewall.firewalld)
    ? firewall.firewalld.filter((command): command is string => typeof command === 'string')
    : fallback.firewall.firewalld;
  const ufw = Array.isArray(firewall.ufw) ? firewall.ufw.filter((command): command is string => typeof command === 'string') : fallback.firewall.ufw;
  const notes = Array.isArray(source.notes) ? source.notes.filter((note): note is string => typeof note === 'string') : fallback.notes;
  return {
    listen: typeof source.listen === 'string' && source.listen ? source.listen : fallback.listen,
    public_host: typeof source.public_host === 'string' ? source.public_host : fallback.public_host,
    connection_string:
      typeof source.connection_string === 'string' && source.connection_string
        ? source.connection_string
        : fallback.connection_string,
    token_set: source.token_set === undefined ? fallback.token_set : Boolean(source.token_set),
    auth: source.auth === 'open' ? 'open' : fallback.auth,
    steps: steps.length > 0 ? steps : fallback.steps,
    systemd_unit:
      typeof source.systemd_unit === 'string' && source.systemd_unit ? source.systemd_unit : fallback.systemd_unit,
    firewall: { firewalld, ufw },
    notes: notes.length > 0 ? notes : fallback.notes,
    panel_url: typeof source.panel_url === 'string' && source.panel_url ? source.panel_url : fallback.panel_url,
  };
}

/** Which setup step shows firewall tabs (title contains "firewall"). */
export function isFirewallStep(step: BackboneSetupStep): boolean {
  return /firewall/i.test(step.title);
}

/** Which setup step is the "connect a gateway" one. */
export function isGatewayStep(step: BackboneSetupStep): boolean {
  return /connect|gateway|join/i.test(step.title);
}

/** Replace the connection string inside a bridge setup payload with the current one. */
export function withConnectionStringSetup(setup: BackboneSetup, connectionString: string): BackboneSetup {
  if (!connectionString) return setup;
  const previous = setup.connection_string;
  const swap = (command: string) => (previous && previous !== connectionString ? command.split(previous).join(connectionString) : command);
  return {
    ...setup,
    connection_string: connectionString,
    steps: setup.steps.map((step) => ({ ...step, commands: step.commands.map(swap) })),
  };
}

/** Panel URL hint for self-hosted setups. */
export function panelUrlHint(panelUrl: string | null | undefined): string {
  const url = panelUrl?.trim();
  if (!url) return 'http://127.0.0.1:8787/backbone';
  return url;
}
