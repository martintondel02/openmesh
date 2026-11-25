import { describe, expect, it } from 'vitest';
import {
  DEMO_ANNE_ID,
  DEMO_CHANNEL_FAMILY,
  DEMO_CHANNEL_TUR,
  DEMO_LARS_ID,
  DEMO_OWN_ID,
  createDemoDataset,
  demoChannelUrl,
  demoContactUrl,
  demoIdentity,
} from '../mock/data';
import { base64ToBytes, isValidNodeId } from '../utils';
import { parseOpenMeshLink } from '../links';

const NOW = new Date(2026, 2, 15, 12, 0, 0).getTime();
const OSLO = { minLat: 59.8, maxLat: 60.1, minLon: 10.3, maxLon: 11.0 };

describe('demo dataset: shape and invariants', () => {
  const dataset = createDemoDataset(NOW);

  it('is deterministic for a fixed clock', () => {
    const again = createDemoDataset(NOW);
    expect(JSON.stringify(again.messages)).toBe(JSON.stringify(dataset.messages));
    expect(again.nodes.map((node) => node.node_id)).toEqual(dataset.nodes.map((node) => node.node_id));
  });

  it('has a coherent own identity', () => {
    expect(dataset.ownNodeId).toBe(DEMO_OWN_ID);
    expect(isValidNodeId(dataset.ownNodeId)).toBe(true);
    expect(dataset.hello.node.node_id).toBe(dataset.ownNodeId);
    expect(dataset.hello.node.name).toBe(dataset.ownName);
    expect(dataset.config.node.name).toBe(dataset.ownName);
  });

  it('populates several nodes with unique ids and Oslo positions', () => {
    expect(dataset.nodes.length).toBeGreaterThanOrEqual(6);
    const ids = new Set(dataset.nodes.map((node) => node.node_id));
    expect(ids.size).toBe(dataset.nodes.length);
    for (const node of dataset.nodes) {
      expect(isValidNodeId(node.node_id)).toBe(true);
      expect(node.last_seen_ms).toBeLessThanOrEqual(NOW);
      if (node.position) {
        expect(node.position.lat).toBeGreaterThan(OSLO.minLat);
        expect(node.position.lat).toBeLessThan(OSLO.maxLat);
        expect(node.position.lon).toBeGreaterThan(OSLO.minLon);
        expect(node.position.lon).toBeLessThan(OSLO.maxLon);
      }
      if (node.rssi !== null) {
        expect(node.rssi).toBeLessThan(0);
        expect(node.rssi).toBeGreaterThan(-140);
      }
    }
    expect(dataset.nodes.some((node) => node.class === 'repeater')).toBe(true);
    expect(dataset.nodes.some((node) => node.class === 'tracker')).toBe(true);
    expect(dataset.nodes.some((node) => node.class === 'gateway')).toBe(true);
  });

  it('keeps contacts consistent with nodes and identities', () => {
    expect(dataset.contacts.length).toBeGreaterThan(0);
    for (const contact of dataset.contacts) {
      expect(dataset.identities[contact.node_id]).toBeTruthy();
      const bytes = base64ToBytes(dataset.identities[contact.node_id]!);
      expect(bytes.length).toBe(72);
      const nodeId = Array.from(bytes.slice(0, 8), (byte) => byte.toString(16).padStart(2, '0')).join('');
      expect(nodeId).toBe(contact.node_id);
    }
    const anne = dataset.contacts.find((contact) => contact.node_id === DEMO_ANNE_ID);
    expect(anne?.name).toBe('Anne');
  });

  it('has channels with 32-byte PSKs', () => {
    expect(dataset.channels.map((channel) => channel.channel_id)).toEqual(
      expect.arrayContaining([DEMO_CHANNEL_FAMILY, DEMO_CHANNEL_TUR]),
    );
    for (const channel of dataset.channels) {
      expect(channel.psk_set).toBe(true);
      const psk = dataset.channelPsks[channel.channel_id];
      expect(psk).toBeTruthy();
      expect(base64ToBytes(psk!).length).toBe(32);
    }
  });

  it('sorts messages and references existing conversations', () => {
    const timestamps = dataset.messages.map((message) => message.sent_at_ms);
    expect([...timestamps].sort((a, b) => a - b)).toEqual(timestamps);
    const groupTimestamps = dataset.groupMessages.map((message) => message.sent_at_ms);
    expect([...groupTimestamps].sort((a, b) => a - b)).toEqual(groupTimestamps);
    for (const message of dataset.messages) {
      expect([DEMO_ANNE_ID, DEMO_LARS_ID]).toContain(message.peer);
    }
    for (const message of dataset.groupMessages) {
      expect([DEMO_CHANNEL_FAMILY, DEMO_CHANNEL_TUR]).toContain(message.channel_id);
      expect(message.channel_name).toBe(
        message.channel_id === DEMO_CHANNEL_FAMILY ? 'familien' : 'nordmarka-tur',
      );
      expect(message.signature_valid).toBe(true);
    }
  });

  it('seeds the UI edge cases reviewers need', () => {
    expect(dataset.messages.some((message) => message.demoDelivery === 'failed')).toBe(true);
    expect(dataset.messages.some((message) => !message.outgoing && !message.read)).toBe(true);
    expect(dataset.messages.some((message) => message.reply_to !== null)).toBe(true);
    expect(dataset.messages.some((message) => message.attachment?.mime.startsWith('image/'))).toBe(true);
    expect(dataset.messages.some((message) => message.attachment?.mime.startsWith('audio/'))).toBe(true);
    expect(dataset.groupMessages.some((message) => message.outgoing)).toBe(true);
    expect(dataset.groupMessages.some((message) => message.kind === 'position')).toBe(true);
  });

  it('embeds playable media as data urls', () => {
    expect(dataset.trailPhoto.dataUrl.startsWith('data:image/svg+xml')).toBe(true);
    expect(dataset.cabinMap.dataUrl.startsWith('data:image/svg+xml')).toBe(true);
    expect(dataset.voiceNote.dataUrl.startsWith('data:audio/wav;base64,')).toBe(true);
    expect(dataset.voiceNote.waveform.length).toBe(32);
    expect(dataset.voiceNote.waveform.every((value) => value >= 0 && value <= 1)).toBe(true);
    expect(dataset.voiceNote.durationMs).toBeGreaterThan(3000);
  });

  it('includes position history for the map polylines', () => {
    expect(Object.keys(dataset.positionHistories).length).toBeGreaterThanOrEqual(3);
    for (const [nodeId, history] of Object.entries(dataset.positionHistories)) {
      expect(history.length).toBeGreaterThan(2);
      expect(isValidNodeId(nodeId)).toBe(true);
      const timestamps = history.map((position) => position.timestamp_ms);
      expect([...timestamps].sort((a, b) => a - b)).toEqual(timestamps);
    }
  });

  it('builds contact and channel invites that parse back', () => {
    const contactUrl = demoContactUrl(dataset, DEMO_ANNE_ID);
    const contact = parseOpenMeshLink(contactUrl);
    expect(contact.kind).toBe('contact');
    if (contact.kind !== 'contact') throw new Error('expected contact');
    expect(contact.nodeId).toBe(DEMO_ANNE_ID);
    expect(contact.identity).toBe(dataset.identities[DEMO_ANNE_ID]);

    const channelUrl = demoChannelUrl(dataset, DEMO_CHANNEL_TUR);
    const channel = parseOpenMeshLink(channelUrl);
    expect(channel.kind).toBe('channel');
    if (channel.kind !== 'channel') throw new Error('expected channel');
    expect(channel.channelId).toBe(DEMO_CHANNEL_TUR);
    expect(channel.psk).toBe(dataset.channelPsks[DEMO_CHANNEL_TUR]);
  });

  it('produces valid deterministic demo identities', () => {
    const identity = demoIdentity(DEMO_OWN_ID, 1);
    expect(base64ToBytes(identity).length).toBe(72);
    expect(demoIdentity(DEMO_OWN_ID, 1)).toBe(identity);
    expect(demoIdentity(DEMO_OWN_ID, 2)).not.toBe(identity);
  });
});
