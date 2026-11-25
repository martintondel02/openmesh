import { describe, expect, it } from 'vitest';
import { createDemoDataset, DEMO_OWN_ID } from '../mock/data';
import { isValidNodeId } from '../utils';
import { buildBackboneConnectionString } from '../backbone';

const NOW = new Date(2026, 2, 15, 12, 0, 0).getTime();

describe('demo backbone seed: invariants', () => {
  const dataset = createDemoDataset(NOW);
  const backbone = dataset.backbone;

  it('is deterministic for a fixed clock', () => {
    const again = createDemoDataset(NOW);
    expect(JSON.stringify(again.backbone)).toBe(JSON.stringify(backbone));
  });

  it('advertises backbone support and a running seeded server', () => {
    expect(dataset.hello.features).toContain('backbone');
    expect(backbone.config.enabled).toBe(true);
    expect(backbone.config.running).toBe(true);
    expect(backbone.config.auth).toBe('token');
    expect(backbone.config.token_set).toBe(true);
    expect(backbone.config.bind).toBe('0.0.0.0');
    expect(backbone.config.port).toBe(9443);
    expect(backbone.status.running).toBe(true);
    expect(backbone.status.uptime_s).toBeGreaterThan(60 * 60);
    expect(backbone.status.throughput_in_bps).toBeGreaterThan(0);
    expect(backbone.status.throughput_out_bps).toBeGreaterThan(0);
    expect(backbone.status.auth_failures).toBeGreaterThan(0);
    expect(backbone.status.store_forward_envelopes).toBeGreaterThan(0);
  });

  it('seeds 2–3 realistic peers with live counters', () => {
    expect(backbone.peers.length).toBeGreaterThanOrEqual(2);
    expect(backbone.peers.length).toBeLessThanOrEqual(3);
    expect(backbone.status.peers).toBe(backbone.peers.length);
    expect(new Set(backbone.peers.map((peer) => peer.node_id)).size).toBe(backbone.peers.length);
    expect(backbone.peers.some((peer) => peer.direction === 'inbound')).toBe(true);
    expect(backbone.peers.some((peer) => peer.direction === 'outbound')).toBe(true);
    for (const peer of backbone.peers) {
      expect(isValidNodeId(peer.node_id)).toBe(true);
      expect(peer.name.length).toBeGreaterThan(0);
      expect(peer.remote_addr).toMatch(/:\d+$/);
      expect(peer.frames_in).toBeGreaterThan(0);
      expect(peer.frames_out).toBeGreaterThan(0);
      expect(peer.rtt_ms).toBeGreaterThan(0);
      expect(peer.app_version).toBe('0.2.0');
      expect(peer.connected_at_ms).toBeLessThanOrEqual(NOW);
    }
  });

  it('uses a clearly-fake demo token and a parseable connection string', () => {
    expect(backbone.token).toMatch(/^demo-/);
    expect(backbone.token).not.toContain('=');
    expect(backbone.token.length).toBeGreaterThan(20);

    const url = new URL(backbone.setup.connection_string);
    expect(url.protocol).toBe('openmesh:');
    expect(url.hostname).toBe('backbone');
    expect(url.searchParams.get('host')).toBe(backbone.config.public_host);
    expect(url.searchParams.get('port')).toBe(String(backbone.config.port));
    expect(url.searchParams.get('token')).toBe(backbone.token);
    expect(url.searchParams.get('node')).toBe(DEMO_OWN_ID);
    expect(url.searchParams.get('v')).toBe('1');
  });

  it('rebuilds the same connection string from the seed config', () => {
    const rebuilt = buildBackboneConnectionString({
      host: backbone.config.public_host,
      port: backbone.config.port,
      token: backbone.token,
      nodeId: DEMO_OWN_ID,
      name: backbone.config.name,
    });
    expect(backbone.setup.connection_string).toBe(rebuilt);
  });

  it('ships a complete onboarding payload for the wizard', () => {
    const { setup } = backbone;
    expect(setup.steps.length).toBeGreaterThanOrEqual(5);
    expect(setup.steps.map((step) => step.title.toLowerCase()).join(' ')).toContain('install');
    expect(setup.steps.map((step) => step.title.toLowerCase()).join(' ')).toContain('firewall');
    expect(setup.steps.map((step) => step.commands.join(' ')).join(' ')).toContain('openmesh-bridge join');
    expect(setup.firewall.firewalld.join(' ')).toContain('9443/tcp');
    expect(setup.firewall.ufw.join(' ')).toContain('9443/tcp');
    expect(setup.systemd_unit).toContain('[Unit]');
    expect(setup.panel_url).toContain('/backbone');
    expect(setup.notes.length).toBeGreaterThan(0);
    expect(setup.auth).toBe('token');
    expect(setup.token_set).toBe(true);
  });
});
