import { describe, expect, it } from 'vitest';
import {
  BACKBONE_EXPOSURE_GUIDE_URL,
  EXPOSURE_ALTERNATIVES,
  OPENFLARE_LAUNCH_NOTE,
  OPENFLARE_URL,
  buildExposureAdvice,
  buildFallbackExposure,
  buildFallbackSetup,
  buildLoopbackBindCommand,
  exposureStepInsertIndex,
  isLoopbackAddress,
  normalizeBackboneExposure,
  normalizeBackboneSetup,
  portFromListen,
} from '../backbone';
import { exposureCopyLines } from '../../screens/backbone/BackboneExposure';
import { createDemoDataset } from '../mock/data';

const CONTRACT_EXPOSure = {
  recommended: 'openflare',
  provider: {
    name: 'Openflare',
    url: 'https://github.com/martintondel02/openflare',
    description:
      'Self-hosted tunnel, reverse proxy and control plane with outbound-only connectors and one public edge.',
  },
  when_to_use: 'Your backbone is behind NAT/CGNAT, or you do not want to open an inbound port.',
  steps: [
    'bind the backbone to 127.0.0.1',
    'create a TCP tunnel to 127.0.0.1:9443',
    'set public_host to the tunnel hostname',
    'share the connection string',
  ],
  alternatives: ['WireGuard', 'Tailscale', 'Cloudflare Tunnel', 'Caddy/nginx stream TLS'],
};

describe('exposure: contract payloads', () => {
  it('normalises the documented §8 exposure object', () => {
    const exposure = normalizeBackboneExposure(CONTRACT_EXPOSure);
    expect(exposure.recommended).toBe('openflare');
    expect(exposure.provider).toMatchObject({ name: 'Openflare', url: OPENFLARE_URL });
    expect(exposure.provider?.description).toContain('outbound-only');
    expect(exposure.when_to_use).toContain('NAT/CGNAT');
    expect(exposure.steps).toHaveLength(4);
    expect(exposure.alternatives).toEqual([...EXPOSURE_ALTERNATIVES]);
  });

  it('is backwards compatible when the bridge omits exposure or sends junk', () => {
    const fallback = buildFallbackExposure(9443, 'mesh.example.org');
    expect(normalizeBackboneExposure(undefined, fallback)).toBe(fallback);
    expect(normalizeBackboneExposure(null, fallback)).toBe(fallback);
    const partial = normalizeBackboneExposure({ recommended: 'tailscale' }, fallback);
    expect(partial.recommended).toBe('tailscale');
    expect(partial.provider?.name).toBe('Openflare');
    expect(partial.alternatives?.length).toBeGreaterThan(3);
    expect(partial.steps?.length).toBeGreaterThan(0);
    const junkProvider = normalizeBackboneExposure({ provider: { name: 'X', url: 'javascript:alert(1)' } }, fallback);
    expect(junkProvider.provider?.url).toBe(OPENFLARE_URL);
  });

  it('builds a complete local fallback with the Openflare launch note', () => {
    const exposure = buildFallbackExposure(9555, 'mesh.example.org');
    expect(exposure.provider?.name).toBe('Openflare');
    expect(exposure.provider?.url).toBe(OPENFLARE_URL);
    expect(exposure.steps?.join(' ')).toContain('127.0.0.1:9555');
    expect(exposure.bind_command).toBe(
      'openmesh-bridge backbone --bind 127.0.0.1 --port 9555 --public-host mesh.example.org',
    );
    expect(exposure.guide_url).toBe(BACKBONE_EXPOSURE_GUIDE_URL);
    expect(OPENFLARE_LAUNCH_NOTE).toContain('prepared for public launch');
  });

  it('attaches exposure when normalising the setup payload and falls back when absent', () => {
    const config = {
      enabled: true,
      running: true,
      bind: '127.0.0.1',
      port: 9443,
      auth: 'token' as const,
      token_set: true,
      max_peers: 32,
      allowlist: [],
      store_forward: true,
      name: 'oslo',
      public_host: 'mesh.example.org',
      ping_interval_s: 30,
    };
    const fallback = buildFallbackSetup({ config, connectionString: 'openmesh://backbone?host=mesh.example.org' });
    const normalized = normalizeBackboneSetup(
      {
        listen: '127.0.0.1:9443',
        public_host: 'mesh.example.org',
        connection_string: 'openmesh://backbone?host=mesh.example.org',
        steps: [{ title: 'Install', commands: ['true'] }],
        exposure: CONTRACT_EXPOSure,
      },
      fallback,
    );
    expect(normalized.exposure?.provider?.name).toBe('Openflare');
    expect(normalized.exposure?.steps).toHaveLength(4);
    const oldBridge = normalizeBackboneSetup({ listen: '127.0.0.1:9443', steps: [{ title: 'Install', commands: ['true'] }] }, fallback);
    expect(oldBridge.exposure).toEqual(fallback.exposure);
  });
});

