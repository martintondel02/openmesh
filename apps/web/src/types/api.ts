/**
 * Wire types for the OpenMesh Bridge API.
 *
 * Normative source: `docs/architecture/WEBSOCKET-API.md` (v0.5.0).
 * Field names here are copied verbatim from the spec — do not rename them.
 * Per spec §7, clients MUST ignore unknown event types and unknown fields.
 */

import type { BackboneConfig, BackbonePeerEvent, BackboneStatusEvent, BackboneTokenEvent } from './backbone';
import type { LegalLimits, RegulatoryOverride } from './regulatory';

/** Every WS message in either direction (spec §2). */
export interface Envelope<T = unknown> {
  v: number;
  type: string;
  /** Present on request/response commands; echoed by the server. */
  id?: string;
  data?: T;
}

export type RoleClass = 'client' | 'repeater' | 'gateway' | 'room_server' | 'tracker';

/** Capability strings used by `node` events (spec §3.3). */
export type Cap =
  | 'gps'
  | 'wifi'
  | 'ethernet'
  | 'store'
  | 'relay'
  | 'ble'
  | 'usb'
  | 'screen'
  | 'audio_out'
  | 'solar';

/** Position object shared by node/message/position events. */
export interface Position {
  lat: number;
  lon: number;
  alt_m?: number | null;
  accuracy_m?: number | null;
  speed_cm_s?: number | null;
  heading_deg?: number | null;
  timestamp_ms: number;
}

export interface HelloNode {
  node_id: string;
  name: string;
  class: RoleClass;
  /** Bitmask (u16) in `hello`; `node` events use a string array instead. */
  caps: number | string[];
}

export interface HelloRadio {
  id: string;
  kind: string;
  label: string;
  connected: boolean;
  profile: string;
  region: string;
}

export interface HelloData {
  bridge_version: string;
  protocol: string;
  node: HelloNode;
  radios: HelloRadio[];
  features: string[];
  store: { messages: number; contacts: number; channels: number };
}

export interface StatusData {
  uptime_s: number;
  radio: {
    connected: boolean;
    tx_queue: number;
    airtime_used_pct: number;
    rssi: number | null;
    snr: number | null;
    frequency_mhz: number | null;
    tx_power_dbm: number | null;
  };
  mesh: {
    tx_frames: number;
    rx_frames: number;
    forwarded: number;
    dropped: number;
    known_nodes: number;
    stored_envelopes: number;
    duty_cycle_pct: number;
  };
  storage: { used_bytes: number; db_path: string };
  /** v0.5 regulatory override status (optional; top-level per the design contract). */
  regulatory_override_active?: boolean;
  regulatory_override_warning?: string | null;
}

export interface NodeData {
  node_id: string;
  name: string;
  class: RoleClass;
  /** Spec §3.3: array of strings for readability. */
  caps: string[];
  hops: number;
  last_seen_ms: number;
  rssi: number | null;
  snr: number | null;
  position: Position | null;
  battery_pct: number | null;
  is_contact: boolean;
  public_key: string | null;
}

export type MessageKind = 'text' | 'position' | 'attachment' | 'telemetry';

export interface Attachment {
  id: string;
  mime: string;
  size: number;
  sha256: string;
  duration_ms?: number | null;
  name?: string | null;
  /** 0..1 while transferring; 1.0 when complete. */
  progress: number;
  width?: number | null;
  height?: number | null;
}

export interface MessageData {
  msg_id: number;
  peer: string;
  from: string;
  to: string;
  kind: MessageKind;
  text: string | null;
  sent_at_ms: number;
  received_at_ms?: number | null;
  outgoing: boolean;
  delivered: boolean;
  read: boolean;
  reply_to: number | null;
  thread: string | null;
  attachment: Attachment | null;
  position: Position | null;
}

/** `message_update`: same shape as `message`, only changed fields + msg_id (§3.5). */
export type MessageUpdateData = Partial<MessageData> & { msg_id: number };

export interface GroupMessageData {
  msg_id: number;
  channel_id: string;
  channel_name: string;
  from: string;
  from_name: string;
  sender_key: string;
  kind: MessageKind;
  text: string | null;
  sent_at_ms: number;
  outgoing: boolean;
  signature_valid: boolean;
  attachment: Attachment | null;
  position: Position | null;
}

export interface AckData {
  msg_id: number;
  peer: string;
  kind: 'delivered' | 'read' | 'resource_window' | 'resource_complete';
  at_ms: number;
}

export interface PositionEventData {
  node_id: string;
  channel_id: string | null;
  position: Position;
}

export interface TelemetryData {
  node_id: string;
  battery_mv?: number | null;
  battery_pct?: number | null;
  uptime_s?: number | null;
  airtime_used_ms?: number | null;
  temperatur_c?: number | null;
  solar_mv?: number | null;
  timestamp_ms: number;
}

export interface StoreOfferData {
  relay: string;
  count: number;
}

export interface ResourceProgressData {
  peer: string;
  msg_id: number;
  res_id: number;
  received: number;
  total: number;
  direction: 'in' | 'out';
}

export interface ConfigChannel {
  channel_id: string;
  name: string;
  psk_set: boolean;
}

