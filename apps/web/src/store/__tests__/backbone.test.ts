import { describe, expect, it } from 'vitest';
import {
  selectBackboneAvailable,
  selectBackboneConnectionString,
  selectBackboneDemo,
  selectBackboneHeader,
  selectBackbonePeers,
  selectBackboneSetup,
  selectBackboneStats,
} from '../backbone-selectors';
import type { BackboneConfig, BackbonePeer, BackboneSetup, BackboneStatus } from '../../types/backbone';
import { buildBackboneConnectionString } from '../../lib/backbone';
import type { HelloData } from '../../types/api';

const NODE_A = '4a5b6c7d8e9f0a1b';
const NODE_B = '8f7e6d5c4b3a2019';
const TOKEN = 'test-token-value';

function config(overrides: Partial<BackboneConfig> = {}): BackboneConfig {
  return {
    enabled: true,
    running: true,
    bind: '0.0.0.0',
    port: 9443,
    auth: 'token',
    token_set: true,
    max_peers: 32,
    allowlist: [],
    store_forward: true,
    name: 'oslo-backbone',
    public_host: 'mesh.example.org',
    ping_interval_s: 30,
    ...overrides,
  };
}

function peer(nodeId: string, overrides: Partial<BackbonePeer> = {}): BackbonePeer {
  return {
    node_id: nodeId,
    name: 'lofoten',
    direction: 'inbound',
    remote_addr: '203.0.113.5:50000',
    connected_at_ms: 1000,
    last_seen_ms: 2000,
    frames_in: 10,
    frames_out: 8,
    bytes_in: 1000,
    bytes_out: 800,
    rtt_ms: 38,
    app_version: '0.2.0',
    auth: 'token',
    ...overrides,
  };
}

function status(overrides: Partial<BackboneStatus> = {}): BackboneStatus {
  return {
    running: true,
    peers: 2,
    uptime_s: 3600,
    frames_in: 1042,
    frames_out: 998,
    bytes_in: 250_080,
    bytes_out: 239_520,
    throughput_in_bps: 812,
    throughput_out_bps: 764,
    store_forward_envelopes: 3,
    auth_failures: 0,
    last_error: null,
    ...overrides,
  };
}

function setup(overrides: Partial<BackboneSetup> = {}): BackboneSetup {
  return {
    listen: '0.0.0.0:9443',
    public_host: 'mesh.example.org',
    connection_string: 'openmesh://backbone?host=mesh.example.org&port=9443&v=1',
    token_set: true,
    auth: 'token',
    steps: [{ title: 'Install', commands: ['curl install.sh | sh'], note: null }],
    systemd_unit: '[Unit]',
    firewall: { firewalld: [], ufw: [] },
    notes: [],
    panel_url: 'http://127.0.0.1:8787/backbone',
    ...overrides,
  };
}

function state(overrides: Record<string, unknown> = {}) {
  return {
    mode: 'live' as const,
    hello: null as HelloData | null,
    ownNodeId: 'a1b2c3d4e5f60718',
    backboneConfig: config() as BackboneConfig | null,
    backboneStatus: status() as BackboneStatus | null,
    backbonePeers: [peer(NODE_B), peer(NODE_A, { direction: 'outbound', name: 'tromso' })] as BackbonePeer[],
    backboneSetup: setup() as BackboneSetup | null,
    backboneSessionToken: TOKEN as string | null,
    ...overrides,
  };
}

describe('backbone selectors: availability and demo flag', () => {
  it('shows the nav item for demo mode, advertised features or an enabled backbone', () => {
    expect(selectBackboneAvailable(state())).toBe(true);
    expect(selectBackboneAvailable(state({ demo: true }))).toBe(true);
    expect(selectBackboneAvailable(state({ backboneConfig: null, mode: 'live', hello: null }))).toBe(false);
    const hello = { features: ['backbone'] } as HelloData;
    expect(selectBackboneAvailable(state({ backboneConfig: null, hello }))).toBe(true);
  });

  it('reports demo mode', () => {
    expect(selectBackboneDemo({ mode: 'demo' })).toBe(true);
    expect(selectBackboneDemo({ mode: 'live' })).toBe(false);
  });
});