describe('exposure: bind command and advice', () => {
  it('generates the loopback bind command with sensible placeholders', () => {
    expect(buildLoopbackBindCommand()).toBe(
      'openmesh-bridge backbone --bind 127.0.0.1 --port 9443 --public-host <public-host>',
    );
    expect(buildLoopbackBindCommand({ port: 9555, publicHost: 'mesh.example.org' })).toBe(
      'openmesh-bridge backbone --bind 127.0.0.1 --port 9555 --public-host mesh.example.org',
    );
    expect(buildLoopbackBindCommand({ publicHost: '  spaced host  ' })).toContain('--public-host spaced');
    expect(buildLoopbackBindCommand({ port: Number.NaN })).toContain('--port 9443');
  });

  it('parses ports and loopback addresses', () => {
    expect(portFromListen('0.0.0.0:9443')).toBe(9443);
    expect(portFromListen('127.0.0.1:443')).toBe(443);
    expect(portFromListen('nonsense')).toBe(9443);
    expect(portFromListen('host:99999')).toBe(9443);
    expect(isLoopbackAddress('127.0.0.1')).toBe(true);
    expect(isLoopbackAddress('localhost')).toBe(true);
    expect(isLoopbackAddress('::1')).toBe(true);
    expect(isLoopbackAddress('[::1]')).toBe(true);
    expect(isLoopbackAddress('127.4.5.6')).toBe(true);
    expect(isLoopbackAddress('mesh.example.org')).toBe(false);
    expect(isLoopbackAddress(null)).toBe(false);
  });

  it('recommends a tunnel when the backbone is loopback-only without a public host', () => {
    const advice = buildExposureAdvice({
      bind: '127.0.0.1',
      port: 9443,
      publicHost: '',
      exposure: normalizeBackboneExposure(CONTRACT_EXPOSure),
    });
    expect(advice.state).toBe('tunnel');
    expect(advice.badge).toBe('tunnel recommended');
    expect(advice.providerName).toBe('Openflare');
    expect(advice.providerUrl).toBe(OPENFLARE_URL);
    expect(advice.bindCommand).toContain('--bind 127.0.0.1');
    expect(advice.body).toContain('NAT/CGNAT');
    expect(advice.guideUrl).toContain('docs/guides/backbone-exposure.md');
  });

  it('reports a configured public endpoint when public_host is set', () => {
    const advice = buildExposureAdvice({ bind: '0.0.0.0', port: 9443, publicHost: 'mesh.example.org' });
    expect(advice.state).toBe('public');
    expect(advice.badge).toBe('public endpoint configured');
    expect(advice.publicEndpoint).toBe('mesh.example.org:9443');
    expect(advice.title).toContain('mesh.example.org');
  });

  it('warns when binding every interface without a public host', () => {
    const advice = buildExposureAdvice({ bind: '0.0.0.0', port: 9443, publicHost: '', exposure: CONTRACT_EXPOSure });
    expect(advice.state).toBe('bound');
    expect(advice.badge).toBe('bound to all interfaces');
    expect(advice.body).toContain('firewall or VPN');
  });

  it('inserts the exposure step directly after the firewall step', () => {
    const config = { enabled: true, running: true, bind: '0.0.0.0', port: 9443, auth: 'token' as const, token_set: true, max_peers: 32, allowlist: [], store_forward: true, name: 'x', public_host: '', ping_interval_s: 30 };
    const setup = buildFallbackSetup({ config, connectionString: 'openmesh://backbone?x=1' });
    const firewallIndex = setup.steps.findIndex((step) => /firewall/i.test(step.title));
    expect(firewallIndex).toBeGreaterThanOrEqual(0);
    expect(exposureStepInsertIndex(setup.steps)).toBe(firewallIndex + 1);
    expect(exposureStepInsertIndex([])).toBe(0);
  });

  it('includes the exposure section in copy-all output', () => {
    const lines = exposureCopyLines(
      normalizeBackboneExposure(CONTRACT_EXPOSure),
      '127.0.0.1:9443',
      'mesh.example.org',
    ).join('\n');
    expect(lines).toContain('Expose it publicly (optional)');
    expect(lines).toContain('Openflare');
    expect(lines).toContain('--bind 127.0.0.1');
    expect(lines).toContain('docs/guides/backbone-exposure.md');
  });
});

describe('exposure: demo payload', () => {
  it('seeds the same exposure object in demo mode without a real token', () => {
    const dataset = createDemoDataset(new Date(2026, 2, 15, 12, 0, 0).getTime());
    const exposure = dataset.backbone.setup.exposure;
    expect(exposure).toBeTruthy();
    expect(exposure?.recommended).toBe('openflare');
    expect(exposure?.provider?.name).toBe('Openflare');
    expect(exposure?.provider?.url).toBe(OPENFLARE_URL);
    expect(exposure?.when_to_use).toContain('NAT/CGNAT');
    expect(exposure?.steps?.length).toBeGreaterThanOrEqual(4);
    expect(exposure?.alternatives).toEqual(expect.arrayContaining(['WireGuard', 'Tailscale']));
    expect(exposure?.guide_url).toContain('docs/guides/backbone-exposure.md');
    expect(dataset.backbone.token).toContain('demo-');
    expect(dataset.backbone.setup.public_host).toBe(dataset.backbone.config.public_host);
    const advice = buildExposureAdvice({
      bind: dataset.backbone.config.bind,
      port: dataset.backbone.config.port,
      publicHost: dataset.backbone.config.public_host,
      exposure,
    });
    expect(advice.state).toBe('public');
  });
});
