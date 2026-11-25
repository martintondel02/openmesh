/**
 * Parsing and building of `openmesh://` links.
 *
 * Link shapes used by the client (fields follow WEBSOCKET-API §4.3/§4.4 and the
 * 72-byte identity layout in PROTOCOL §3):
 *
 *   openmesh://contact?node_id=<16 hex>&name=<label>&identity=<base64 72B>
 *   openmesh://channel?channel_id=<16 hex>&name=<label>&psk=<base64 32B>
 *
 * Parsing is intentionally forgiving: query parameters may be percent-encoded,
 * base64 may be URL-safe, and hex may be uppercase. Invalid links return a typed
 * error instead of throwing so the paste box can explain the problem.
 */

import { decodeBase64OfLength, isHex, isValidNodeId } from './utils';

export const CONTACT_LINK_SCHEME = 'openmesh';

export interface ParsedContactLink {
  kind: 'contact';
  nodeId: string | null;
  name: string | null;
  identity: string | null;
  nodeIdFromIdentity: string | null;
}

export interface ParsedChannelLink {
  kind: 'channel';
  channelId: string | null;
  name: string | null;
  psk: string | null;
}

export interface ParsedLinkError {
  kind: 'error';
  error: string;
}

export type ParsedLink = ParsedContactLink | ParsedChannelLink | ParsedLinkError;

/** Node id embedded in a 72-byte identity: `node_id(8) || sign_pk(32) || dh_pk(32)`. */
export function nodeIdFromIdentity(identity: string): string | null {
  const bytes = decodeBase64OfLength(identity, 72);
  if (!bytes) return null;
  return Array.from(bytes.slice(0, 8), (b) => b.toString(16).padStart(2, '0')).join('');
}

export function isValidIdentity(identity: string): boolean {
  return decodeBase64OfLength(identity, 72) !== null;
}

export function isValidPsk(psk: string): boolean {
  return decodeBase64OfLength(psk, 32) !== null;
}

function parseUrl(input: string): URL | null {
  try {
    return new URL(input.trim());
  } catch {
    return null;
  }
}

export function parseOpenMeshLink(input: string): ParsedLink {
  const trimmed = input.trim();
  if (!trimmed) return { kind: 'error', error: 'Paste an openmesh:// link first.' };
  if (!/^openmesh:\/\//i.test(trimmed)) {
    return { kind: 'error', error: 'Not an openmesh:// link. Expected openmesh://contact?…' };
  }
  const url = parseUrl(trimmed);
  if (!url) return { kind: 'error', error: 'That link is not a valid URL.' };
  const action = (url.hostname || url.pathname.replace(/^\/+/, '')).toLowerCase().split('/')[0] ?? '';
  const params = url.searchParams;

  if (action === 'contact') {
    const rawNodeId = params.get('node_id') ?? params.get('nodeId') ?? '';
    const rawName = params.get('name');
    const rawIdentity = params.get('identity') ?? params.get('id') ?? '';
    const nodeId = rawNodeId ? rawNodeId.toLowerCase() : null;
    if (nodeId && !isValidNodeId(nodeId)) {
      return { kind: 'error', error: 'node_id must be 16 hex characters (8 bytes).' };
    }
    const identity = rawIdentity || null;
    let nodeIdFromIdent: string | null = null;
    if (identity) {
      if (!isValidIdentity(identity)) {
        return { kind: 'error', error: 'identity must be base64 for the 72-byte public identity.' };
      }
      nodeIdFromIdent = nodeIdFromIdentity(identity);
      if (nodeId && nodeIdFromIdent && nodeId !== nodeIdFromIdent) {
        return { kind: 'error', error: 'node_id does not match the identity in this link.' };
      }
    }
    if (!nodeId && !identity) {
      return { kind: 'error', error: 'Link has no node_id or identity.' };
    }
    return {
      kind: 'contact',
      nodeId: nodeId ?? nodeIdFromIdent,
      name: rawName ? rawName.slice(0, 64) : null,
      identity,
      nodeIdFromIdentity: nodeIdFromIdent,
    };
  }

  if (action === 'channel') {
    const rawChannelId = params.get('channel_id') ?? params.get('channelId') ?? '';
    const channelId = rawChannelId ? rawChannelId.toLowerCase() : null;
    if (channelId && !isHex(channelId, 8)) {
      return { kind: 'error', error: 'channel_id must be 16 hex characters (8 bytes).' };
    }
    const psk = params.get('psk') ?? null;
    if (psk && !isValidPsk(psk)) {
      return { kind: 'error', error: 'psk must be base64 for 32 bytes (256-bit channel key).' };
    }
    const name = params.get('name');
    if (!name && !channelId && !psk) {
      return { kind: 'error', error: 'Channel link needs a name, channel_id or psk.' };
    }
    return {
      kind: 'channel',
      channelId,
      name: name ? name.slice(0, 48) : null,
      psk,
    };
  }

  return { kind: 'error', error: `Unknown openmesh:// action “${action || 'none'}”.` };
}

export interface ContactLinkFields {
  nodeId: string;
  name?: string | null;
  identity?: string | null;
}

export function buildContactLink(fields: ContactLinkFields): string {
  const params = new URLSearchParams();
  params.set('node_id', fields.nodeId.toLowerCase());
  if (fields.name) params.set('name', fields.name);
  if (fields.identity) params.set('identity', fields.identity);
  return `${CONTACT_LINK_SCHEME}://contact?${params.toString()}`;
}

export interface ChannelLinkFields {
  channelId?: string | null;
  name?: string | null;
  psk?: string | null;
}

export function buildChannelLink(fields: ChannelLinkFields): string {
  const params = new URLSearchParams();
  if (fields.channelId) params.set('channel_id', fields.channelId.toLowerCase());
  if (fields.name) params.set('name', fields.name);
  if (fields.psk) params.set('psk', fields.psk);
  return `${CONTACT_LINK_SCHEME}://channel?${params.toString()}`;
}

/** Short human label for a pasted link, shown in the join dialog confirmation. */
export function describeLink(link: ParsedLink): string {
  if (link.kind === 'error') return link.error;
  if (link.kind === 'contact') {
    const who = link.name ?? link.nodeId ?? link.nodeIdFromIdentity ?? 'unknown node';
    return `Contact invite for ${who}`;
  }
  const what = link.name ?? link.channelId ?? 'unnamed channel';
  return link.psk ? `Channel “${what}” with a shared key` : `Channel “${what}” without a key`;
}
