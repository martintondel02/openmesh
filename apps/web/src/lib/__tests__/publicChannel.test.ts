import { describe, expect, it } from 'vitest';
import {
  PUBLIC_CHANNEL_ID,
  PUBLIC_CHANNEL_JOIN_URL,
  PUBLIC_CHANNEL_NAME,
  PUBLIC_CHANNEL_PSK,
  PUBLIC_CHANNEL_SUBTITLE,
  findPublicChannel,
  isPublicChannel,
  isPublicChannelId,
  publicChannelLeaveBlocked,
  shouldAutoSelectPublicChannel,
} from '../public-channel';
import { parseOpenMeshLink } from '../links';

describe('public channel constants', () => {
  it('uses the protocol-derived canonical id and a parseable name+PSK join URL', () => {
    expect(PUBLIC_CHANNEL_ID).toBe('a16d63142854cef4');
    expect(PUBLIC_CHANNEL_NAME).toBe('Public Mesh');
    expect(PUBLIC_CHANNEL_PSK).toBe('-w-KmLkWsRmxrtQqcPT2sXrlcTHTiaCNu1F-Qp_YqZI');
    expect(PUBLIC_CHANNEL_JOIN_URL.startsWith('openmesh://channel?name=Public%20Mesh&psk=')).toBe(true);
    const parsed = parseOpenMeshLink(PUBLIC_CHANNEL_JOIN_URL);
    expect(parsed.kind).toBe('channel');
    if (parsed.kind === 'channel') {
      expect(parsed.name).toBe(PUBLIC_CHANNEL_NAME);
      expect(parsed.psk).toBe(PUBLIC_CHANNEL_PSK);
      // The canonical id is derived; the invite deliberately does not depend on it.
      expect(parsed.channelId).toBeNull();
    }
  });

  it('recognises the channel by public flag, canonical id or name (legacy bridges)', () => {
    expect(isPublicChannel({ public: true, name: 'anything' })).toBe(true);
    expect(isPublicChannel({ channel_id: PUBLIC_CHANNEL_ID.toUpperCase() })).toBe(true);
    expect(isPublicChannel({ name: ' public mesh ' })).toBe(true);
    expect(isPublicChannel({ channel_id: 'f0e1d2c3b4a59687', name: 'familien' })).toBe(false);
    expect(isPublicChannel(null)).toBe(false);
    expect(isPublicChannelId(PUBLIC_CHANNEL_ID)).toBe(true);
    expect(isPublicChannelId('baac0f9144756da1')).toBe(false);
  });

  it('finds the public channel and blocks leaving it', () => {
    const channels = {
      f0e1d2c3b4a59687: { channel_id: 'f0e1d2c3b4a59687', name: 'familien', psk_set: true },
      [PUBLIC_CHANNEL_ID]: { channel_id: PUBLIC_CHANNEL_ID, name: PUBLIC_CHANNEL_NAME, psk_set: true, public: true },
    };
    expect(findPublicChannel(channels)?.channel_id).toBe(PUBLIC_CHANNEL_ID);
    expect(findPublicChannel({})).toBeUndefined();
    expect(publicChannelLeaveBlocked(PUBLIC_CHANNEL_ID, channels)).toBe(true);
    expect(publicChannelLeaveBlocked('f0e1d2c3b4a59687', channels)).toBe(false);
    // Canonical id alone blocks even if an old bridge omitted `public`.
    expect(publicChannelLeaveBlocked(PUBLIC_CHANNEL_ID, {})).toBe(true);
  });

  it('auto-selects only on a fresh/empty live client without a deep link or active thread', () => {
    const channels = { [PUBLIC_CHANNEL_ID]: { channel_id: PUBLIC_CHANNEL_ID, name: PUBLIC_CHANNEL_NAME } };
    const base = {
      mode: 'live' as const,
      hasActiveThread: false,
      hasThreadDeepLink: false,
      messageCount: 0,
      channels,
    };
    expect(shouldAutoSelectPublicChannel(base)).toBe(true);
    expect(shouldAutoSelectPublicChannel({ ...base, mode: 'demo' })).toBe(false);
    expect(shouldAutoSelectPublicChannel({ ...base, hasActiveThread: true })).toBe(false);
    expect(shouldAutoSelectPublicChannel({ ...base, hasThreadDeepLink: true })).toBe(false);
    expect(shouldAutoSelectPublicChannel({ ...base, messageCount: 1 })).toBe(false);
    expect(shouldAutoSelectPublicChannel({ ...base, channels: {} })).toBe(false);
    expect(PUBLIC_CHANNEL_SUBTITLE).toBe('Public · anyone can join');
  });
});
