import { describe, expect, it } from 'vitest';
import {
  INSTALL_OPTIONS,
  addAllowlistEntry,
  applyPeerEvent,
  backbonePill,
  buildBackboneConnectionString,
  buildFallbackFirewall,
  buildFallbackSetup,
  buildFallbackSystemdUnit,
  connectionStringHasToken,
  connectionStringToken,
  defaultBackboneConfig,
  diffBackboneConfig,
  formatAuthMode,
  formatCount,
  formatBackboneUptime,
  formatEndpoint,
  formatListenAddress,
  formatPeerDirection,
  formatRtt,
  formatThroughput,
  hasBackboneChanges,
  isBackboneRelevant,
  maskToken,
  mergeBackboneConfig,
  normalizeBackboneConfig,
  normalizeBackboneSetup,
  redactConnectionString,
  removeAllowlistEntry,
  rttTone,
  sortPeers,
  sumPeerTraffic,
  validateAllowlistEntry,
  withConnectionStringSetup,
} from '../backbone';
import type { BackbonePeer } from '../../types/backbone';

// Test-only fixture, deliberately not a realistic bearer secret.
const TOKEN = 'TEST-ONLY-token-not-a-real-secret';
const NODE_A = '4a5b6c7d8e9f0a1b';
const NODE_B = '8f7e6d5c4b3a2019';

function peer(nodeId: string, overrides: Partial<BackbonePeer> = {}): BackbonePeer {
  return {
    node_id: nodeId,
    name: 'peer',
    direction: 'inbound',
    remote_addr: '203.0.113.5:51000',
    connected_at_ms: 1_000,
    last_seen_ms: 2_000,
    frames_in: 10,
    frames_out: 8,
    bytes_in: 1000,
    bytes_out: 800,
    rtt_ms: 40,
    app_version: '0.2.0',
    auth: 'token',
    ...overrides,
  };
}

describe('backbone: connection strings', () => {
  it('builds the documented openmesh://backbone URL', () => {
    const url = buildBackboneConnectionString({
      host: 'mesh.example.org',
      port: 9443,
      token: TOKEN,
      nodeId: NODE_A,
      name: 'oslo-backbone',
    });
    expect(url.startsWith('openmesh://backbone?')).toBe(true);
    expect(url).toContain('host=mesh.example.org');
    expect(url).toContain('port=9443');
    expect(url).toContain(`token=${encodeURIComponent(TOKEN)}`);
    expect(url).toContain(`node=${NODE_A}`);
    expect(url).toContain('v=1');
    expect(connectionStringToken(url)).toBe(TOKEN);
    expect(connectionStringHasToken(url)).toBe(true);
  });

  it('omits the token in open mode (BACKBONE.md §6)', () => {
    const url = buildBackboneConnectionString({ host: '10.0.0.5', port: 9443, token: null, name: 'lan' });
    expect(url).not.toContain('token=');
    expect(connectionStringToken(url)).toBeNull();
    expect(connectionStringHasToken(url)).toBe(false);
  });

  it('redacts the token but keeps the rest of the URL readable', () => {
    const url = buildBackboneConnectionString({ host: 'mesh.example.org', port: 9443, token: TOKEN, name: 'x' });
    const redacted = redactConnectionString(url);
    expect(redacted).not.toContain(TOKEN);
    expect(redacted).toContain('host=mesh.example.org');
    expect(redacted).toContain('token=••••');
    // Non-token URLs pass through unchanged.
    const open = buildBackboneConnectionString({ host: 'h', port: 1 });
    expect(redactConnectionString(open)).toBe(open);
  });

  it('masks tokens for display', () => {
    expect(maskToken(null)).toBe('—');
    expect(maskToken('abcdef123456')).toBe('abcdef••••••');
    expect(maskToken('short')).toBe('•••••');
  });

  it('tolerates malformed connection strings', () => {
    expect(() => connectionStringToken('not a url')).not.toThrow();
    expect(connectionStringToken('not a url')).toBeNull();
  });
});

