/**
 * Pure store reducers. The zustand store wires these together; keeping them
 * separate makes the tricky parts (optimistic sends, message merges, acks,
 * unread counts) unit-testable without React.
 */

import type {
  ChatGroupMessage,
  ChatMessage,
  Channel,
  ConfigData,
  Contact,
  ContactData,
  DeliveryState,
  GroupMessageData,
  MessageData,
  MessageUpdateData,
  NodeData,
  Position,
} from '../types/api';

/* ------------------------------------------------------------------ */
/* Messages                                                            */
/* ------------------------------------------------------------------ */

/** Derive the UI delivery state from the bridge flags + local bookkeeping. */
export function deriveDelivery(message: Pick<MessageData, 'outgoing' | 'delivered' | 'read'>, local?: DeliveryState): DeliveryState {
  if (local === 'pending' || local === 'failed') return local;
  if (!message.outgoing) return 'sent';
  if (message.read) return 'read';
  if (message.delivered) return 'delivered';
  return 'sent';
}

function normalizeMessage(input: MessageData, existing?: ChatMessage): ChatMessage {
  // A failed send stays failed until the bridge reports delivery progress; every
  // other transition is derived from the authoritative bridge flags.
  const delivery: DeliveryState =
    existing?.delivery === 'failed' && !input.delivered && !input.read
      ? 'failed'
      : deriveDelivery(input);
  return {
    ...input,
    // A bridge echo may omit media fields the optimistic row already knows about.
    attachment: input.attachment ?? existing?.attachment ?? null,
    position: input.position ?? existing?.position ?? null,
    delivery,
    ...(existing?.tempId ? { tempId: existing.tempId } : {}),
    ...(existing?.localUrl ? { localUrl: existing.localUrl } : {}),
    ...(existing?.waveform ? { waveform: existing.waveform } : {}),
    ...(existing?.error ? { error: existing.error } : {}),
    ...(existing?.seeded ? { seeded: true } : {}),
  };
}

/** Insert or update a direct message. Optimistic rows are reconciled by tempId. */
export function upsertMessage(messages: ChatMessage[], incoming: MessageData, tempId?: string): ChatMessage[] {
  const byTemp = tempId
    ? messages.findIndex((m) => m.tempId === tempId)
    : -1;
  const byId = messages.findIndex((m) => m.msg_id === incoming.msg_id && m.msg_id !== 0);
  const index = byId >= 0 ? byId : byTemp;
  if (index >= 0) {
    const existing = messages[index]!;
    const next = messages.slice();
    next[index] = normalizeMessage(incoming, existing);
    return next;
  }
  return [...messages, normalizeMessage(incoming)];
}

/** Apply a `message_update` event, merging only the fields that changed. */
export function applyMessageUpdate(messages: ChatMessage[], update: MessageUpdateData): ChatMessage[] {
  const index = messages.findIndex((m) => m.msg_id === update.msg_id && m.msg_id !== 0);
  if (index < 0) return messages;
  const existing = messages[index]!;
  const merged: MessageData = {
    ...existing,
    ...update,
    attachment: update.attachment !== undefined ? update.attachment : existing.attachment,
    text: update.text !== undefined ? update.text : existing.text,
    reply_to: update.reply_to !== undefined ? update.reply_to : existing.reply_to,
  };
  const next = messages.slice();
  next[index] = normalizeMessage(merged, existing);
  return next;
}

/**
 * Apply a `message_update` event to a channel message.
 *
 * The bridge sends the channel message first and the stored attachment (`id`, size, sha256,
 * progress 1.0) in a later `message_update` — the update must reach `groupMessages[]` too or
 * received channel images/voice notes never render.
 */
export function applyGroupMessageUpdate(messages: ChatGroupMessage[], update: MessageUpdateData): ChatGroupMessage[] {
  const index = messages.findIndex((m) => m.msg_id === update.msg_id && m.msg_id !== 0);
  if (index < 0) return messages;
  const existing = messages[index]!;
  const next = messages.slice();
  next[index] = {
    ...existing,
    // Only merge fields the update actually carries; keep local-only fields (localUrl, waveform).
    ...(update.attachment !== undefined ? { attachment: update.attachment } : {}),
    ...(update.text !== undefined ? { text: update.text } : {}),
    ...(update.kind !== undefined ? { kind: update.kind } : {}),
  };
  return next;
}

/** Clamp a resource-progress event into 0..1. */
export function resourceProgressFraction(received: number, total: number): number {
  if (!Number.isFinite(total) || total <= 0) return 0;
  if (!Number.isFinite(received) || received <= 0) return 0;
  return Math.min(1, received / total);
}

/**
 * Bump an attachment's transfer progress for `msg_id` in a direct or channel message list.
 * Returns the original array when nothing matched (keeps React re-renders cheap).
 */
