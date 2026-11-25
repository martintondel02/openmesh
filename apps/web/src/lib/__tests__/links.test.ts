import { describe, expect, it } from 'vitest';
import {
  buildChannelLink,
  buildContactLink,
  describeLink,
  isValidIdentity,
  isValidPsk,
  nodeIdFromIdentity,
  parseOpenMeshLink,
} from '../links';
import { bytesToBase64 } from '../utils';

const NODE_ID = '6b1f9a2c4d7e8031';
const CHANNEL_ID = 'f0e1d2c3b4a59687';

function makeIdentity(nodeId: string): string {
  const bytes = new Uint8Array(72);
  for (let index = 0; index < 72; index += 1) bytes[index] = (index * 7 + 3) % 256;
  const idBytes = new Uint8Array(8);
  for (let index = 0; index < 8; index += 1) {
    idBytes[index] = Number.parseInt(nodeId.slice(index * 2, index * 2 + 2), 16);
  }
  bytes.set(idBytes, 0);
  return bytesToBase64(bytes);
}

function makePsk(): string {
  const bytes = new Uint8Array(32);
  for (let index = 0; index < 32; index += 1) bytes[index] = (index * 11 + 5) % 256;
  return bytesToBase64(bytes);
}

const IDENTITY = makeIdentity(NODE_ID);
const PSK = makePsk();

describe('links: contact invites', () => {
  it('round-trips a contact link with name and identity', () => {
    const link = buildContactLink({ nodeId: NODE_ID, name: 'Anne Marie', identity: IDENTITY });
    const parsed = parseOpenMeshLink(link);
    expect(parsed.kind).toBe('contact');
    if (parsed.kind !== 'contact') throw new Error('expected contact');
    expect(parsed.nodeId).toBe(NODE_ID);
    expect(parsed.name).toBe('Anne Marie');
    expect(parsed.identity).toBe(IDENTITY);
    expect(parsed.nodeIdFromIdentity).toBe(NODE_ID);
  });

  it('derives the node id from a 72-byte identity', () => {
    expect(nodeIdFromIdentity(IDENTITY)).toBe(NODE_ID);
    expect(isValidIdentity(IDENTITY)).toBe(true);
    expect(isValidIdentity(bytesToBase64(new Uint8Array(71)))).toBe(false);
  });

  it('accepts links that only carry an identity', () => {
    const link = `openmesh://contact?name=Anne&identity=${encodeURIComponent(IDENTITY)}`;
    const parsed = parseOpenMeshLink(link);
    expect(parsed.kind).toBe('contact');
    if (parsed.kind !== 'contact') throw new Error('expected contact');
    expect(parsed.nodeId).toBe(NODE_ID);
  });

  it('rejects node ids that do not match the identity', () => {
    const link = `openmesh://contact?node_id=ffffffffffffffff&identity=${encodeURIComponent(IDENTITY)}`;
    const parsed = parseOpenMeshLink(link);
    expect(parsed.kind).toBe('error');
    if (parsed.kind !== 'error') throw new Error('expected error');
    expect(parsed.error).toMatch(/does not match/i);
  });

  it('rejects malformed or unknown links', () => {
    expect(parseOpenMeshLink('').kind).toBe('error');
    expect(parseOpenMeshLink('https://example.com').kind).toBe('error');
    expect(parseOpenMeshLink('openmesh://contact?node_id=xyz').kind).toBe('error');
    expect(parseOpenMeshLink('openmesh://unknown?foo=1').kind).toBe('error');
    expect(parseOpenMeshLink('openmesh://contact').kind).toBe('error');
  });
});

describe('links: channel invites', () => {
  it('round-trips a channel link with a PSK', () => {
    const link = buildChannelLink({ channelId: CHANNEL_ID, name: 'nordmarka-tur', psk: PSK });
    const parsed = parseOpenMeshLink(link);
    expect(parsed.kind).toBe('channel');
    if (parsed.kind !== 'channel') throw new Error('expected channel');
    expect(parsed.channelId).toBe(CHANNEL_ID);
    expect(parsed.name).toBe('nordmarka-tur');
    expect(parsed.psk).toBe(PSK);
  });

  it('validates the channel PSK length', () => {
    expect(isValidPsk(PSK)).toBe(true);
    expect(isValidPsk(bytesToBase64(new Uint8Array(31)))).toBe(false);
    const parsed = parseOpenMeshLink(`openmesh://channel?name=tur&psk=${encodeURIComponent(bytesToBase64(new Uint8Array(16)))}`);
    expect(parsed.kind).toBe('error');
  });

  it('rejects contact links passed as channel invites', () => {
    const parsed = parseOpenMeshLink(buildContactLink({ nodeId: NODE_ID, name: 'Anne' }));
    expect(parsed.kind).toBe('contact');
  });

  it('describes links for the join dialog preview', () => {
    const contact = parseOpenMeshLink(buildContactLink({ nodeId: NODE_ID, name: 'Anne' }));
    expect(describeLink(contact)).toContain('Anne');
    const channel = parseOpenMeshLink(buildChannelLink({ name: 'familien', psk: PSK }));
    expect(describeLink(channel)).toContain('familien');
    expect(describeLink(channel)).toContain('shared key');
  });
});
