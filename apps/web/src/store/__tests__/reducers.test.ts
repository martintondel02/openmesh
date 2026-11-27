import { describe, expect, it } from 'vitest';
import {
  appendPositionHistory,
  applyAck,
  applyGroupMessageUpdate,
  applyMessageUpdate,
  applyResourceProgress,
  resourceProgressFraction,
  buildConversations,
  deriveDelivery,
  markMessageFailed,
  markMessagePending,
  mergeConfig,
  reconcileSendResult,
  removeContact,
  sortMessages,
  unreadCount,
  upsertChannel,
  upsertContact,
  upsertGroupMessage,
  upsertMessage,
  upsertNode,
} from '../reducers';
import type { ChatGroupMessage, ChatMessage, ConfigData, NodeData, Position } from '../../types/api';

function direct(partial: Partial<ChatMessage> & { msg_id: number; peer: string }): ChatMessage {
  return {
    from: partial.outgoing ? 'me' : partial.peer,
    to: partial.outgoing ? partial.peer : 'me',
    kind: 'text',
    text: 'hello',
    sent_at_ms: 1_000,
    outgoing: false,
    delivered: false,
    read: false,
    reply_to: null,
    thread: null,
    attachment: null,
    position: null,
    delivery: 'sent',
    ...partial,
  };
}

function node(id: string, overrides: Partial<NodeData> = {}): NodeData {
  return {
    node_id: id,
    name: `node-${id}`,
    class: 'client',
    caps: ['gps'],
    hops: 1,
    last_seen_ms: 1_000,
    rssi: -90,
    snr: 5,
    position: null,
    battery_pct: 80,
    is_contact: false,
    public_key: null,
    ...overrides,
  };
}

function group(partial: Partial<ChatGroupMessage> & { msg_id: number }): ChatGroupMessage {
  return {
    channel_id: 'chan',
    channel_name: 'e2e test',
    from: 'me',
    from_name: 'PC Bridge',
    sender_key: '',
    kind: 'text',
    text: 'hi',
    sent_at_ms: 1_000,
    outgoing: true,
    signature_valid: true,
    attachment: null,
    position: null,
    ...partial,
  };
}

function attachment(overrides: Partial<NonNullable<ChatGroupMessage['attachment']>> = {}) {
  return {
    id: 'att_1',
    mime: 'image/webp',
    size: 6018,
    sha256: 'abc',
    progress: 1,
    ...overrides,
  };
}

