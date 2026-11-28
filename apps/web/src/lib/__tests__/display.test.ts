import { describe, expect, it } from 'vitest';
import {
  PEER_ONLINE_WINDOW_MS,
  deliveryPresentation,
  displayName,
  nameWithShortId,
  peerLinkFacts,
  peerQuality,
  shortNodeId,
} from '../display';

const NODE = '68102c09d9c0aeca';
const NOW = 1_800_000_000_000;

describe('display: names and fallbacks', () => {
  it('prefers the announced name, then the pinned alias, then node <first8>', () => {
    expect(displayName({ announcedName: '  Ada  ', contactName: 'my-phone', nodeId: NODE })).toBe('Ada');
    expect(displayName({ announcedName: '', contactName: 'Pinned phone', nodeId: NODE })).toBe('Pinned phone');
    expect(displayName({ announcedName: null, contactName: null, nodeId: NODE })).toBe('node 68102c09');
    expect(displayName({ announcedName: 'Ada', contactName: 'alias', nodeId: NODE, own: true })).toBe('You');
  });

  it('formats short ids and the name + id pair', () => {
    expect(shortNodeId(NODE)).toBe('68102c09');
    expect(shortNodeId('AB12CD34EF56')).toBe('ab12cd34');
    expect(shortNodeId('')).toBe('unknown');
    expect(nameWithShortId('Ada', NODE)).toBe('Ada · 68102c09');
  });
});

describe('display: peer quality vocabulary', () => {
  it('maps strong/usable/weak signals while the peer was heard recently', () => {
    expect(peerQuality({ rssi: -70, lastSeenMs: NOW - 30_000, nowMs: NOW }).label).toBe('Strong');
    expect(peerQuality({ rssi: -90, lastSeenMs: NOW - 30_000, nowMs: NOW }).label).toBe('OK');
    expect(peerQuality({ rssi: -103, lastSeenMs: NOW - 30_000, nowMs: NOW }).label).toBe('Weak');
    // A fresh node with no RSSI reading is still reachable: OK, not Offline.
    expect(peerQuality({ rssi: null, lastSeenMs: NOW - 30_000, nowMs: NOW }).label).toBe('OK');
  });

  it('marks stale or never-heard peers as Offline even with a good last RSSI', () => {
    const stale = peerQuality({ rssi: -70, lastSeenMs: NOW - PEER_ONLINE_WINDOW_MS - 1, nowMs: NOW });
    expect(stale.level).toBe('offline');
    expect(stale.label).toBe('Offline');
    expect(peerQuality({ rssi: -70, lastSeenMs: null, nowMs: NOW }).label).toBe('Offline');
  });

  it('builds compact peer facts for the header chip and inspector card', () => {
    const facts = peerLinkFacts({
      nodeId: NODE,
      announcedName: 'MTENK-PHONE',
      contactName: 'alias',
      isContact: true,
      role: 'client',
      hops: 0,
      rssi: -70,
      snr: 2.5,
      lastSeenMs: NOW - 30_000,
      nowMs: NOW,
    });
    expect(facts.name).toBe('MTENK-PHONE');
    expect(facts.shortId).toBe('68102c09');
    expect(facts.quality.level).toBe('strong');
    expect(facts.lastHeard).toBe('just now');
    expect(facts.isContact).toBe(true);
  });
});

describe('display: delivery wording', () => {
  it('gives explicit labels for every direct delivery state', () => {
    expect(deliveryPresentation({ state: 'pending', peerName: 'Ada' }).label).toBe('Sending…');
    expect(deliveryPresentation({ state: 'sent', peerName: 'Ada' }).label).toBe('Sent to bridge');
    expect(deliveryPresentation({ state: 'delivered', peerName: 'Ada' }).label).toBe('Delivered to Ada');
    expect(deliveryPresentation({ state: 'read', peerName: 'Ada' }).label).toBe('Read by Ada');
    const failed = deliveryPresentation({ state: 'failed', peerName: 'Ada', error: 'Radio busy' });
    expect(failed.label).toBe('Not delivered');
    expect(failed.explanation).toContain('Radio busy');
    const cancelled = deliveryPresentation({ state: 'cancelled', peerName: 'Ada' });
    expect(cancelled.label).toBe('Cancelled');
    expect(cancelled.tone).toBe('slate');
    expect(cancelled.hasReceipt).toBe(false);
    expect(cancelled.explanation).toContain('cancelled');
    expect(deliveryPresentation({ state: 'cancelled', group: true }).label).toBe('Cancelled');
  });

  it('explains that channel messages have no per-recipient receipt', () => {
    const group = deliveryPresentation({ state: 'sent', group: true });
    expect(group.label).toBe('Sent to channel');
    expect(group.hasReceipt).toBe(false);
    expect(group.explanation.toLowerCase()).toContain('no per-recipient');
    const groupFailed = deliveryPresentation({ state: 'failed', group: true });
    expect(groupFailed.tone).toBe('rose');
    expect(groupFailed.label).toBe('Channel send failed');
  });

  it('keeps the delivered/read explanation tied to the peer name', () => {
    const delivered = deliveryPresentation({ state: 'delivered', peerName: 'Ada' });
    expect(delivered.explanation).toContain("Ada's node acknowledged");
    expect(delivered.tone).toBe('emerald');
    expect(deliveryPresentation({ state: 'sent', peerName: 'Ada', hasReceipts: false }).explanation).toContain(
      'not available',
    );
  });
});
