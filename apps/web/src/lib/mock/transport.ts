/**
 * Demo transport: a fully local stand-in for `openmesh-bridge`.
 *
 * It speaks the same envelope shapes as the live WebSocket client, replays the
 * seeded dataset's initial state (hello/config/nodes/contacts/channels) and then
 * keeps the mesh feeling alive with status ticks, delivery acks, incoming
 * messages and slowly moving nodes. Commands are answered the way the bridge
 * answers them (send_result + echo + ack, config snapshots, channel creation, …)
 * so no UI code needs to know it is running without a backend.
 */

import type {
  AckData,
  ChannelCreatedData,
  ChannelsEventData,
  ConfigData,
  ContactData,
  ContactsEventData,
  Envelope,
  GroupMessageData,
  MessageData,
  NodeData,
  Position,
  StatusData,
} from '../../types/api';
import type { MeshTransport, TransportStatus, UploadedAttachment } from '../transport';
import { nextCommandId } from '../transport';
import type { DemoDataset } from './data';
import { createDemoDataset, demoChannelUrl, demoContactUrl } from './data';
import { buildContactLink } from '../links';
import { generateChannelPsk, bytesToBase64 } from '../utils';
import { mulberry32 } from '../audio';
import { estimateText } from '../airtime';
import { bytesToHex } from '../utils';

const tick = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

function hash32(input: string, seed = 0x811c9dc5): number {
  let hash = seed;
  for (let i = 0; i < input.length; i += 1) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash >>> 0;
}

function channelIdFor(name: string, psk: string): string {
  const a = hash32(`${name}|${psk}`).toString(16).padStart(8, '0');
  const b = hash32(psk, hash32(name)).toString(16).padStart(8, '0');
  return a + b;
}

interface AmbientEvent {
  kind: 'message' | 'group_message' | 'position' | 'store_offer';
  data: Record<string, unknown>;
}

export interface MockTransportOptions {
  dataset?: DemoDataset;
  onEvent?: (event: Envelope) => void;
  onStatus?: (status: TransportStatus) => void;
  /** Status tick period, ms. */
  statusIntervalMs?: number;
  /** Ambient mesh activity period, ms (realistic but slow; 0 disables). */
  activityIntervalMs?: number;
}

export class MockTransport implements MeshTransport {
  readonly mode = 'demo' as const;
  readonly dataset: DemoDataset;

  private readonly eventListeners = new Set<(event: Envelope) => void>();
  private readonly statusListeners = new Set<(status: TransportStatus) => void>();
  private timers = new Set<ReturnType<typeof setTimeout>>();
  private objectUrls = new Map<string, string>();
  private connected = false;
  private disposed = false;
  private nextMsgId: number;
  private activityIndex = 0;
  private uptime: number;
  private readonly statusIntervalMs: number;
  private readonly activityIntervalMs: number;
  private readonly rand: () => number;
  private channels: ConfigData['channels'];
  private contacts: ContactData[];
  private nodes: NodeData[];
  private config: ConfigData;
  private ownName: string;

  constructor(options: MockTransportOptions = {}) {
    this.dataset = options.dataset ?? createDemoDataset();
    this.config = structuredCloneSafe(this.dataset.config);
    this.channels = structuredCloneSafe(this.dataset.channels);
    this.contacts = structuredCloneSafe(this.dataset.contacts);
    this.nodes = structuredCloneSafe(this.dataset.nodes);
    this.ownName = this.dataset.ownName;
    this.uptime = this.dataset.status.uptime_s;
    this.nextMsgId = 9000;
    this.statusIntervalMs = options.statusIntervalMs ?? 4_000;
    this.activityIntervalMs = options.activityIntervalMs ?? 26_000;
    this.rand = mulberry32(424242);
    if (options.onEvent) this.eventListeners.add(options.onEvent);
    if (options.onStatus) this.statusListeners.add(options.onStatus);
  }

