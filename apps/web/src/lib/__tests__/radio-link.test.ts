import { describe, expect, it } from 'vitest';
import {
  COMPOSER_RADIO_CONNECTING_WARNING,
  COMPOSER_RADIO_OFFLINE_WARNING,
  COMPOSER_RADIO_RECONNECTING_WARNING,
  composerRadioNotice,
  composerRadioWarning,
  describeRadioLink,
  formatLastFrameAge,
  isLiveRadioOffline,
  isRadioLinkStateValue,
  radioGuidance,
  radioLinkText,
  radioLinkTone,
  radioOfflineBanner,
  reconnectAttemptLabel,
  shouldShowComposerRadioWarning,
  shouldShowRadioOfflineBanner,
  statusLabel,
  transportState,
  transportStateLabel,
} from '../radio-link';
import { bannedTermsInCopy } from '../mode';
import type { HelloData, HelloRadio, RadioLinkStateValue, StatusData } from '../../types/api';

function radio(overrides: Partial<HelloRadio> = {}): HelloRadio {
  return {
    id: 'serial-1',
    kind: 'serial',
    label: 'Serial /dev/ttyACM0 @115200',
    connected: true,
    state: 'connected',
    profile: 'MEDIUM_FAST',
    region: 'EU868',
    ...overrides,
  };
}

function simRadio(overrides: Partial<HelloRadio> = {}): HelloRadio {
  return radio({ id: 'sim0', kind: 'sim', label: 'Simulated SX1262', state: 'virtual', ...overrides });
}

function hello(radios: HelloRadio[]): HelloData {
  return {
    bridge_version: '0.7.1',
    protocol: 'omp/1',
    node: { node_id: '6d2dfb7be7948c4b', name: 'PC Bridge', class: 'gateway', caps: 27 },
    radios,
    features: [],
    store: { messages: 0, contacts: 0, channels: 0 },
  };
}

function status(overrides: Partial<StatusData['radio']> = {}): StatusData {
  return {
    uptime_s: 60,
    radio: {
      connected: true,
      state: 'connected',
      last_rx_ms: 1_700_000_000_000,
      reconnect_attempts: 0,
      tx_queue: 0,
      airtime_used_pct: 1.2,
      rssi: -87,
      snr: 6.5,
      frequency_mhz: 869.525,
      tx_power_dbm: 14,
      ...overrides,
    },
    mesh: {
      tx_frames: 0,
      rx_frames: 0,
      forwarded: 0,
      dropped: 0,
      known_nodes: 0,
      stored_envelopes: 0,
      duty_cycle_pct: 0,
    },
    storage: { used_bytes: 0, db_path: '/tmp/bridge.redb' },
  };
}