describe('reducers: delivery state', () => {
  it('derives ticks from bridge flags', () => {
    expect(deriveDelivery({ outgoing: false, delivered: true, read: false })).toBe('sent');
    expect(deriveDelivery({ outgoing: true, delivered: false, read: false })).toBe('sent');
    expect(deriveDelivery({ outgoing: true, delivered: true, read: false })).toBe('delivered');
    expect(deriveDelivery({ outgoing: true, delivered: true, read: true })).toBe('read');
    expect(deriveDelivery({ outgoing: true, delivered: false, read: false }, 'pending')).toBe('pending');
    expect(deriveDelivery({ outgoing: true, delivered: false, read: false }, 'failed')).toBe('failed');
  });

  it('inserts and updates direct messages by msg_id', () => {
    const first = upsertMessage([], direct({ msg_id: 1, peer: 'peer' }));
    expect(first.length).toBe(1);
    const updated = upsertMessage(
      first,
      direct({ msg_id: 1, peer: 'peer', text: 'edited', outgoing: true, delivered: true }),
    );
    expect(updated.length).toBe(1);
    expect(updated[0]!.text).toBe('edited');
    expect(updated[0]!.delivery).toBe('delivered');
  });

  it('reconciles an optimistic row by tempId', () => {
    const optimistic = direct({ msg_id: 0, peer: 'peer', tempId: 'local_1', delivery: 'pending', outgoing: true });
    const result = upsertMessage([optimistic], direct({ msg_id: 42, peer: 'peer', outgoing: true }), 'local_1');
    expect(result.length).toBe(1);
    expect(result[0]!.msg_id).toBe(42);
    expect(result[0]!.tempId).toBe('local_1');
    expect(result[0]!.delivery).toBe('sent');
  });

  it('keeps the local attachment preview across an update', () => {
    const optimistic = direct({
      msg_id: 7,
      peer: 'peer',
      outgoing: true,
      localUrl: 'blob:preview',
      attachment: {
        id: 'att_1',
        mime: 'image/jpeg',
        size: 100,
        sha256: 'x',
        progress: 1,
        duration_ms: null,
        name: 'a.jpg',
        width: 10,
        height: 10,
      },
    });
    const result = upsertMessage([optimistic], direct({ msg_id: 7, peer: 'peer', outgoing: true, attachment: null }));
    expect(result[0]!.localUrl).toBe('blob:preview');
    expect(result[0]!.attachment?.id).toBe('att_1');
  });

  it('applies partial message updates', () => {
    const messages = [direct({ msg_id: 5, peer: 'peer', outgoing: true })];
    const updated = applyMessageUpdate(messages, { msg_id: 5, delivered: true });
    expect(updated[0]!.delivered).toBe(true);
    expect(updated[0]!.text).toBe('hello');
    const missing = applyMessageUpdate(messages, { msg_id: 999, delivered: true });
    expect(missing).toBe(messages);
  });

  it('applies acks and makes read imply delivered', () => {
    const messages = [direct({ msg_id: 9, peer: 'peer', outgoing: true, delivery: 'sent' })];
    const delivered = applyAck(messages, 9, 'delivered');
    expect(delivered[0]!.delivery).toBe('delivered');
    const read = applyAck(messages, 9, 'read');
    expect(read[0]!.delivery).toBe('read');
    expect(read[0]!.delivered).toBe(true);
    expect(applyAck(messages, 1234, 'delivered')).toBe(messages);
  });

  it('marks pending/failed states', () => {
    const messages = [direct({ msg_id: 3, peer: 'peer', outgoing: true, tempId: 't3', delivery: 'failed' })];
    const pending = markMessagePending(messages, 3);
    expect(pending[0]!.delivery).toBe('pending');
    const failed = markMessageFailed(pending, 't3', 'no route');
    expect(failed[0]!.delivery).toBe('failed');
    expect(failed[0]!.error).toBe('no route');
  });

  it('reconciles send results', () => {
    const messages = [direct({ msg_id: 0, peer: 'peer', outgoing: true, tempId: 't1', delivery: 'pending' })];
    const reconciled = reconcileSendResult(messages, 't1', 77);
    expect(reconciled[0]!.msg_id).toBe(77);
    expect(reconciled[0]!.delivery).toBe('sent');
  });

  it('counts unread incoming messages', () => {
    expect(
      unreadCount([
        direct({ msg_id: 1, peer: 'a', outgoing: false, read: false }),
        direct({ msg_id: 2, peer: 'a', outgoing: false, read: true }),
        direct({ msg_id: 3, peer: 'a', outgoing: true, read: false }),
      ]),
    ).toBe(1);
  });

  it('sorts without mutating the input', () => {
    const messages = [
      direct({ msg_id: 2, peer: 'a', sent_at_ms: 2000 }),
      direct({ msg_id: 1, peer: 'a', sent_at_ms: 1000 }),
    ];
    const sorted = sortMessages(messages);
    expect(sorted.map((message) => message.msg_id)).toEqual([1, 2]);
    expect(messages[0]!.msg_id).toBe(2);
  });
});