export interface ConfigData {
  node: { name: string; class: string; announce_interval_s: number };
  radio: {
    profile: string;
    region: string;
    tx_power_dbm: number;
    frequency_mhz?: number | null;
    interface?: string | null;
    /** v0.5 legal maxima mirrored on the radio block. */
    max_power_dbm?: number | null;
    duty_cycle_pct?: number | null;
    max_bw_khz?: number | null;
  };
  features: {
    auto_announce: boolean;
    store_forward: boolean;
    position_sharing: boolean;
    /** v0.3 UI mode synced from this client (local layout preference wins). */
    ui_mode?: 'beginner' | 'advanced';
  };
  channels: ConfigChannel[];
  /** Optional backbone section pushed with `config` after set_backbone_config (BACKBONE.md §7.2). */
  backbone?: BackboneConfig;
  /** v0.5 legal maxima for the selected region (optional for older bridges). */
  legal?: LegalLimits | null;
  /** v0.5 emergency override state; absent on older bridges. */
  regulatory_override?: RegulatoryOverride | null;
  override_active?: boolean;
  override_warning?: string | null;
}

export interface ContactData {
  node_id: string;
  name: string;
  identity: string;
  last_seen_ms: number;
}

export interface ContactsEventData {
  contacts: ContactData[];
}

export interface ChannelsEventData {
  channels: ConfigChannel[];
}

export interface ChannelCreatedData {
  channel_id: string;
  name: string;
  /** Returned exactly once, at creation time. Never returned afterwards. */
  psk?: string;
}

export interface SendResultData {
  msg_id: number;
  est_airtime_ms: number;
  queued: boolean;
}

export interface SyncDoneData {
  messages: number;
  nodes: number;
}

export interface NodesEventData {
  nodes: NodeData[];
}

export interface MessagesEventData {
  messages: MessageData[];
}

export interface ErrorData {
  code:
    | 'bad_request'
    | 'unauthorized'
    | 'unknown_peer'
    | 'not_found'
    | 'too_large'
    | 'radio_busy'
    | 'internal';
  message: string;
}

export interface ContactUrlData {
  /** e.g. `openmesh://contact?node_id=…&identity=…` */
  url: string;
}

export interface ChannelUrlData {
  /** e.g. `openmesh://channel?channel_id=…&name=…&psk=…` */
  url: string;
}

/** All server→client event payloads, keyed by envelope `type`. */
export interface ServerEventMap {
  hello: HelloData;
  status: StatusData;
  node: NodeData;
  message: MessageData;
  message_update: MessageUpdateData;
  group_message: GroupMessageData;
  ack: AckData;
  position: PositionEventData;
  telemetry: TelemetryData;
  store_offer: StoreOfferData;
  resource_progress: ResourceProgressData;
  backbone_status: BackboneStatusEvent;
  backbone_peer: BackbonePeerEvent;
  backbone_token: BackboneTokenEvent;
  config: ConfigData;
  sync_done: SyncDoneData;
  nodes: NodesEventData;
  messages: MessagesEventData;
  contacts: ContactsEventData;
  channels: ChannelsEventData;
  channel_created: ChannelCreatedData;
  send_result: SendResultData;
  contact_url: ContactUrlData;
  channel_url: ChannelUrlData;
  error: ErrorData;
}

export type ServerEventType = keyof ServerEventMap;

/* ------------------------------------------------------------------ */
/* Client-side domain types (not wire types)                          */
/* ------------------------------------------------------------------ */

export type ConnectionState =
  | 'idle'
  | 'connecting'
  | 'connected'
  | 'demo'
  | 'reconnecting'
  | 'offline';

export type TransportMode = 'live' | 'demo';

/**
 * Delivery state derived from the bridge flags. `sent` means the bridge echoed
 * the message but no delivery ack has arrived yet.
 */
export type DeliveryState = 'pending' | 'sent' | 'delivered' | 'read' | 'failed';

/** A direct message plus local-only UI bookkeeping. */
export interface ChatMessage extends MessageData {
  delivery: DeliveryState;
  /** Locally generated id used to reconcile an optimistic message with its echo. */
  tempId?: string;
  /** Blob/data URL for media that is local-only (optimistic send, demo dataset). */
  localUrl?: string;
  /** Demo/local waveform amplitudes 0..1, ~32 bars. */
  waveform?: number[];
  /** Local error explanation for failed sends. */
  error?: string;
  /** True for demo/seed data, so the UI can label it as illustrative. */
  seeded?: boolean;
}

export interface ChatGroupMessage extends GroupMessageData {
  tempId?: string;
  localUrl?: string;
  waveform?: number[];
  error?: string;
  seeded?: boolean;
}

export interface Channel {
  channel_id: string;
  name: string;
  psk_set: boolean;
  seeded?: boolean;
}

export interface Contact extends ContactData {
  seeded?: boolean;
}

export type Screen = 'chats' | 'map' | 'nodes' | 'contacts' | 'backbone' | 'agents' | 'help' | 'settings';

/** A conversation row in the chats sidebar. */
export interface Conversation {
  id: string;
  kind: 'direct' | 'group';
  name: string;
  /** Short id / node id / channel id. */
  address: string;
  subtitle: string;
  lastText: string;
  lastAt: number;
  unread: number;
  online: boolean;
  role?: RoleClass;
  hops?: number;
  delivery?: DeliveryState;
  outgoing?: boolean;
  pinned?: boolean;
}