  /* ---------------- plumbing ---------------- */

  onEvent(listener: (event: Envelope) => void): () => void {
    this.eventListeners.add(listener);
    return () => this.eventListeners.delete(listener);
  }

  onStatus(listener: (status: TransportStatus) => void): () => void {
    this.statusListeners.add(listener);
    return () => this.statusListeners.delete(listener);
  }

  private emitStatus(status: TransportStatus): void {
    for (const listener of this.statusListeners) listener(status);
  }

  private emit(type: string, data?: Record<string, unknown>, id?: string): void {
    const envelope: Envelope = {
      v: 1,
      type,
      ...(id ? { id } : {}),
      ...(data !== undefined ? { data } : {}),
    };
    for (const listener of this.eventListeners) listener(envelope);
  }

  private later(fn: () => void, delayMs: number): void {
    if (this.disposed) return;
    const timer = setTimeout(() => {
      this.timers.delete(timer);
      if (!this.disposed) fn();
    }, delayMs);
    this.timers.add(timer);
  }

  private clearTimers(): void {
    for (const timer of this.timers) clearTimeout(timer);
    this.timers.clear();
  }

  /* ---------------- lifecycle ---------------- */

  connect(): void {
    if (this.disposed || this.connected) return;
    this.connected = true;
    this.emitStatus({ state: 'demo', detail: 'Seeded demo mesh — no bridge required' });

    const { hello, status } = this.dataset;
    this.later(() => this.emit('hello', hello as unknown as Record<string, unknown>), 40);
    this.later(() => this.emit('config', this.config as unknown as Record<string, unknown>), 80);
    this.later(() => this.emit('nodes', { nodes: this.nodes }), 120);
    this.later(() => this.emit('contacts', { contacts: this.contacts }), 140);
    this.later(() => this.emit('channels', { channels: this.channels }), 160);
    this.later(() => this.emit('status', status as unknown as Record<string, unknown>), 180);
    this.later(() => this.emit('sync_done', { messages: 12, nodes: this.nodes.length }), 220);

    this.startStatusLoop();
    if (this.activityIntervalMs > 0) this.startActivityLoop();
  }

  private startStatusLoop(): void {
    const schedule = () => {
      this.later(() => {
        this.uptime += Math.round(this.statusIntervalMs / 1000);
        const jitter = Math.round(this.rand() * 10) / 10;
        const status: StatusData = {
          ...this.dataset.status,
          uptime_s: this.uptime,
          radio: {
            ...this.dataset.status.radio,
            airtime_used_pct: Math.round((0.3 + jitter) * 100) / 100,
            rssi: -87 + Math.round((this.rand() - 0.5) * 6),
            snr: Math.round((6.2 + (this.rand() - 0.5) * 2) * 100) / 100,
            tx_queue: this.rand() < 0.15 ? 1 : 0,
          },
          mesh: {
            ...this.dataset.status.mesh,
            tx_frames: this.dataset.status.mesh.tx_frames + Math.round(this.uptime / 7),
            rx_frames: this.dataset.status.mesh.rx_frames + Math.round(this.uptime / 3),
            duty_cycle_pct: Math.round((0.3 + jitter) * 100) / 100,
          },
        };
        this.emit('status', status as unknown as Record<string, unknown>);
        schedule();
      }, this.statusIntervalMs);
    };
    schedule();
  }

  private startActivityLoop(): void {
    const schedule = () => {
      this.later(() => {
        const ambient = this.nextAmbientEvent();
        this.emit(ambient.kind, ambient.data);
        schedule();
      }, this.activityIntervalMs);
    };
    schedule();
  }