export function applyResourceProgress<T extends { msg_id: number; attachment: { progress: number } | null }>(
  items: T[],
  msgId: number,
  progress: number,
): T[] {
  const bounded = Math.min(1, Math.max(0, progress));
  let changed = false;
  const next = items.map((item) => {
    if (item.msg_id !== msgId || !item.attachment) return item;
    changed = true;
    return { ...item, attachment: { ...item.attachment, progress: bounded } };
  });
  return changed ? next : items;
}

/** Apply an `ack` event. `read` implies delivered. */
export function applyAck(messages: ChatMessage[], msgId: number, kind: 'delivered' | 'read' | 'resource_window' | 'resource_complete'): ChatMessage[] {
  if (kind !== 'delivered' && kind !== 'read') return messages;
  const index = messages.findIndex((m) => m.msg_id === msgId);
  if (index < 0) return messages;
  const existing = messages[index]!;
  const next = messages.slice();
  next[index] = {
    ...existing,
    delivered: true,
    read: kind === 'read' ? true : existing.read,
    delivery: kind === 'read' ? 'read' : 'delivered',
    error: undefined,
  };
  return next;
}

/** Mark a failed or pending outgoing message for retry (optimistic). */
export function markMessagePending(messages: ChatMessage[], msgId: number): ChatMessage[] {
  return messages.map((m) =>
    m.msg_id === msgId ? { ...m, delivery: 'pending', error: undefined } : m,
  );
}

export function markMessageFailed(messages: ChatMessage[], tempId: string, error: string): ChatMessage[] {
  return messages.map((m) => (m.tempId === tempId ? { ...m, delivery: 'failed', error } : m));
}

/** Replace an optimistic row with the bridge's confirmation. */
export function reconcileSendResult(
  messages: ChatMessage[],
  tempId: string,
  msgId: number,
): ChatMessage[] {
  return messages.map((m) => (m.tempId === tempId ? { ...m, msg_id: msgId, delivery: 'sent', tempId: undefined } : m));
}

export function unreadCount(messages: Pick<ChatMessage, 'outgoing' | 'read'>[]): number {
  return messages.filter((m) => !m.outgoing && !m.read).length;
}

/** Newest-first sort without mutating the input. */
export function sortMessages<T extends { sent_at_ms: number }>(messages: T[]): T[] {
  return messages.slice().sort((a, b) => a.sent_at_ms - b.sent_at_ms);
}

/* ------------------------------------------------------------------ */
/* Group messages                                                     */
/* ------------------------------------------------------------------ */

export function upsertGroupMessage(messages: ChatGroupMessage[], incoming: GroupMessageData, tempId?: string): ChatGroupMessage[] {
  const index = tempId
    ? messages.findIndex((m) => m.tempId === tempId)
    : messages.findIndex((m) => m.msg_id === incoming.msg_id && m.msg_id !== 0);
  if (index >= 0) {
    const existing = messages[index]!;
    const next = messages.slice();
    next[index] = {
      ...incoming,
      ...(existing.tempId ? { tempId: existing.tempId } : {}),
      ...(existing.localUrl ? { localUrl: existing.localUrl } : {}),
      ...(existing.waveform ? { waveform: existing.waveform } : {}),
      ...(existing.seeded ? { seeded: true } : {}),
    };
    return next;
  }
  return [...messages, { ...incoming, ...(tempId ? { tempId } : {}) }];
}

/* ------------------------------------------------------------------ */
/* Nodes, contacts, channels                                          */
/* ------------------------------------------------------------------ */

/** Merge a node event; keeps existing fields when the update omits them. */
export function upsertNode(nodes: Record<string, NodeData>, incoming: NodeData): Record<string, NodeData> {
  const existing = nodes[incoming.node_id];
  if (!existing) return { ...nodes, [incoming.node_id]: incoming };
  return {
    ...nodes,
    [incoming.node_id]: {
      ...existing,
      ...incoming,
      // A node event without a position should not erase a known position.
      position: incoming.position ?? existing.position,
    },
  };
}

/** Append a position sample to the per-node history, deduped by timestamp. */
export function appendPositionHistory(
  history: Record<string, Position[]>,
  nodeId: string,
  position: Position,
  limit = 200,
): Record<string, Position[]> {
  const existing = history[nodeId] ?? [];
  if (existing.some((p) => p.timestamp_ms === position.timestamp_ms)) return history;
  const next = [...existing, position].sort((a, b) => a.timestamp_ms - b.timestamp_ms);
  return { ...history, [nodeId]: next.length > limit ? next.slice(next.length - limit) : next };
}

export function upsertContact(contacts: Record<string, Contact>, incoming: ContactData): Record<string, Contact> {
  return { ...contacts, [incoming.node_id]: { ...contacts[incoming.node_id], ...incoming } };
}

export function removeContact(contacts: Record<string, Contact>, nodeId: string): Record<string, Contact> {
  if (!contacts[nodeId]) return contacts;
  const next = { ...contacts };
  delete next[nodeId];
  return next;
}

export function upsertChannel(channels: Record<string, Channel>, incoming: Channel): Record<string, Channel> {
  return { ...channels, [incoming.channel_id]: { ...channels[incoming.channel_id], ...incoming } };
}