describe('radio link: state', () => {
  it('reports a connected serial radio from status + hello', () => {
    const link = describeRadioLink(hello([radio()]), status());
    expect(link.state).toBe('connected');
    expect(link.connectedCount).toBe(1);
    expect(link.totalCount).toBe(1);
    expect(link.primary?.label).toBe('Serial /dev/ttyACM0 @115200');
    expect(link.lastRxMs).toBe(1_700_000_000_000);
    expect(link.reconnectAttempts).toBe(0);
  });

  it('maps every bridge-reported state', () => {
    for (const state of ['connected', 'connecting', 'reconnecting', 'disconnected', 'virtual'] as const) {
      expect(describeRadioLink(hello([radio()]), status({ state })).state).toBe(state);
    }
  });

  it('falls back to connected on bridges that predate the state field', () => {
    const legacy = status();
    delete (legacy.radio as { state?: unknown }).state;
    expect(describeRadioLink(hello([radio()]), legacy).state).toBe('connected');
    const down = status({ connected: false });
    delete (down.radio as { state?: unknown }).state;
    expect(describeRadioLink(hello([radio({ connected: false })]), down).state).toBe('disconnected');
  });

  it('summarises hello transports before the first status frame, like the bridge', () => {
    expect(describeRadioLink(hello([radio()]), null).state).toBe('connected');
    expect(describeRadioLink(hello([radio({ state: 'connecting', connected: false })]), null).state).toBe('connecting');
    expect(describeRadioLink(hello([radio({ state: 'reconnecting', connected: false })]), null).state).toBe(
      'reconnecting',
    );
    expect(describeRadioLink(hello([radio({ state: 'disconnected', connected: false })]), null).state).toBe(
      'disconnected',
    );
    expect(describeRadioLink(hello([]), null).state).toBe('disconnected');
  });

  it('treats a bridge with only virtual transports as virtual, not offline', () => {
    expect(describeRadioLink(hello([simRadio()]), null).state).toBe('virtual');
    expect(describeRadioLink(hello([simRadio()]), status({ state: 'virtual' })).state).toBe('virtual');
    // A configured-but-stopped backbone is still a virtual radio (no serial link).
    expect(
      describeRadioLink(hello([radio({ id: 'bb', kind: 'backbone', label: 'Backbone', connected: false, state: 'disconnected' })]), null)
        .state,
    ).toBe('virtual');
  });

  it('reports unknown only when nothing has arrived at all', () => {
    expect(describeRadioLink(null, null).state).toBe('unknown');
    expect(describeRadioLink(undefined, undefined).state).toBe('unknown');
  });

  it('ignores malformed new fields instead of inventing data', () => {
    const link = describeRadioLink(hello([radio()]), status({ last_rx_ms: null, reconnect_attempts: 0 }));
    expect(link.lastRxMs).toBeNull();
    expect(link.reconnectAttempts).toBe(0);
    const malformed = status();
    delete (malformed.radio as { last_rx_ms?: unknown }).last_rx_ms;
    expect(describeRadioLink(hello([radio()]), malformed).lastRxMs).toBeNull();
  });

  it('handles several radios gracefully and prefers a connected serial radio', () => {
    const link = describeRadioLink(
      hello([
        simRadio(),
        radio({ id: 'usb0', label: 'Heltec Wireless Tracker', connected: false, state: 'disconnected' }),
        radio({ id: 'serial-2', label: 'RAK4631', connected: true }),
      ]),
      status(),
    );
    expect(link.state).toBe('connected');
    expect(link.connectedCount).toBe(2);
    expect(link.totalCount).toBe(3);
    expect(link.primary?.id).toBe('serial-2');
    expect(link.disconnectedRadios.map((item) => item.id)).toEqual(['usb0']);
  });

  it('keeps the reconnect attempt count and last receive timestamp', () => {
    const link = describeRadioLink(
      hello([radio({ connected: false, state: 'reconnecting' })]),
      status({ state: 'reconnecting', connected: false, last_rx_ms: 1_700_000_000_000, reconnect_attempts: 3 }),
    );
    expect(link.state).toBe('reconnecting');
    expect(link.reconnectAttempts).toBe(3);
    expect(link.lastRxMs).toBe(1_700_000_000_000);
  });

  it('accepts only the documented state strings', () => {
    expect(isRadioLinkStateValue('reconnecting')).toBe(true);
    expect(isRadioLinkStateValue('offline')).toBe(false);
    expect(isRadioLinkStateValue(undefined)).toBe(false);
  });
});