  private nextAmbientEvent(): AmbientEvent {
    const { ownNodeId } = this.dataset;
    const events: AmbientEvent[] = [
      {
        kind: 'message',
        data: {
          msg_id: this.nextMsgId++,
          peer: this.dataset.contacts[0]!.node_id,
          from: this.dataset.contacts[0]!.node_id,
          to: ownNodeId,
          kind: 'text',
          text: 'Key retrieved. Cabin is warm — come up when you can.',
          sent_at_ms: Date.now(),
          received_at_ms: Date.now(),
          outgoing: false,
          delivered: true,
          read: false,
          reply_to: null,
          thread: null,
          attachment: null,
          position: null,
        },
      },
      {
        kind: 'position',
        data: {
          node_id: this.dataset.nodes[5]!.node_id,
          channel_id: null,
          position: this.nudgePosition(this.dataset.nodes[5]!.position, 0.0016),
        },
      },
      {
        kind: 'message',
        data: {
          msg_id: this.nextMsgId++,
          peer: this.dataset.contacts[1]!.node_id,
          from: this.dataset.contacts[1]!.node_id,
          to: ownNodeId,
          kind: 'text',
          text: 'Ridge has a clear LoRa path today — 3 hops to the gateway.',
          sent_at_ms: Date.now(),
          received_at_ms: Date.now(),
          outgoing: false,
          delivered: true,
          read: false,
          reply_to: null,
          thread: null,
          attachment: null,
          position: null,
        },
      },
      {
        kind: 'group_message',
        data: {
          msg_id: this.nextMsgId++,
          channel_id: this.dataset.channels[0]!.channel_id,
          channel_name: this.dataset.channels[0]!.name,
          from: this.dataset.contacts[0]!.node_id,
          from_name: 'Anne',
          sender_key: this.dataset.identities[this.dataset.contacts[0]!.node_id]!,
          kind: 'text',
          text: 'Table booked for 18:00. Bring the mustard again.',
          sent_at_ms: Date.now(),
          outgoing: false,
          signature_valid: true,
          attachment: null,
          position: null,
        },
      },
      {
        kind: 'store_offer',
        data: { relay: this.dataset.nodes[2]!.node_id, count: 3 },
      },
      {
        kind: 'group_message',
        data: {
          msg_id: this.nextMsgId++,
          channel_id: this.dataset.channels[1]!.channel_id,
          channel_name: this.dataset.channels[1]!.name,
          from: this.dataset.nodes[6]!.node_id,
          from_name: 'trail-tracker',
          sender_key: this.dataset.identities[this.dataset.nodes[6]!.node_id]!,
          kind: 'position',
          text: null,
          sent_at_ms: Date.now(),
          outgoing: false,
          signature_valid: true,
          attachment: null,
          position: this.nudgePosition(this.dataset.nodes[6]!.position, 0.002),
        },
      },
    ];
    const event = events[this.activityIndex % events.length]!;
    this.activityIndex += 1;
    return event;
  }

  private nudgePosition(base: Position | null, step: number): Position {
    const lat = (base?.lat ?? 59.93) + (this.rand() - 0.5) * step;
    const lon = (base?.lon ?? 10.75) + (this.rand() - 0.5) * step;
    return {
      lat: Math.round(lat * 1e6) / 1e6,
      lon: Math.round(lon * 1e6) / 1e6,
      alt_m: base?.alt_m ?? 120,
      accuracy_m: 8,
      timestamp_ms: Date.now(),
    };
  }

  disconnect(): void {
    this.connected = false;
    this.clearTimers();
    this.emitStatus({ state: 'idle' });
  }

  dispose(): void {
    this.disposed = true;
    this.disconnect();
    for (const url of this.objectUrls.values()) URL.revokeObjectURL(url);
    this.objectUrls.clear();
    this.eventListeners.clear();
    this.statusListeners.clear();
  }

  /* ---------------- commands ---------------- */

  send(type: string, data: Record<string, unknown> = {}, id?: string): string {
    const commandId = id ?? nextCommandId();
    this.later(() => this.handleCommand(commandId, type, data), 90);
    return commandId;
  }