/** Deep-partial config patch (set_config accepts any subset of fields). */
export interface ConfigPatch {
  node?: Partial<ConfigData['node']>;
  radio?: Partial<ConfigData['radio']>;
  features?: Partial<ConfigData['features']>;
  channels?: ConfigData['channels'];
}

/** Merge a full config snapshot into node/radio/feature fields. */
export function mergeConfig(config: ConfigData | null, patch: ConfigPatch): ConfigData | null {
  if (!config) return (patch as ConfigData) ?? null;
  return {
    node: { ...config.node, ...(patch.node ?? {}) },
    radio: { ...config.radio, ...(patch.radio ?? {}) },
    features: { ...config.features, ...(patch.features ?? {}) },
    channels: patch.channels ?? config.channels,
  };
}

/* ------------------------------------------------------------------ */
/* Conversations                                                      */
/* ------------------------------------------------------------------ */

export interface ConversationSource {
  messages: ChatMessage[];
  groupMessages: ChatGroupMessage[];
  nodes: Record<string, NodeData>;
  channels: Record<string, Channel>;
  contacts: Record<string, Contact>;
}

export interface ConversationSummary {
  id: string;
  kind: 'direct' | 'group';
  name: string;
  address: string;
  subtitle: string;
  lastText: string;
  lastAt: number;
  unread: number;
  delivery?: DeliveryState;
  outgoing?: boolean;
}

/**
 * Build the chat sidebar model: every channel, every known contact and every
 * peer we have exchanged messages with, newest first.
 */
export function buildConversations(source: ConversationSource): ConversationSummary[] {
  const { messages, groupMessages, nodes, channels, contacts } = source;
  const directPeers = new Set<string>([
    ...Object.keys(contacts),
    ...Object.keys(nodes).filter((id) => nodes[id]?.is_contact),
    ...messages.map((m) => m.peer),
  ]);

  const rows: ConversationSummary[] = [];

  for (const peer of directPeers) {
    const peerMessages = messages.filter((m) => m.peer === peer);
    if (peerMessages.length === 0 && !contacts[peer]) continue;
    const last = peerMessages[peerMessages.length - 1];
    const node = nodes[peer];
    const contact = contacts[peer];
    const name = contact?.name || node?.name || `Node ${peer.slice(0, 4)}`;
    rows.push({
      id: peer,
      kind: 'direct',
      name,
      address: peer,
      subtitle: node ? `${node.hops} ${node.hops === 1 ? 'hop' : 'hops'} · ${node.rssi ?? '—'} dBm` : 'contact',
      lastText: last ? describeLastMessage(last) : 'Say hello',
      lastAt: last?.sent_at_ms ?? 0,
      unread: unreadCount(peerMessages),
      delivery: last?.delivery,
      outgoing: last?.outgoing,
    });
  }

  for (const channel of Object.values(channels)) {
    const channelMessages = groupMessages.filter((m) => m.channel_id === channel.channel_id);
    const last = channelMessages[channelMessages.length - 1];
    rows.push({
      id: channel.channel_id,
      kind: 'group',
      name: channel.name,
      address: channel.channel_id,
      subtitle: `${channelMessages.length} ${channelMessages.length === 1 ? 'message' : 'messages'}`,
      lastText: last ? describeLastGroupMessage(last) : 'Channel created — say hello',
      lastAt: last?.sent_at_ms ?? 0,
      unread: last && !last.outgoing ? 1 : 0,
    });
  }

  // De-duplicate direct rows if a peer is also in contacts twice.
  const seen = new Set<string>();
  const unique = rows.filter((row) => {
    if (seen.has(row.id)) return false;
    seen.add(row.id);
    return true;
  });
  unique.sort((a, b) => b.lastAt - a.lastAt);
  return unique;
}

function describeLastMessage(message: ChatMessage): string {
  if (message.kind === 'attachment') {
    if (message.attachment?.mime.startsWith('audio/')) return 'Voice message';
    if (message.attachment?.mime.startsWith('image/')) return 'Photo';
    return 'Attachment';
  }
  if (message.kind === 'position') return 'Shared a position';
  if (message.kind === 'telemetry') return 'Telemetry';
  return message.text ?? '';
}

function describeLastGroupMessage(message: ChatGroupMessage): string {
  if (message.kind === 'attachment') {
    if (message.attachment?.mime.startsWith('audio/')) return 'Voice message';
    if (message.attachment?.mime.startsWith('image/')) return 'Photo';
    return 'Attachment';
  }
  if (message.kind === 'position') return 'Shared a position';
  return message.text ?? '';
}

/** Latest message timestamp across direct + group traffic. */
export function latestActivityTimestamp(source: Pick<ConversationSource, 'messages' | 'groupMessages'>): number {
  const direct = source.messages.reduce((max, m) => Math.max(max, m.sent_at_ms), 0);
  const group = source.groupMessages.reduce((max, m) => Math.max(max, m.sent_at_ms), 0);
  return Math.max(direct, group);
}