describe('radio link: text and tone', () => {
  it('uses plain status words per state that never depend on colour', () => {
    const label = (state: RadioLinkStateValue) =>
      statusLabel(describeRadioLink(hello([radio()]), status({ state })));
    expect(label('connected')).toBe('Radio connected');
    expect(label('connecting')).toBe('Radio connecting…');
    expect(label('reconnecting')).toBe('Radio reconnecting');
    expect(label('disconnected')).toBe('Radio disconnected');
    expect(label('virtual')).toBe('Virtual radio (simulator)');
    expect(statusLabel(describeRadioLink(null, null))).toBe('Radio status unknown');
  });

  it('shows the reconnect attempt count only after the first retry', () => {
    const at = (attempts: number) =>
      describeRadioLink(
        hello([radio({ connected: false, state: 'reconnecting' })]),
        status({ state: 'reconnecting', connected: false, reconnect_attempts: attempts }),
      );
    expect(statusLabel(at(1))).toBe('Radio reconnecting');
    expect(statusLabel(at(2))).toBe('Radio reconnecting (attempt 2)');
    expect(radioLinkText(at(4)).statusLabel).toBe('Radio reconnecting (attempt 4)');
  });

  it('names virtual radios by their transport kind', () => {
    expect(
      statusLabel(
        describeRadioLink(hello([simRadio()]), status({ state: 'virtual' })),
      ),
    ).toBe('Virtual radio (simulator)');
    expect(
      statusLabel(
        describeRadioLink(
          hello([radio({ id: 'udp', kind: 'udp', label: 'UDP LAN link', state: 'virtual' })]),
          status({ state: 'virtual' }),
        ),
      ),
    ).toBe('Virtual radio (LAN link)');
    expect(
      statusLabel(
        describeRadioLink(
          hello([radio({ id: 'bb', kind: 'backbone', label: 'Backbone', state: 'virtual' })]),
          status({ state: 'virtual' }),
        ),
      ),
    ).toBe('Virtual radio (backbone)');
  });

  it('builds friendly beginner detail lines from bridge data only', () => {
    const text = radioLinkText(describeRadioLink(hello([radio()]), status()), { beginner: true });
    expect(text.deviceLabel).toBe('Serial /dev/ttyACM0 @115200');
    expect(text.settingsLabel).toBe('Medium Fast · EU868');
    expect(text.radioCountLabel).toBeNull();
  });

  it('uses raw profile ids in advanced detail lines', () => {
    const text = radioLinkText(describeRadioLink(hello([radio()]), status()));
    expect(text.settingsLabel).toBe('MEDIUM FAST · EU868');
  });

  it('counts multiple radios and reports no detail without any radio', () => {
    const multi = radioLinkText(
      describeRadioLink(hello([radio({ id: 'a' }), radio({ id: 'b', connected: false, state: 'disconnected' })]), status()),
    );
    expect(multi.radioCountLabel).toBe('1 of 2 radios connected');
    const empty = radioLinkText(describeRadioLink(hello([]), status({ state: 'disconnected', connected: false })));
    expect(empty.deviceLabel).toBeNull();
    expect(empty.settingsLabel).toBeNull();
    expect(empty.radioCountLabel).toBeNull();
  });

  it('maps states to tones for decoration only', () => {
    const tone = (state: RadioLinkStateValue) =>
      radioLinkTone(describeRadioLink(hello([radio()]), status({ state })));
    expect(tone('connected')).toBe('emerald');
    expect(tone('connecting')).toBe('amber');
    expect(tone('reconnecting')).toBe('amber');
    expect(tone('disconnected')).toBe('rose');
    expect(tone('virtual')).toBe('slate');
    expect(radioLinkTone(describeRadioLink(null, null))).toBe('slate');
  });

  it('labels each transport for the multi-radio list', () => {
    expect(transportStateLabel(radio())).toBe('Connected');
    expect(transportStateLabel(radio({ state: 'connecting', connected: false }))).toBe('Connecting');
    expect(transportStateLabel(radio({ state: 'reconnecting', connected: false }))).toBe('Reconnecting');
    expect(transportStateLabel(radio({ state: 'disconnected', connected: false }))).toBe('Not connected');
    expect(transportStateLabel(simRadio())).toBe('Virtual');
  });

  it('normalises transports that predate the state field', () => {
    expect(transportState(simRadio({ state: undefined }))).toBe('virtual');
    expect(transportState(radio({ state: undefined }))).toBe('connected');
    expect(transportState(radio({ state: undefined, connected: false }))).toBe('disconnected');
  });
});