  private handleCommand(commandId: string, type: string, data: Record<string, unknown>): void {
    switch (type) {
      case 'sync':
        this.emit('sync_done', { messages: 12, nodes: this.nodes.length }, commandId);
        return;
      case 'get_config':
        this.emit('config', this.config as unknown as Record<string, unknown>, commandId);
        return;
      case 'set_config':
        this.applyConfigPatch(data as Partial<ConfigData>);
        this.emit('config', this.config as unknown as Record<string, unknown>, commandId);
        return;
      case 'get_nodes':
      case 'get_nodes_map':
        this.emit('nodes', { nodes: this.nodes }, commandId);
        return;
      case 'get_contacts':
        this.emit('contacts', { contacts: this.contacts }, commandId);
        return;
      case 'get_channels':
        this.emit('channels', { channels: this.channels }, commandId);
        return;
      case 'get_messages': {
        const peer = String(data.peer ?? '');
        const messages = this.dataset.messages.filter((m) => m.peer === peer);
        this.emit('messages', { messages }, commandId);
        return;
      }
      case 'get_stats':
        this.emit('status', this.dataset.status as unknown as Record<string, unknown>, commandId);
        return;
      case 'send_message':
        this.handleSendMessage(commandId, data);
        return;
      case 'send_group_message':
        this.handleSendGroupMessage(commandId, data);
        return;
      case 'send_position':
        this.handleSendPosition(commandId, data);
        return;
      case 'mark_read':
        this.handleMarkRead(data);
        return;
      case 'retry':
        this.handleRetry(data);
        return;
      case 'delete_message':
        return;
      case 'add_contact':
        this.handleAddContact(commandId, data);
        return;
      case 'remove_contact':
        this.handleRemoveContact(commandId, data);
        return;
      case 'contact_qr':
        this.handleContactQr(commandId, data);
        return;
      case 'create_channel':
        this.handleCreateChannel(commandId, data);
        return;
      case 'join_channel':
        this.handleJoinChannel(commandId, data);
        return;
      case 'leave_channel':
        this.handleLeaveChannel(commandId, data);
        return;
      case 'channel_invite':
        this.handleChannelInvite(commandId, data);
        return;
      case 'announce':
        this.emit('status', this.dataset.status as unknown as Record<string, unknown>, commandId);
        return;
      case 'radio_scan':
        this.emit('error', { code: 'radio_busy', message: 'Scan is not supported by the simulated radio.' }, commandId);
        return;
      default:
        this.emit('error', { code: 'bad_request', message: `Unknown command “${type}”` }, commandId);
    }
  }

  private handleSendMessage(commandId: string, data: Record<string, unknown>): void {
    const to = String(data.to ?? '');
    const text = typeof data.text === 'string' ? data.text : '';
    const replyTo = typeof data.reply_to === 'number' ? data.reply_to : null;
    const attachmentId = typeof data.attachment_id === 'string' ? data.attachment_id : null;
    const msgId = this.nextMsgId++;
    this.emit(
      'send_result',
      {
        msg_id: msgId,
        est_airtime_ms: estimateText(text, this.config.radio.profile).airtimeMs,
        queued: true,
      },
      commandId,
    );
    this.later(() => {
      const message: MessageData = {
        msg_id: msgId,
        peer: to,
        from: this.dataset.ownNodeId,
        to,
        kind: attachmentId ? 'attachment' : 'text',
        text: attachmentId ? null : text,
        sent_at_ms: Date.now(),
        received_at_ms: Date.now(),
        outgoing: true,
        delivered: false,
        read: false,
        reply_to: replyTo,
        thread: null,
        attachment: attachmentId
          ? {
              id: attachmentId,
              mime: 'application/octet-stream',
              size: 0,
              sha256: '',
              duration_ms: null,
              name: null,
              progress: 1,
            }
          : null,
        position: null,
      };
      this.emit('message', message as unknown as Record<string, unknown>);
      this.later(() => this.emit('ack', { msg_id: msgId, peer: to, kind: 'delivered', at_ms: Date.now() }), 1_400);
      this.later(() => this.emit('ack', { msg_id: msgId, peer: to, kind: 'read', at_ms: Date.now() }), 3_600);
    }, 260);
  }

