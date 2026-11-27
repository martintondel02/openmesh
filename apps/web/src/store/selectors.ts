/**
 * Derived view models. Kept out of the store so components can memoise them and
 * tests can assert sidebar/thread behaviour without React.
 */

import type { Channel, ChatGroupMessage, ChatMessage, Conversation, NodeData, RoleClass } from '../types/api';
import type { AppState } from './app';
import { buildConversations } from './reducers';
import { formatLastSeen, formatRole } from '../lib/utils';
import { displayName } from '../lib/display';

type StateSlice = Pick<
  AppState,
  'messages' | 'groupMessages' | 'nodes' | 'channels' | 'contacts' | 'readCursors' | 'activeThread' | 'ownNodeId'
>;

/** Unread counts per conversation id, honouring per-channel read cursors. */
export function selectUnreadCounts(state: StateSlice): Record<string, number> {
  const counts: Record<string, number> = {};
  for (const message of state.messages) {
    if (message.outgoing || message.read) continue;
    counts[message.peer] = (counts[message.peer] ?? 0) + 1;
  }
  for (const message of state.groupMessages) {
    if (message.outgoing) continue;
    const cursor = state.readCursors[message.channel_id] ?? 0;
    if (message.sent_at_ms > cursor) {
      counts[message.channel_id] = (counts[message.channel_id] ?? 0) + 1;
    }
  }
  return counts;
}

export function selectTotalUnread(state: StateSlice): number {
  return Object.values(selectUnreadCounts(state)).reduce((sum, value) => sum + value, 0);
}

/** Chat sidebar rows: channels + contacts + anyone we have traffic with. */
export function selectConversations(state: StateSlice, nowMs = Date.now()): Conversation[] {
  const rows = buildConversations({
    messages: state.messages,
    groupMessages: state.groupMessages,
    nodes: state.nodes,
    channels: state.channels,
    contacts: state.contacts,
  });
  const unread = selectUnreadCounts(state);
  return rows.map((row) => {
    const node = state.nodes[row.id];
    const channel = state.channels[row.id];
    const online = row.kind === 'direct' ? (node ? nowMs - node.last_seen_ms < 10 * 60_000 : false) : true;
    const subtitle =
      row.kind === 'direct'
        ? node
          ? `${formatRole(node.class)} · ${node.hops} ${node.hops === 1 ? 'hop' : 'hops'}${online ? '' : ` · ${formatLastSeen(node.last_seen_ms, nowMs)}`}`
          : 'Direct message'
        : `${channel ? 'Shared channel' : 'Channel'} · E2E encrypted`;
    return {
      id: row.id,
      kind: row.kind,
      name: row.name,
      address: row.address,
      subtitle,
      lastText: row.lastText,
      lastAt: row.lastAt,
      unread: unread[row.id] ?? 0,
      online,
      role: node?.class,
      hops: node?.hops,
      delivery: row.delivery,
      outgoing: row.outgoing,
    } satisfies Conversation;
  });
}

export interface ThreadHeaderModel {
  name: string;
  subtitle: string;
  kind: 'direct' | 'group';
  role: RoleClass | null;
  hops: number | null;
  node: NodeData | null;
  channel: Channel | null;
  online: boolean;
  address: string;
}

export function selectThreadHeader(state: StateSlice, nowMs = Date.now()): ThreadHeaderModel | null {
  const thread = state.activeThread;
  if (!thread) return null;
  if (thread.kind === 'direct') {
    const node = state.nodes[thread.id] ?? null;
    const contact = state.contacts[thread.id];
    // Announced name first, then the pinned contact alias, then `node <first8>` (task-13).
    const name = displayName({ announcedName: node?.name, contactName: contact?.name, nodeId: thread.id });
    const online = node ? nowMs - node.last_seen_ms < 10 * 60_000 : false;
    return {
      name,
      subtitle: node
        ? `${formatRole(node.class)} · ${node.hops} ${node.hops === 1 ? 'hop' : 'hops'} · ${node.rssi ?? '—'} dBm${online ? '' : ` · ${formatLastSeen(node.last_seen_ms, nowMs)}`}`
        : 'Direct message',
      kind: 'direct',
      role: node?.class ?? 'client',
      hops: node?.hops ?? null,
      node,
      channel: null,
      online,
      address: thread.id,
    };
  }
  const channel = state.channels[thread.id] ?? null;
  const count = state.groupMessages.filter((m) => m.channel_id === thread.id).length;
  return {
    name: channel?.name ?? thread.id.slice(0, 8),
    subtitle: `Channel · ${count} ${count === 1 ? 'message' : 'messages'} · signed`,
    kind: 'group',
    role: null,
    hops: null,
    node: null,
    channel,
    online: true,
    address: thread.id,
  };
}

/** Nodes sorted so infrastructure first, then clients, then trackers. */
const ROLE_ORDER: Record<string, number> = {
  gateway: 0,
  repeater: 1,
  room_server: 2,
  client: 3,
  tracker: 4,
};

export function selectNodesSorted(nodes: Record<string, NodeData>): NodeData[] {
  return Object.values(nodes).sort((a, b) => {
    const roleDelta = (ROLE_ORDER[a.class] ?? 9) - (ROLE_ORDER[b.class] ?? 9);
    if (roleDelta !== 0) return roleDelta;
    return a.name.localeCompare(b.name);
  });
}

/** Best signal among known nodes, used by the map/nodes summary strip. */
export function selectMeshSummary(state: Pick<AppState, 'nodes' | 'channels' | 'contacts' | 'messages'>) {
  const nodes = Object.values(state.nodes);
  return {
    nodes: nodes.length,
    contacts: Object.keys(state.contacts).length,
    channels: Object.keys(state.channels).length,
    repeaters: nodes.filter((n) => n.class === 'repeater').length,
    gateways: nodes.filter((n) => n.class === 'gateway').length,
    trackers: nodes.filter((n) => n.class === 'tracker').length,
    withPosition: nodes.filter((n) => n.position).length,
    bestRssi: nodes.reduce<number | null>((best, node) => {
      if (node.rssi === null) return best;
      return best === null ? node.rssi : Math.max(best, node.rssi);
    }, null),
    unread: selectTotalUnread(state as StateSlice),
  };
}

/** Thread messages sorted ascending, used by the chat view. */
export function selectThreadMessages(
  state: Pick<AppState, 'messages' | 'groupMessages' | 'activeThread'>,
): { direct: ChatMessage[]; group: ChatGroupMessage[] } {
  const thread = state.activeThread;
  if (!thread) return { direct: [], group: [] };
  if (thread.kind === 'direct') {
    return {
      direct: state.messages.filter((m) => m.peer === thread.id).sort((a, b) => a.sent_at_ms - b.sent_at_ms),
      group: [],
    };
  }
  return {
    direct: [],
    group: state.groupMessages
      .filter((m) => m.channel_id === thread.id)
      .sort((a, b) => a.sent_at_ms - b.sent_at_ms),
  };
}