describe('radio link: last frame and reconnect age', () => {
  const NOW = 1_700_000_100_000;

  it('formats the last-frame age in plain words', () => {
    expect(formatLastFrameAge(null, NOW)).toBeNull();
    expect(formatLastFrameAge(undefined, NOW)).toBeNull();
    expect(formatLastFrameAge(NOW - 300, NOW)).toBe('just now');
    expect(formatLastFrameAge(NOW - 5_000, NOW)).toBe('5 s ago');
    expect(formatLastFrameAge(NOW - 90_000, NOW)).toBe('2 min ago');
    expect(formatLastFrameAge(NOW - 45 * 60_000, NOW)).toBe('45 min ago');
    expect(formatLastFrameAge(NOW - 2 * 3_600_000, NOW)).toBe('2 h ago');
    // Clock skew: a future timestamp never produces a negative age.
    expect(formatLastFrameAge(NOW + 5_000, NOW)).toBe('just now');
  });

  it('shows a reconnect attempt label only while retrying after the first try', () => {
    const link = (state: 'connected' | 'reconnecting', attempts: number) =>
      describeRadioLink(
        hello([radio()]),
        status({ state, connected: state === 'connected', reconnect_attempts: attempts }),
      );
    expect(reconnectAttemptLabel(link('connected', 0))).toBeNull();
    expect(reconnectAttemptLabel(link('reconnecting', 1))).toBeNull();
    expect(reconnectAttemptLabel(link('reconnecting', 3))).toBe('Reconnect attempt 3');
  });
});

describe('radio link: guidance', () => {
  const guidanceFor = (state: RadioLinkStateValue) => {
    const link = describeRadioLink(
      hello([radio({ connected: state === 'connected' || state === 'virtual', state })]),
      status({ state }),
    );
    return radioGuidance(link);
  };

  it('shows no guidance while connected', () => {
    expect(guidanceFor('connected')).toBeNull();
  });

  it('explains a virtual radio as normal, not an error', () => {
    const guidance = guidanceFor('virtual');
    expect(guidance).toContain('virtual radio');
    expect(guidance).toContain('normal');
  });

  it('explains connecting, reconnecting and disconnected states', () => {
    expect(guidanceFor('connecting')).toContain('opening the radio connection');
    expect(guidanceFor('reconnecting')).toContain('keeps trying to reconnect');
    expect(guidanceFor('disconnected')).toContain('cannot open the radio');
    expect(radioGuidance(describeRadioLink(null, null))).toContain('has not reported a radio yet');
  });
});

describe('radio link: banner and composer copy', () => {
  const linkFor = (state: RadioLinkStateValue, attempts = 0) =>
    describeRadioLink(
      hello([radio({ connected: state === 'connected' || state === 'virtual', state })]),
      status({ state, connected: state === 'connected' || state === 'virtual', reconnect_attempts: attempts }),
    );

  it('has a banner for connecting, reconnecting and disconnected only', () => {
    expect(radioOfflineBanner(linkFor('connected'))).toBeNull();
    expect(radioOfflineBanner(linkFor('virtual'))).toBeNull();
    expect(radioOfflineBanner(describeRadioLink(null, null))).toBeNull();
    expect(radioOfflineBanner(linkFor('connecting'))?.title).toBe('Radio connecting…');
    expect(radioOfflineBanner(linkFor('reconnecting'))?.title).toContain('keeps retrying');
    expect(radioOfflineBanner(linkFor('disconnected'))?.title).toBe('Radio disconnected');
  });

  it('mentions automatic retry only while reconnecting', () => {
    expect(radioOfflineBanner(linkFor('reconnecting'))?.body).toContain('automatically');
    expect(radioOfflineBanner(linkFor('reconnecting', 3))?.title).toContain('(attempt 3)');
    expect(radioOfflineBanner(linkFor('connecting'))?.body).toContain('as soon as it is ready');
    expect(radioOfflineBanner(linkFor('disconnected'))?.body).toContain('cannot reach the radio');
  });

  it('reuses only an existing help-corpus slug in every banner', () => {
    for (const state of ['connecting', 'reconnecting', 'disconnected'] as const) {
      expect(radioOfflineBanner(linkFor(state))?.helpTopic).toBe('troubleshooting');
    }
  });

  it('returns a state-specific composer warning and keeps sending possible', () => {
    expect(composerRadioWarning(linkFor('connected'))).toBeNull();
    expect(composerRadioWarning(linkFor('virtual'))).toBeNull();
    expect(composerRadioWarning(linkFor('connecting'))).toBe(COMPOSER_RADIO_CONNECTING_WARNING);
    expect(composerRadioWarning(linkFor('reconnecting'))).toBe(COMPOSER_RADIO_RECONNECTING_WARNING);
    expect(composerRadioWarning(linkFor('disconnected'))).toBe(COMPOSER_RADIO_OFFLINE_WARNING);
    expect(COMPOSER_RADIO_OFFLINE_WARNING).toBe('Radio offline — your message will queue until it reconnects.');
    expect(COMPOSER_RADIO_RECONNECTING_WARNING).toContain('will queue');
    expect(COMPOSER_RADIO_CONNECTING_WARNING).toContain('will queue');
  });
});