  private handleSendGroupMessage(commandId: string, data: Record<string, unknown>): void {
    const channelId = String(data.channel_id ?? '');
    const channel = this.channels.find((c) => c.channel_id === channelId);
    const msgId = this.nextMsgId++;
    this.emit('send_result', { msg_id: msgId, est_airtime_ms: 610, queued: true }, commandId);
    this.later(() => {
      const message: GroupMessageData = {
        msg_id: msgId,
        channel_id: channelId,
        channel_name: channel?.name ?? 'channel',
        from: this.dataset.ownNodeId,
        from_name: this.ownName,
        sender_key: this.dataset.identities[this.dataset.ownNodeId]!,
        kind: typeof data.attachment_id === 'string' ? 'attachment' : 'text',
        text: typeof data.text === 'string' ? data.text : null,
        sent_at_ms: Date.now(),
        outgoing: true,
        signature_valid: true,
        attachment: null,
        position: null,
      };
      this.emit('group_message', message as unknown as Record<string, unknown>);
    }, 280);
  }

  private handleSendPosition(_commandId: string, data: Record<string, unknown>): void {
    const position = (data.position as Position | undefined) ?? this.dataset.ownPosition;
    const channelId = typeof data.channel_id === 'string' ? data.channel_id : null;
    const to = typeof data.to === 'string' ? data.to : null;
    this.emit('position', {
      node_id: this.dataset.ownNodeId,
      channel_id: channelId,
      position: { ...position, timestamp_ms: Date.now() },
    });
    this.later(() => {
      if (channelId) {
        const channel = this.channels.find((c) => c.channel_id === channelId);
        this.emit('group_message', {
          msg_id: this.nextMsgId++,
          channel_id: channelId,
          channel_name: channel?.name ?? 'channel',
          from: this.dataset.ownNodeId,
          from_name: this.ownName,
          sender_key: this.dataset.identities[this.dataset.ownNodeId]!,
          kind: 'position',
          text: null,
          sent_at_ms: Date.now(),
          outgoing: true,
          signature_valid: true,
          attachment: null,
          position: { ...position, timestamp_ms: Date.now() },
        });
      } else if (to) {
        this.emit('message', {
          msg_id: this.nextMsgId++,
          peer: to,
          from: this.dataset.ownNodeId,
          to,
          kind: 'position',
          text: null,
          sent_at_ms: Date.now(),
          received_at_ms: Date.now(),
          outgoing: true,
          delivered: true,
          read: false,
          reply_to: null,
          thread: null,
          attachment: null,
          position: { ...position, timestamp_ms: Date.now() },
        });
      }
    }, 240);
  }

  private findMessage(msgId: number): { peer: string; msg: DemoDataset['messages'][number] | null } {
    const msg = this.dataset.messages.find((m) => m.msg_id === msgId) ?? null;
    return { peer: msg?.peer ?? '', msg };
  }

  private handleMarkRead(data: Record<string, unknown>): void {
    const msgId = Number(data.msg_id ?? 0);
    const { peer } = this.findMessage(msgId);
    if (!peer) return;
    this.later(() => {
      this.emit('ack', { msg_id: msgId, peer, kind: 'read', at_ms: Date.now() } satisfies AckData);
      this.emit('message_update', { msg_id: msgId, read: true });
    }, 120);
  }