describe('backbone: formatting', () => {
  it('formats throughput from bits per second', () => {
    expect(formatThroughput(null)).toBe('—');
    expect(formatThroughput(0)).toBe('0 b/s');
    expect(formatThroughput(812)).toBe('812 b/s');
    expect(formatThroughput(8_240)).toBe('8.2 kb/s');
    expect(formatThroughput(2_400_000)).toBe('2.4 Mb/s');
  });

  it('compacts large counters for stat cards', () => {
    expect(formatCount(987)).toBe('987');
    expect(formatCount(1_842_112)).toBe('1.84M');
    expect(formatCount(42_100)).toBe('42k');
    expect(formatCount(null)).toBe('—');
  });

  it('formats uptime for the status strip', () => {
    expect(formatBackboneUptime(42)).toBe('42s');
    expect(formatBackboneUptime(3_720)).toBe('1h 2m');
    expect(formatBackboneUptime(1_234_567)).toBe('14d 6h');
    expect(formatBackboneUptime(null)).toBe('—');
  });

  it('formats endpoints, directions, auth and RTT', () => {
    expect(formatEndpoint({ public_host: 'mesh.example.org', bind: '0.0.0.0', port: 9443 })).toBe(
      'mesh.example.org:9443',
    );
    expect(formatEndpoint({ public_host: '', bind: '0.0.0.0', port: 9443 })).toBe('0.0.0.0:9443');
    expect(formatListenAddress({ bind: '127.0.0.1', port: 9443 })).toBe('127.0.0.1:9443');
    expect(formatPeerDirection('inbound')).toBe('Inbound');
    expect(formatPeerDirection('outbound')).toBe('Outbound');
    expect(formatAuthMode('open')).toContain('Open');
    expect(formatAuthMode('token')).toContain('HMAC');
    expect(formatRtt(38.4)).toBe('38 ms');
    expect(formatRtt(0)).toBe('—');
    expect(rttTone(20)).toBe('good');
    expect(rttTone(120)).toBe('fair');
    expect(rttTone(400)).toBe('poor');
    expect(rttTone(null)).toBe('unknown');
  });
});

describe('backbone: status pill and feature gating', () => {
  const base = defaultBackboneConfig({ enabled: true, running: true });

  it('reflects running, stopped, disabled and open states', () => {
    expect(backbonePill(base, { running: true }).phase).toBe('running');
    expect(backbonePill(base, { running: false }).phase).toBe('stopped');
    expect(backbonePill(defaultBackboneConfig({ enabled: false }), null).phase).toBe('disabled');
    expect(backbonePill({ ...base, auth: 'open' }, { running: true })).toMatchObject({
      phase: 'running-open',
      tone: 'rose',
    });
    expect(backbonePill(null, null).phase).toBe('loading');
  });

  it('shows the nav item when the bridge advertises backbone or has it enabled', () => {
    expect(isBackboneRelevant({ features: ['backbone'], config: null, demo: false })).toBe(true);
    expect(isBackboneRelevant({ features: [], config: { enabled: true }, demo: false })).toBe(true);
    expect(isBackboneRelevant({ features: [], config: { enabled: false }, demo: false })).toBe(false);
    expect(isBackboneRelevant({ features: [], config: null, demo: true })).toBe(true);
  });
});