describe('radio link: banner and composer decisions', () => {
  const live = { mode: 'live', connection: 'connected' } as const;
  const inputFor = (state: RadioLinkStateValue) => ({
    ...live,
    hello: hello([radio({ connected: state === 'connected' || state === 'virtual', state })]),
    bridgeStatus: status({ state, connected: state === 'connected' || state === 'virtual' }),
  });

  it('triggers for connecting, reconnecting and disconnected', () => {
    for (const state of ['connecting', 'reconnecting', 'disconnected'] as const) {
      expect(isLiveRadioOffline(inputFor(state))).toBe(true);
      expect(shouldShowRadioOfflineBanner(inputFor(state))).toBe(true);
      expect(shouldShowComposerRadioWarning(inputFor(state))).toBe(true);
      expect(composerRadioNotice(inputFor(state))).not.toBeNull();
    }
  });

  it('never triggers for connected or virtual radios', () => {
    for (const state of ['connected', 'virtual'] as const) {
      expect(isLiveRadioOffline(inputFor(state))).toBe(false);
      expect(shouldShowRadioOfflineBanner(inputFor(state))).toBe(false);
      expect(shouldShowComposerRadioWarning(inputFor(state))).toBe(false);
      expect(composerRadioNotice(inputFor(state))).toBeNull();
    }
  });

  it('does not claim offline outside a live, connected panel', () => {
    const offlineRadio = inputFor('reconnecting');
    expect(shouldShowRadioOfflineBanner({ ...offlineRadio, mode: 'demo' })).toBe(false);
    expect(shouldShowRadioOfflineBanner({ ...offlineRadio, connection: 'reconnecting' })).toBe(false);
    expect(shouldShowComposerRadioWarning({ ...offlineRadio, connection: 'offline' })).toBe(false);
  });

  it('does not claim offline before a radio state is known', () => {
    const unknown = { ...live, hello: null, bridgeStatus: null };
    expect(shouldShowRadioOfflineBanner(unknown)).toBe(false);
    expect(shouldShowComposerRadioWarning(unknown)).toBe(false);
  });
});

describe('radio link: beginner copy rules', () => {
  it('avoids protocol jargon in beginner-facing copy', () => {
    const lines: string[] = [];
    for (const state of ['connected', 'connecting', 'reconnecting', 'disconnected', 'virtual'] as const) {
      const link = describeRadioLink(
        hello([state === 'virtual' ? simRadio() : radio({ connected: state === 'connected', state })]),
        status({ state, connected: state === 'connected' || state === 'virtual', reconnect_attempts: 2 }),
      );
      lines.push(...[statusLabel(link), radioGuidance(link) ?? '', radioOfflineBanner(link)?.title ?? '', radioOfflineBanner(link)?.body ?? '', composerRadioWarning(link) ?? '']);
    }
    lines.push(
      radioGuidance(describeRadioLink(null, null)) ?? '',
      ...transportStateLabel(radio()),
    );
    for (const copy of lines.filter(Boolean)) {
      expect(bannedTermsInCopy(copy)).toEqual([]);
    }
  });
});