  private handleRetry(data: Record<string, unknown>): void {
    const msgId = Number(data.msg_id ?? 0);
    const { peer } = this.findMessage(msgId);
    this.later(() => {
      this.emit('message_update', { msg_id: msgId, delivered: false, read: false });
    }, 80);
    this.later(() => {
      this.emit('ack', { msg_id: msgId, peer, kind: 'delivered', at_ms: Date.now() } satisfies AckData);
      this.emit('message_update', { msg_id: msgId, delivered: true });
    }, 900);
  }

  private handleAddContact(commandId: string, data: Record<string, unknown>): void {
    const nodeId = String(data.node_id ?? '').toLowerCase();
    const name = String(data.name ?? 'Contact');
    const identity = typeof data.identity === 'string' ? data.identity : this.dataset.identities[nodeId] ?? buildDemoIdentityBytes(nodeId);
    const existing = this.contacts.find((c) => c.node_id === nodeId);
    const contact: ContactData = { node_id: nodeId, name, identity, last_seen_ms: Date.now() };
    this.contacts = existing ? this.contacts.map((c) => (c.node_id === nodeId ? contact : c)) : [...this.contacts, contact];
    const node = this.nodes.find((n) => n.node_id === nodeId);
    if (node) {
      node.is_contact = true;
      node.name = name;
      this.emit('node', node as unknown as Record<string, unknown>);
    } else if (nodeId.length === 16) {
      const synthetic: NodeData = {
        node_id: nodeId,
        name,
        class: 'client',
        caps: ['gps'],
        hops: 2,
        last_seen_ms: Date.now(),
        rssi: -95,
        snr: 5,
        position: null,
        battery_pct: 70,
        is_contact: true,
        public_key: identity,
      };
      this.nodes = [...this.nodes, synthetic];
      this.emit('node', synthetic as unknown as Record<string, unknown>);
    }
    this.emit('contacts', { contacts: this.contacts } satisfies ContactsEventData, commandId);
  }

  private handleRemoveContact(commandId: string, data: Record<string, unknown>): void {
    const nodeId = String(data.node_id ?? '').toLowerCase();
    this.contacts = this.contacts.filter((c) => c.node_id !== nodeId);
    const node = this.nodes.find((n) => n.node_id === nodeId);
    if (node) {
      node.is_contact = false;
      this.emit('node', node as unknown as Record<string, unknown>);
    }
    this.emit('contacts', { contacts: this.contacts } satisfies ContactsEventData, commandId);
  }

  private handleContactQr(commandId: string, data: Record<string, unknown>): void {
    const nodeId = String(data.node_id ?? '').toLowerCase();
    const contact = this.contacts.find((c) => c.node_id === nodeId);
    const url =
      contact?.identity && contact?.name
        ? buildContactLink({ nodeId, name: contact.name, identity: contact.identity })
        : demoContactUrl(this.dataset, nodeId);
    this.emit('contact_url', { url }, commandId);
  }

  private handleCreateChannel(commandId: string, data: Record<string, unknown>): void {
    const name = String(data.name ?? '').trim() || 'channel';
    const psk = typeof data.psk === 'string' && data.psk ? data.psk : generateChannelPsk();
    const channelId = channelIdFor(name, psk);
    if (!this.channels.some((c) => c.channel_id === channelId)) {
      this.channels = [...this.channels, { channel_id: channelId, name, psk_set: true }];
      this.config = { ...this.config, channels: this.channels };
      this.dataset.channelPsks[channelId] = psk;
    }
    this.emit('channel_created', { channel_id: channelId, name, psk } satisfies ChannelCreatedData, commandId);
    this.emit('channels', { channels: this.channels } satisfies ChannelsEventData);
    this.emit('config', this.config as unknown as Record<string, unknown>);
  }