describe('backbone: config normalization and diffing', () => {
  it('normalizes the documented snapshot and the {enabled:false} form', () => {
    const normalized = normalizeBackboneConfig({
      enabled: true,
      bind: '0.0.0.0',
      port: 'not-a-number',
      auth: 'open',
      token_set: true,
      max_peers: 64,
      allowlist: [NODE_A.toUpperCase(), NODE_A],
      store_forward: false,
      name: 'oslo',
      public_host: 'mesh.example.org',
      ping_interval_s: 20,
      extra_unknown_field: 'ignored',
    });
    expect(normalized.port).toBe(9443);
    expect(normalized.auth).toBe('open');
    expect(normalized.max_peers).toBe(64);
    expect(normalized.allowlist).toEqual([NODE_A, NODE_A]);
    expect(normalized.store_forward).toBe(false);
    expect(normalized.running).toBe(true);

    const disabled = normalizeBackboneConfig({ enabled: false });
    expect(disabled.enabled).toBe(false);
    expect(disabled.running).toBe(false);
    expect(disabled.port).toBe(9443);
  });

  it('merges patches and computes minimal diffs', () => {
    const base = defaultBackboneConfig({ allowlist: [NODE_A] });
    const merged = mergeBackboneConfig(base, { enabled: true, port: 9444, allowlist: [NODE_B] });
    expect(merged.enabled).toBe(true);
    expect(merged.port).toBe(9444);
    expect(merged.allowlist).toEqual([NODE_B]);

    const empty = diffBackboneConfig(base, { ...base, allowlist: [NODE_A.toUpperCase()] });
    expect(empty).toEqual({});
    expect(hasBackboneChanges(base, { ...base })).toBe(false);

    const draft = { ...base, name: 'renamed', allowlist: [NODE_A, NODE_B] };
    const patch = diffBackboneConfig(base, draft);
    expect(patch.name).toBe('renamed');
    expect(patch.allowlist).toEqual([NODE_A, NODE_B]);
    expect(patch.port).toBeUndefined();
    expect(hasBackboneChanges(base, draft)).toBe(true);
  });

  it('validates allowlist entries and de-duplicates them', () => {
    expect(validateAllowlistEntry(NODE_A)).toEqual({ ok: true, nodeId: NODE_A });
    expect(validateAllowlistEntry('ZZZZ').ok).toBe(false);
    expect(validateAllowlistEntry('abc').ok).toBe(false);
    expect(addAllowlistEntry([], ` ${NODE_A.toUpperCase()} `)).toEqual({ allowlist: [NODE_A], error: null });
    expect(addAllowlistEntry([NODE_A], NODE_A).error).toContain('already');
    expect(removeAllowlistEntry([NODE_A, NODE_B], NODE_A.toUpperCase())).toEqual([NODE_B]);
  });
});

describe('backbone: peers', () => {
  it('applies joined/updated/left events', () => {
    let peers: BackbonePeer[] = [];
    peers = applyPeerEvent(peers, { action: 'joined', peer: peer(NODE_A, { name: 'lofoten' }) });
    expect(peers).toHaveLength(1);
    peers = applyPeerEvent(peers, {
      action: 'updated',
      peer: peer(NODE_A, { name: 'lofoten', frames_in: 99, rtt_ms: 12 }),
    });
    expect(peers).toHaveLength(1);
    expect(peers[0]!.frames_in).toBe(99);
    expect(peers[0]!.rtt_ms).toBe(12);
    peers = applyPeerEvent(peers, { action: 'joined', peer: peer(NODE_B, { direction: 'outbound', name: 'tromso' }) });
    expect(sortPeers(peers).map((entry) => entry.name)).toEqual(['lofoten', 'tromso']);
    peers = applyPeerEvent(peers, { action: 'left', peer: peer(NODE_A) });
    expect(peers.map((entry) => entry.node_id)).toEqual([NODE_B]);
  });

  it('sums per-peer traffic for stat fallbacks', () => {
    const totals = sumPeerTraffic([
      peer(NODE_A, { frames_in: 2, frames_out: 3, bytes_in: 40, bytes_out: 60 }),
      peer(NODE_B, { frames_in: 5, frames_out: 7, bytes_in: 500, bytes_out: 700 }),
    ]);
    expect(totals).toEqual({ frames_in: 7, frames_out: 10, bytes_in: 540, bytes_out: 760 });
  });
});