describe('reducers: nodes and positions', () => {
  it('merges node updates without losing a known position', () => {
    const nodes = { abc: node('abc', { position: { lat: 1, lon: 2, timestamp_ms: 5 } }) };
    const merged = upsertNode(nodes, node('abc', { position: null, rssi: -100 }));
    expect(merged.abc!.position).toEqual({ lat: 1, lon: 2, timestamp_ms: 5 });
    expect(merged.abc!.rssi).toBe(-100);
  });

  it('appends position history and dedupes identical timestamps', () => {
    const position: Position = { lat: 59.9, lon: 10.7, timestamp_ms: 1000 };
    const history = appendPositionHistory({}, 'abc', position);
    expect(history.abc).toHaveLength(1);
    const duplicate = appendPositionHistory(history, 'abc', position);
    expect(duplicate.abc).toHaveLength(1);
    const next = appendPositionHistory(
      duplicate,
      'abc',
      { lat: 59.91, lon: 10.71, timestamp_ms: 2000 },
    );
    expect(next.abc!.map((entry) => entry.timestamp_ms)).toEqual([1000, 2000]);
  });

  it('caps history length', () => {
    let history = {};
    for (let index = 0; index < 10; index += 1) {
      history = appendPositionHistory(history, 'abc', { lat: 1, lon: 1, timestamp_ms: index }, 5);
    }
    expect((history as Record<string, Position[]>).abc).toHaveLength(5);
  });

  it('upserts and removes contacts', () => {
    let contacts = upsertContact({}, { node_id: 'abc', name: 'Anne', identity: 'id', last_seen_ms: 1 });
    contacts = upsertContact(contacts, { node_id: 'abc', name: 'Anne B', identity: 'id', last_seen_ms: 2 });
    expect(contacts.abc!.name).toBe('Anne B');
    expect(removeContact(contacts, 'abc')).toEqual({});
    expect(removeContact(contacts, 'missing')).toBe(contacts);
  });

  it('upserts channels by id', () => {
    const channels = upsertChannel({}, { channel_id: 'ff', name: 'familien', psk_set: true });
    expect(channels.ff!.name).toBe('familien');
  });
});

describe('reducers: config merge', () => {
  it('deep-merges partial config patches', () => {
    const config: ConfigData = {
      node: { name: 'a', class: 'client', announce_interval_s: 1800 },
      radio: { profile: 'MEDIUM_FAST', region: 'EU868', tx_power_dbm: 14 },
      features: { auto_announce: true, store_forward: true, position_sharing: true },
      channels: [],
    };
    const merged = mergeConfig(config, { node: { name: 'b' } });
    expect(merged!.node.name).toBe('b');
    expect(merged!.node.class).toBe('client');
    expect(merged!.radio.profile).toBe('MEDIUM_FAST');
    expect(mergeConfig(null, config)).toEqual(config);
  });
});

describe('reducers: conversations', () => {
  it('builds channel and direct rows with unread counts', () => {
    const conversations = buildConversations({
      messages: [
        direct({ msg_id: 1, peer: 'anne', sent_at_ms: 1000, outgoing: false, read: false }),
        direct({ msg_id: 2, peer: 'anne', sent_at_ms: 2000, outgoing: false, read: false }),
      ],
      groupMessages: [
        {
          msg_id: 10,
          channel_id: 'chan',
          channel_name: 'familien',
          from: 'anne',
          from_name: 'Anne',
          sender_key: '',
          kind: 'text',
          text: 'dinner?',
          sent_at_ms: 1500,
          outgoing: false,
          signature_valid: true,
          attachment: null,
          position: null,
        },
      ],
      nodes: { anne: node('anne', { name: 'Anne', is_contact: true }) },
      channels: { chan: { channel_id: 'chan', name: 'familien', psk_set: true } },
      contacts: { anne: { node_id: 'anne', name: 'Anne', identity: 'id', last_seen_ms: 1000 } },
    });
    expect(conversations).toHaveLength(2);
    const anne = conversations.find((row) => row.id === 'anne');
    expect(anne?.kind).toBe('direct');
    expect(anne?.unread).toBe(2);
    expect(anne?.lastText).toBe('hello');
    const channel = conversations.find((row) => row.id === 'chan');
    expect(channel?.kind).toBe('group');
    expect(channel?.name).toBe('familien');
  });

  it('describes attachments in previews', () => {
    const conversations = buildConversations({
      messages: [
        direct({
          msg_id: 1,
          peer: 'anne',
          kind: 'attachment',
          text: null,
          attachment: {
            id: 'att',
            mime: 'audio/ogg; codecs=opus',
            size: 1000,
            sha256: '',
            duration_ms: 4000,
            name: 'voice.ogg',
            progress: 1,
          },
        }),
      ],
      groupMessages: [],
      nodes: {},
      channels: {},
      contacts: { anne: { node_id: 'anne', name: 'Anne', identity: 'id', last_seen_ms: 1 } },
    });
    expect(conversations[0]!.lastText).toBe('Voice message');
  });
});