describe('backbone selectors: stats', () => {
  it('prefers live status counters', () => {
    const stats = selectBackboneStats(state());
    expect(stats.peers).toBe(2);
    expect(stats.maxPeers).toBe(32);
    expect(stats.framesIn).toBe(1042);
    expect(stats.framesOut).toBe(998);
    expect(stats.bytesIn).toBe(250_080);
    expect(stats.throughputInBps).toBe(812);
    expect(stats.storeForwardEnvelopes).toBe(3);
    expect(stats.running).toBe(true);
  });

  it('falls back to config totals, then peer sums, before the first status event', () => {
    const withTotals = selectBackboneStats(
      state({
        backboneStatus: null,
        backboneConfig: config({ totals: { frames_in: 50, frames_out: 40, bytes_in: 5000, bytes_out: 4000 } }),
      }),
    );
    expect(withTotals.framesIn).toBe(50);
    expect(withTotals.bytesOut).toBe(4000);

    const peersOnly = selectBackboneStats(state({ backboneStatus: null, backboneConfig: config() }));
    expect(peersOnly.framesIn).toBe(20);
    expect(peersOnly.bytesIn).toBe(2000);
    expect(peersOnly.authFailures).toBe(0);
    expect(peersOnly.uptimeS).toBe(0);
  });

  it('derives the header pill, endpoint and listen address', () => {
    const header = selectBackboneHeader(state());
    expect(header.pill.phase).toBe('running');
    expect(header.endpoint).toBe('mesh.example.org:9443');
    expect(header.listen).toBe('0.0.0.0:9443');

    const openHeader = selectBackboneHeader(
      state({ backboneConfig: config({ auth: 'open' }), backboneStatus: status() }),
    );
    expect(openHeader.pill.phase).toBe('running-open');

    const stopped = selectBackboneHeader(state({ backboneStatus: status({ running: false }) }));
    expect(stopped.pill.phase).toBe('stopped');
  });
});

describe('backbone selectors: connection string and setup', () => {
  it('builds a tokenised connection string when the rotated token is in memory', () => {
    const connection = selectBackboneConnectionString(state());
    expect(connection).toContain('openmesh://backbone?');
    expect(connection).toContain(`token=${encodeURIComponent(TOKEN)}`);
    expect(connection).toContain('node=a1b2c3d4e5f60718');
    expect(connection).toContain('port=9443');
  });

  it('falls back to the setup payload when the token is not in memory', () => {
    const fallback = 'openmesh://backbone?host=mesh.example.org&port=9443&v=1';
    const connection = selectBackboneConnectionString(
      state({ backboneSessionToken: null, backboneSetup: setup({ connection_string: fallback }) }),
    );
    expect(connection).toBe(fallback);
  });

  it('omits the token for open auth and returns null with no config or setup', () => {
    const open = selectBackboneConnectionString(
      state({ backboneConfig: config({ auth: 'open' }), backboneSetup: setup() }),
    );
    expect(open).not.toContain('token=');
    expect(selectBackboneConnectionString(state({ backboneConfig: null, backboneSetup: null }))).toBeNull();
  });

  it('produces an effective setup with the current connection string in its steps', () => {
    const effective = selectBackboneSetup(state());
    expect(effective.connection_string).toContain('openmesh://backbone?');
    expect(effective.connection_string).toContain(TOKEN);

    const fallbackOnly = selectBackboneSetup(state({ backboneSetup: null }));
    expect(fallbackOnly.steps.length).toBeGreaterThanOrEqual(5);
    expect(fallbackOnly.firewall.firewalld.join(' ')).toContain('9443/tcp');
    expect(fallbackOnly.panel_url).toContain('/backbone');

    const rotated = buildBackboneConnectionString({
      host: 'mesh.example.org',
      port: 9443,
      token: 'NEW',
      nodeId: 'a1b2c3d4e5f60718',
      name: 'oslo-backbone',
    });
    const swapped = selectBackboneSetup(
      state({
        backboneSetup: setup({
          connection_string: 'openmesh://backbone?token=OLD',
          steps: [{ title: 'Connect a gateway', commands: ["openmesh-bridge join 'openmesh://backbone?token=OLD'"] }],
        }),
        backboneSessionToken: 'NEW',
      }),
    );
    // Setup steps that referenced OLD are swapped to the effective string (NEW).
    expect(swapped.connection_string).toBe(rotated);
    expect(swapped.steps[0]!.commands.join(' ')).toContain('NEW');
    expect(swapped.steps[0]!.commands.join(' ')).not.toContain('OLD');
  });

  it('sorts peers with inbound links first', () => {
    const peers = selectBackbonePeers(state());
    expect(peers.map((entry) => entry.direction)).toEqual(['inbound', 'outbound']);
  });
});