  private handleJoinChannel(commandId: string, data: Record<string, unknown>): void {
    const name = String(data.name ?? '').trim() || 'channel';
    const psk = typeof data.psk === 'string' && data.psk ? data.psk : generateChannelPsk();
    const existing = this.channels.find((c) => c.name === name);
    const channelId = existing?.channel_id ?? channelIdFor(name, psk);
    if (!existing) {
      this.channels = [...this.channels, { channel_id: channelId, name, psk_set: true }];
      this.config = { ...this.config, channels: this.channels };
      this.dataset.channelPsks[channelId] = psk;
    }
    this.emit('channel_created', { channel_id: channelId, name } satisfies ChannelCreatedData, commandId);
    this.emit('channels', { channels: this.channels } satisfies ChannelsEventData);
    this.emit('config', this.config as unknown as Record<string, unknown>);
  }

  private handleLeaveChannel(commandId: string, data: Record<string, unknown>): void {
    const channelId = String(data.channel_id ?? '');
    this.channels = this.channels.filter((c) => c.channel_id !== channelId);
    this.config = { ...this.config, channels: this.channels };
    this.emit('channels', { channels: this.channels } satisfies ChannelsEventData, commandId);
    this.emit('config', this.config as unknown as Record<string, unknown>);
  }

  private handleChannelInvite(commandId: string, data: Record<string, unknown>): void {
    const channelId = String(data.channel_id ?? '');
    this.emit('channel_url', { url: demoChannelUrl(this.dataset, channelId) }, commandId);
  }

  private applyConfigPatch(patch: Partial<ConfigData>): void {
    if (patch.node) {
      this.config = { ...this.config, node: { ...this.config.node, ...patch.node } };
      this.ownName = this.config.node.name || this.ownName;
    }
    if (patch.radio) {
      const merged = { ...this.config.radio, ...patch.radio };
      this.config = { ...this.config, radio: merged };
      const ownNode = this.nodes.find((n) => n.node_id === this.dataset.ownNodeId);
      if (ownNode) ownNode.name = this.ownName;
    }
    if (patch.features) {
      this.config = { ...this.config, features: { ...this.config.features, ...patch.features } };
    }
    // The bridge returns a full config snapshot after set_config (WEBSOCKET-API §3.12).
    this.config = { ...this.config, channels: this.channels };
  }

  /* ---------------- attachments ---------------- */

  async uploadAttachment(blob: Blob, filename?: string): Promise<UploadedAttachment> {
    await tick(160);
    const id = `att_${bytesToHex(crypto.getRandomValues(new Uint8Array(6)))}`;
    const url = URL.createObjectURL(blob);
    this.objectUrls.set(id, url);
    const digest = await digestBytes(blob).catch(() => '');
    void filename;
    return { id, size: blob.size, sha256: digest };
  }

  async resolveAttachmentUrl(attachmentId: string, _mime?: string | null): Promise<string> {
    const local = this.objectUrls.get(attachmentId);
    if (local) return local;
    if (attachmentId === 'att_a1b2c3d4e5f6') return this.dataset.trailPhoto.dataUrl;
    if (attachmentId === 'att_f0e1d2c3b4a5') return this.dataset.cabinMap.dataUrl;
    if (attachmentId === 'att_voice0000001') return this.dataset.voiceNote.dataUrl;
    throw new Error('Demo attachment not found');
  }
}

function structuredCloneSafe<T>(value: T): T {
  if (typeof structuredClone === 'function') return structuredClone(value);
  return JSON.parse(JSON.stringify(value)) as T;
}

/** Deterministic identity bytes for unknown demo nodes (demo keys only). */
function buildDemoIdentityBytes(nodeId: string): string {
  const bytes = new Uint8Array(72);
  const rand = mulberry32(hash32(nodeId));
  for (let i = 0; i < 72; i += 1) bytes[i] = Math.floor(rand() * 256);
  return bytesToBase64(bytes);
}

async function digestBytes(blob: Blob): Promise<string> {
  if (!globalThis.crypto?.subtle) return '';
  const buffer = await globalThis.crypto.subtle.digest('SHA-256', await blob.arrayBuffer());
  return bytesToHex(new Uint8Array(buffer));
}