describe('backbone: setup payload rendering', () => {
  const config = defaultBackboneConfig({
    enabled: true,
    port: 9443,
    public_host: 'mesh.example.org',
    name: 'oslo-backbone',
    token_set: true,
  });
  const connectionString = buildBackboneConnectionString({
    host: 'mesh.example.org',
    port: 9443,
    token: TOKEN,
    name: 'oslo-backbone',
  });

  it('builds a complete fallback setup with numbered onboarding steps', () => {
    const setup = buildFallbackSetup({ config, connectionString });
    expect(setup.steps.length).toBeGreaterThanOrEqual(5);
    expect(setup.steps[0]!.title).toMatch(/install/i);
    expect(setup.steps.some((step) => /start/i.test(step.title))).toBe(true);
    expect(setup.steps.some((step) => /firewall/i.test(step.title))).toBe(true);
    expect(setup.steps.some((step) => /keep|service|systemd/i.test(step.title))).toBe(true);
    expect(setup.steps.some((step) => /connect|gateway|join/i.test(step.title))).toBe(true);
    expect(setup.steps.some((step) => /verify|test/i.test(step.title))).toBe(true);
    expect(setup.steps.every((step) => step.title.length > 0 && step.commands.length > 0)).toBe(true);
    expect(setup.firewall.firewalld.join(' ')).toContain('9443/tcp');
    expect(setup.firewall.ufw.join(' ')).toContain('9443/tcp');
    expect(setup.systemd_unit).toContain('[Unit]');
    expect(setup.listen).toBe('0.0.0.0:9443');
    expect(setup.panel_url).toContain('/backbone');
    expect(setup.notes.length).toBeGreaterThan(1);
    expect(setup.steps[0]!.commands[0]).toContain('releases/latest/download/install.sh');
    expect(setup.steps[0]!.note).toMatch(/tar\.gz/i);
  });

  it('includes the connection string in the gateway step and swaps it when needed', () => {
    const setup = buildFallbackSetup({ config, connectionString });
    const gatewayStep = setup.steps.find((step) => /connect|gateway|join/i.test(step.title))!;
    expect(gatewayStep.commands.join(' ')).toContain(connectionString);

    const rotated = buildBackboneConnectionString({ host: 'mesh.example.org', port: 9443, token: 'NEWTOKEN' });
    const swapped = withConnectionStringSetup(setup, rotated);
    expect(swapped.connection_string).toBe(rotated);
    const swappedGateway = swapped.steps.find((step) => /connect|gateway|join/i.test(step.title))!;
    expect(swappedGateway.commands.join(' ')).toContain('NEWTOKEN');
    expect(swappedGateway.commands.join(' ')).not.toContain(TOKEN);
  });

  it('normalizes partial bridge payloads and keeps safe fallbacks', () => {
    const fallback = buildFallbackSetup({ config, connectionString });
    const normalized = normalizeBackboneSetup(
      {
        listen: '0.0.0.0:9443',
        connection_string: connectionString,
        auth: 'token',
        steps: [{ title: 'Install OpenMesh', commands: ['curl install.sh | sh'], note: 'checksum' }],
        firewall: { firewalld: ['sudo firewall-cmd --add-port=9443/tcp'] },
        notes: ['payloads are E2E encrypted'],
        panel_url: 'http://127.0.0.1:8787/backbone',
      },
      fallback,
    );
    expect(normalized.steps).toHaveLength(1);
    expect(normalized.steps[0]!.note).toBe('checksum');
    expect(normalized.firewall.firewalld).toHaveLength(1);
    expect(normalized.firewall.ufw).toEqual(fallback.firewall.ufw);
    expect(normalized.systemd_unit).toBe(fallback.systemd_unit);
    expect(normalized.token_set).toBe(fallback.token_set);
    expect(normalizeBackboneSetup(null, fallback)).toEqual(fallback);
    expect(normalizeBackboneSetup({ steps: 'nope' }, fallback).steps).toEqual(fallback.steps);
  });

  it('ships install options for Linux, macOS and Windows', () => {
    expect(INSTALL_OPTIONS.map((option) => option.id)).toEqual(['linux', 'macos', 'windows']);
    for (const option of INSTALL_OPTIONS) {
      expect(option.commands.length).toBeGreaterThan(0);
      expect(option.hint.length).toBeGreaterThan(0);
      expect(option.note.length).toBeGreaterThan(0);
    }
    expect(INSTALL_OPTIONS[0]!.commands[0]).toContain('releases/latest/download/install.sh');
    expect(INSTALL_OPTIONS[2]!.commands.join(' ')).toContain('install.sh');
    expect(INSTALL_OPTIONS[2]!.note.toLowerCase()).toContain('tar.gz');
  });

  it('provides firewall and systemd fallbacks with the configured port', () => {
    expect(buildFallbackFirewall(9555).firewalld.join(' ')).toContain('9555/tcp');
    expect(buildFallbackFirewall(9555).ufw.join(' ')).toContain('9555/tcp');
    const unit = buildFallbackSystemdUnit();
    expect(unit).toContain('ExecStart=');
    expect(unit).toContain('WantedBy=default.target');
    expect(unit).toContain('NoNewPrivileges=true');
  });
});