describe('reducers: group messages', () => {
  it('updates an optimistic group message in place', () => {
    const optimistic = {
      msg_id: 0,
      channel_id: 'chan',
      channel_name: 'familien',
      from: 'me',
      from_name: 'me',
      sender_key: '',
      kind: 'text' as const,
      text: 'hi',
      sent_at_ms: 1000,
      outgoing: true,
      signature_valid: true,
      attachment: null,
      position: null,
      tempId: 'local_1',
    };
    const echoed = { ...optimistic, msg_id: 55, tempId: undefined };
    const result = upsertGroupMessage([optimistic], echoed, 'local_1');
    expect(result).toHaveLength(1);
    expect(result[0]!.msg_id).toBe(55);
  });

  it('applies channel attachment updates from message_update', () => {
    // The bridge emits the group message first (attachment null) and the stored attachment via a
    // later message_update; the update must land in groupMessages or received media never shows.
    const pending = group({ msg_id: 9, kind: 'attachment', text: 'fixture image' });
    const stored = attachment({ progress: 1 });
    const updated = applyGroupMessageUpdate([pending], { msg_id: 9, attachment: stored });
    expect(updated).toHaveLength(1);
    expect(updated[0]!.attachment).toEqual(stored);
    expect(updated[0]!.text).toBe('fixture image');

    const rows = [pending];
    const same = applyGroupMessageUpdate(rows, { msg_id: 404, attachment: stored });
    expect(same).toBe(rows);
    expect(same[0]!.attachment).toBeNull();
  });

  it('keeps group-local fields (localUrl/waveform) when an update arrives', () => {
    const optimistic = group({
      msg_id: 0,
      tempId: 'local_1',
      localUrl: 'blob:preview',
      waveform: [0.2, 0.5],
      attachment: attachment({ id: 'att_local', progress: 1 }),
    });
    const echoed = group({ msg_id: 77, kind: 'attachment', text: null });
    const merged = applyGroupMessageUpdate([{ ...optimistic, msg_id: 77 }], {
      msg_id: 77,
      attachment: attachment({ id: 'att_77' }),
    });
    expect(merged[0]!.attachment?.id).toBe('att_77');
    expect(merged[0]!.waveform).toEqual([0.2, 0.5]);
    expect(merged[0]!.localUrl).toBe('blob:preview');
    // The optimistic row itself (msg_id 0) can still be reconciled by tempId via upsertGroupMessage.
    expect(upsertGroupMessage([optimistic], echoed, 'local_1')).toHaveLength(1);
  });
});

describe('reducers: resource progress', () => {
  it('clamps progress into 0..1', () => {
    expect(resourceProgressFraction(5, 10)).toBe(0.5);
    expect(resourceProgressFraction(20, 10)).toBe(1);
    expect(resourceProgressFraction(-1, 10)).toBe(0);
    expect(resourceProgressFraction(5, 0)).toBe(0);
    expect(resourceProgressFraction(Number.NaN, 10)).toBe(0);
  });

  it('updates direct and channel attachments by msg_id only', () => {
    const dm = direct({ msg_id: 3, peer: 'peer', attachment: attachment({ id: 'att_dm', progress: 0 }) });
    const gm = group({ msg_id: 4, kind: 'attachment', attachment: attachment({ id: 'att_gm', progress: 0 }) });
    const messages = [dm];
    const groups = [gm];

    const nextMessages = applyResourceProgress(messages, 3, 0.5);
    expect(nextMessages[0]!.attachment?.progress).toBe(0.5);
    const nextGroups = applyResourceProgress(groups, 4, 2);
    expect(nextGroups[0]!.attachment?.progress).toBe(1);
    // Unknown ids and rows without an attachment keep the same array (no re-render churn).
    expect(applyResourceProgress(messages, 999, 0.5)).toBe(messages);
    const textOnly = [direct({ msg_id: 5, peer: 'peer' })];
    expect(applyResourceProgress(textOnly, 5, 0.5)).toBe(textOnly);
  });
});
