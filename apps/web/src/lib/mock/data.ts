/**
 * Seeded demo dataset — a small but realistic Oslo-area mesh.
 *
 * Demo mode exists so the client can be reviewed (and screenshotted) without a
 * bridge running. The data is intentionally plausible: LoRa RSSI/SNR values,
 * hop counts, duty-cycle-friendly timestamps, one failed send, unread traffic,
 * signed group messages, a photo and a playable voice note.
 *
 * Everything is deterministic for a given `nowMs`, which lets tests assert the
 * dataset invariants. Local-only enrichment (`localUrl`, `waveform`,
 * `demoDelivery`) never leaves the client and is not part of the bridge API.
 */

import type {
  BackboneConfig,
  BackbonePeer,
  BackboneSetup,
  BackboneStatus,
} from '../../types/backbone';
import type { ActivityEntry } from '../../types/agents';
import type { BackboneLink, GatewayConnector, McpHttpStatus } from '../../types/ops';
import type { SetupRecommendation, SetupState } from '../../types/setup';
import { recommendedFor } from '../setup';
import type {
  ConfigData,
  ConfigChannel,
  ContactData,
  DeliveryState,
  GroupMessageData,
  HelloData,
  MessageData,
  NodeData,
  Position,
  StatusData,
} from '../../types/api';
import { demoVoiceDataUrl, speechWaveform } from '../audio';
import { generateChannelPsk } from '../utils';
import { bytesToBase64, hexToBytes } from '../utils';
import { mulberry32 } from '../audio';
import { buildChannelLink, buildContactLink } from '../links';
import { buildBackboneConnectionString, buildFallbackSetup } from '../backbone';
import { defaultRegulatoryOverride, demoRegulatoryConfigFields } from '../regulatory';

export interface DemoMessage extends MessageData {
  /** Local-only display URL (SVG data URL for seeded images, WAV for voice). */
  localUrl?: string;
  /** Local-only waveform bars for voice notes (0..1). */
  waveform?: number[];
  /** Local-only delivery override, so the demo shows a failed retry case. */
  demoDelivery?: DeliveryState;
}

export interface DemoGroupMessage extends GroupMessageData {
  localUrl?: string;
  waveform?: number[];
}

export interface DemoMedia {
  dataUrl: string;
  mime: string;
  width: number;
  height: number;
  name: string;
}

export interface DemoBackbone {
  config: BackboneConfig;
  status: BackboneStatus;
  peers: BackbonePeer[];
  setup: BackboneSetup;
  /** Demo-only bearer token; the real API never returns it outside setup/rotate. */
  token: string;
}

export interface DemoDataset {
  now: number;
  ownNodeId: string;
  ownName: string;
  ownPosition: Position;
  hello: HelloData;
  status: StatusData;
  config: ConfigData;
  nodes: NodeData[];
  contacts: ContactData[];
  channels: ConfigChannel[];
  messages: DemoMessage[];
  groupMessages: DemoGroupMessage[];
  positionHistories: Record<string, Position[]>;
  identities: Record<string, string>;
  channelPsks: Record<string, string>;
  trailPhoto: DemoMedia;
  cabinMap: DemoMedia;
  voiceNote: { dataUrl: string; waveform: number[]; durationMs: number };
  backbone: DemoBackbone;
  setup: DemoSetupSeed;
  agents: DemoAgentSeed;
  /** v0.7 remote operations seeds. */
  gateways: GatewayConnector[];
  backboneLinks: BackboneLink[];
  mcp: McpHttpStatus;
}

export interface DemoSetupSeed {
  state: SetupState;
  recommendation: SetupRecommendation;
}

export interface DemoAgentSeed {
  configured: boolean;
  name: string;
  activities: ActivityEntry[];
}

/* ------------------------------------------------------------------ */
/* Stable ids                                                         */
/* ------------------------------------------------------------------ */

export const DEMO_OWN_ID = 'a1b2c3d4e5f60718';
export const DEMO_ANNE_ID = '6b1f9a2c4d7e8031';
export const DEMO_LARS_ID = 'd3e4f5a6b7c8d901';
export const DEMO_SOLAR_RIDGE_ID = '7f3a9c01d2e4b650';
export const DEMO_NORDMARKA_RELAY_ID = '3c5d7e9f1a2b4c60';
export const DEMO_HYTTA_GATEWAY_ID = 'e8f7a6b5c4d3e2f1';
export const DEMO_T1000E_ID = '91a2b3c4d5e6f708';
export const DEMO_TRAIL_TRACKER_ID = '5e6f7a8b9c0d1e2f';
export const DEMO_KOLSAS_TRACKER_ID = '2f3e4d5c6b7a8091';

export const DEMO_BACKBONE_PEER_LOFOTEN = '4a5b6c7d8e9f0a1b';
export const DEMO_BACKBONE_PEER_TROMSO = '8f7e6d5c4b3a2019';
export const DEMO_CHANNEL_FAMILY = 'f0e1d2c3b4a59687';
export const DEMO_CHANNEL_TUR = '1a2b3c4d5e6f7081';

const MINUTE = 60_000;
const HOUR = 3_600_000;
const DAY = 86_400_000;

/** Deterministic pseudo-random bytes encoded as base64 (demo keys only). */
function seededBase64(seed: number, length: number): string {
  const rand = mulberry32(seed);
  const bytes = new Uint8Array(length);
  for (let i = 0; i < length; i += 1) bytes[i] = Math.floor(rand() * 256);
  return bytesToBase64(bytes);
}

/** 72-byte public identity: node_id(8) || sign_pk(32) || dh_pk(32) (PROTOCOL §3). */
export function demoIdentity(nodeId: string, seed = 1): string {
  const bytes = new Uint8Array(72);
  bytes.set(hexToBytes(nodeId).slice(0, 8), 0);
  const rand = mulberry32(seed * 7919 + 13);
  for (let i = 8; i < 72; i += 1) bytes[i] = Math.floor(rand() * 256);
  return bytesToBase64(bytes);
}

/* ------------------------------------------------------------------ */
/* Media                                                              */
/* ------------------------------------------------------------------ */

/** Stylised dusk photo of the Kolsås ridge — generated as an SVG data URL. */
export function ridgePhotoSvg(): string {
  const stars = Array.from({ length: 46 }, (_, i) => {
    const rand = mulberry32(i * 31 + 7);
    const x = Math.round(rand() * 1200);
    const y = Math.round(rand() * 320);
    const r = Math.round((rand() * 1.6 + 0.4) * 10) / 10;
    return `<circle cx="${x}" cy="${y}" r="${r}" fill="#dbeafe" opacity="${(0.2 + rand() * 0.6).toFixed(2)}"/>`;
  }).join('');
  const pines = [
    [90, 800, 60],
    [170, 800, 44],
    [1150, 800, 70],
    [1040, 800, 42],
    [960, 800, 30],
  ]
    .map(
      ([x, y, h]) =>
        `<path d="M${x} ${y} L${(x as number) - (h as number) * 0.42} ${y} L${x} ${(y as number) - (h as number)} L${(x as number) + (h as number) * 0.42} ${y} Z" fill="#050d16"/>`,
    )
    .join('');
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1200 800" width="1200" height="800">
  <defs>
    <linearGradient id="sky" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#081426"/>
      <stop offset="0.42" stop-color="#1b3a5c"/>
      <stop offset="0.66" stop-color="#8a5a4a"/>
      <stop offset="0.82" stop-color="#e09a5e"/>
      <stop offset="1" stop-color="#f6c98b"/>
    </linearGradient>
    <linearGradient id="water" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#c98a58"/>
      <stop offset="0.12" stop-color="#3d5a6e"/>
      <stop offset="1" stop-color="#0a1724"/>
    </linearGradient>
    <radialGradient id="sunGlow" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0" stop-color="#fff3cf" stop-opacity="0.95"/>
      <stop offset="0.5" stop-color="#ffbe73" stop-opacity="0.45"/>
      <stop offset="1" stop-color="#ff9d4d" stop-opacity="0"/>
    </radialGradient>
    <linearGradient id="mist" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#f8e0bd" stop-opacity="0.28"/>
      <stop offset="1" stop-color="#f8e0bd" stop-opacity="0"/>
    </linearGradient>
  </defs>
  <rect width="1200" height="800" fill="url(#sky)"/>
  ${stars}
  <circle cx="842" cy="492" r="150" fill="url(#sunGlow)"/>
  <circle cx="842" cy="492" r="38" fill="#ffe6b5"/>
  <path d="M0 520 L150 470 L260 505 L380 430 L500 500 L640 445 L760 505 L880 470 L1010 515 L1120 480 L1200 505 L1200 620 L0 620 Z" fill="#3c4b58" opacity="0.85"/>
  <path d="M0 566 L120 512 L250 552 L390 495 L530 556 L680 505 L820 562 L950 520 L1080 568 L1200 540 L1200 640 L0 640 Z" fill="#25323f"/>
  <path d="M0 614 L170 568 L310 610 L470 566 L620 618 L780 574 L940 620 L1090 586 L1200 612 L1200 700 L0 700 Z" fill="#12202c"/>
  <rect y="632" width="1200" height="168" fill="url(#water)"/>
  <rect y="632" width="1200" height="26" fill="#f5c58e" opacity="0.35"/>
  <g stroke="#f0b877" stroke-width="2" opacity="0.35" stroke-linecap="round">
    <line x1="700" y1="662" x2="990" y2="662"/>
    <line x1="742" y1="680" x2="950" y2="680"/>
    <line x1="770" y1="700" x2="920" y2="700"/>
  </g>
  <rect y="560" width="1200" height="110" fill="url(#mist)"/>
  ${pines}
  <path d="M0 800 L200 700 L330 742 L470 690 L640 760 L820 706 L980 768 L1120 724 L1200 760 L1200 800 Z" fill="#040b12"/>
  <path d="M420 800 C 520 748, 610 736, 700 700" stroke="#33465a" stroke-width="16" fill="none" opacity="0.5" stroke-linecap="round"/>
</svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

/** Hand-drawn field map of a cabin trail, generated as an SVG data URL. */
export function cabinMapSvg(): string {
  const contours = Array.from({ length: 9 }, (_, i) => {
    const y = 240 + i * 74;
    return `<path d="M40 ${y} C 220 ${y - 46}, 380 ${y + 38}, 560 ${y - 12} S 820 ${y + 40}, 870 ${y - 6}" fill="none" stroke="#b08a62" stroke-width="1.4" opacity="0.55"/>`;
  }).join('');
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 900 1100" width="900" height="1100">
  <rect width="900" height="1100" fill="#efe4cf"/>
  <rect x="0" y="0" width="900" height="1100" fill="none" stroke="#d8c7ac" stroke-width="16"/>
  <g fill="none" stroke="#d9c9ae" stroke-width="1" opacity="0.7">
    ${Array.from({ length: 21 }, (_, i) => `<line x1="0" y1="${i * 54}" x2="900" y2="${i * 54}"/>`).join('')}
    ${Array.from({ length: 17 }, (_, i) => `<line x1="${i * 54}" y1="0" x2="${i * 54}" y2="1100"/>`).join('')}
  </g>
  ${contours}
  <path d="M0 690 C 120 640, 220 700, 300 650 C 390 596, 470 640, 540 600 C 640 546, 720 610, 900 560 L900 860 C 760 900, 650 840, 520 890 C 400 940, 240 880, 0 940 Z" fill="#a8c6cf" opacity="0.85"/>
  <path d="M0 690 C 120 640, 220 700, 300 650 C 390 596, 470 640, 540 600 C 640 546, 720 610, 900 560" fill="none" stroke="#7fa3b0" stroke-width="3"/>
  <path d="M120 960 C 220 900, 250 800, 360 740 C 470 680, 520 600, 640 520 C 720 466, 760 400, 800 320" fill="none" stroke="#c1503c" stroke-width="6" stroke-dasharray="20 14" stroke-linecap="round"/>
  <g transform="translate(612 470)">
    <path d="M-40 44 L-40 0 L0 -34 L40 0 L40 44 Z" fill="#7a4b32" stroke="#3f2a1d" stroke-width="3"/>
    <path d="M-48 4 L0 -38 L48 4" fill="none" stroke="#3f2a1d" stroke-width="7" stroke-linecap="round"/>
    <rect x="-10" y="16" width="20" height="28" fill="#3f2a1d"/>
  </g>
  <g transform="translate(160 300)">
    <path d="M0 0 L22 46 L-22 46 Z" fill="#3f5a3a"/>
    <path d="M0 -26 L22 20 L-22 20 Z" fill="#46653f"/>
    <rect x="-4" y="44" width="8" height="14" fill="#5a3b26"/>
  </g>
  <g transform="translate(742 820)">
    <path d="M0 0 L22 46 L-22 46 Z" fill="#3f5a3a"/>
    <path d="M0 -26 L22 20 L-22 20 Z" fill="#46653f"/>
    <rect x="-4" y="44" width="8" height="14" fill="#5a3b26"/>
  </g>
  <text x="70" y="96" font-family="ui-monospace, Menlo, monospace" font-size="34" fill="#4a3a2a" font-weight="700" letter-spacing="6">NORDMARKA</text>
  <text x="70" y="136" font-family="ui-monospace, Menlo, monospace" font-size="19" fill="#7a6a55" letter-spacing="3">TRAIL SKETCH · 1:25 000</text>
  <text x="648" y="560" font-family="ui-monospace, Menlo, monospace" font-size="24" fill="#4a3a2a">HYTTA</text>
  <text x="90" y="1000" font-family="ui-monospace, Menlo, monospace" font-size="20" fill="#6b5b46">start · Sognsvann car park</text>
  <g transform="translate(800 120)">
    <circle r="46" fill="none" stroke="#8a7a63" stroke-width="3"/>
    <path d="M0 -36 L14 6 L0 -2 L-14 6 Z" fill="#c1503c"/>
    <text x="-7" y="-52" font-family="ui-monospace, Menlo, monospace" font-size="22" fill="#6b5b46">N</text>
  </g>
  <g transform="translate(80 1032)">
    <line x1="0" y1="0" x2="150" y2="0" stroke="#6b5b46" stroke-width="4"/>
    <line x1="0" y1="-8" x2="0" y2="8" stroke="#6b5b46" stroke-width="4"/>
    <line x1="150" y1="-8" x2="150" y2="8" stroke="#6b5b46" stroke-width="4"/>
    <text x="0" y="-16" font-family="ui-monospace, Menlo, monospace" font-size="18" fill="#6b5b46">500 m</text>
  </g>
  <text x="500" y="1062" font-family="ui-monospace, Menlo, monospace" font-size="17" fill="#9a8a73">trail muddy · ridge passable</text>
</svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

/* ------------------------------------------------------------------ */
/* Dataset                                                            */
/* ------------------------------------------------------------------ */

function position(lat: number, lon: number, timestampMs: number, alt = 40, accuracy = 8): Position {
  return {
    lat: Math.round(lat * 1e6) / 1e6,
    lon: Math.round(lon * 1e6) / 1e6,
    alt_m: alt,
    accuracy_m: accuracy,
    timestamp_ms: timestampMs,
  };
}

function demoNode(node: NodeData): NodeData {
  return node;
}

function directMessage(partial: Partial<DemoMessage> & Pick<MessageData, 'msg_id' | 'peer' | 'from' | 'to' | 'sent_at_ms' | 'outgoing'>): DemoMessage {
  return {
    kind: 'text',
    text: null,
    received_at_ms: partial.sent_at_ms,
    delivered: true,
    read: true,
    reply_to: null,
    thread: null,
    attachment: null,
    position: null,
    ...partial,
  };
}

/** Build a plausible position trail around a base point. */
function makeTrail(baseLat: number, baseLon: number, endMs: number, steps: number, seed: number): Position[] {
  const rand = mulberry32(seed);
  const points: Position[] = [];
  let lat = baseLat - 0.012;
  let lon = baseLon - 0.018;
  for (let i = 0; i < steps; i += 1) {
    lat += 0.0009 + rand() * 0.0014;
    lon += (rand() - 0.35) * 0.002;
    points.push(position(lat, lon, endMs - (steps - 1 - i) * 2 * MINUTE, 42 + i * 3, 6 + Math.round(rand() * 8)));
  }
  return points;
}

export function createDemoDataset(nowMs: number = Date.now()): DemoDataset {
  const ownId = DEMO_OWN_ID;
  const identities: Record<string, string> = {
    [ownId]: demoIdentity(ownId, 1),
    [DEMO_ANNE_ID]: demoIdentity(DEMO_ANNE_ID, 2),
    [DEMO_LARS_ID]: demoIdentity(DEMO_LARS_ID, 3),
    [DEMO_SOLAR_RIDGE_ID]: demoIdentity(DEMO_SOLAR_RIDGE_ID, 4),
    [DEMO_NORDMARKA_RELAY_ID]: demoIdentity(DEMO_NORDMARKA_RELAY_ID, 5),
    [DEMO_HYTTA_GATEWAY_ID]: demoIdentity(DEMO_HYTTA_GATEWAY_ID, 6),
    [DEMO_T1000E_ID]: demoIdentity(DEMO_T1000E_ID, 7),
    [DEMO_TRAIL_TRACKER_ID]: demoIdentity(DEMO_TRAIL_TRACKER_ID, 8),
    [DEMO_KOLSAS_TRACKER_ID]: demoIdentity(DEMO_KOLSAS_TRACKER_ID, 9),
  };
  const channelPsks: Record<string, string> = {
    [DEMO_CHANNEL_FAMILY]: seededBase64(101, 32),
    [DEMO_CHANNEL_TUR]: seededBase64(202, 32),
  };

  const ownPosition = position(59.9215, 10.758, nowMs - 4 * MINUTE, 61, 7);
  const nodes: NodeData[] = [
    demoNode({
      node_id: DEMO_ANNE_ID,
      name: 'Anne',
      class: 'client',
      caps: ['gps', 'ble', 'wifi'],
      hops: 1,
      last_seen_ms: nowMs - 3 * MINUTE,
      rssi: -89,
      snr: 7.5,
      position: position(59.9137, 10.7522, nowMs - 3 * MINUTE, 12, 6),
      battery_pct: 42,
      is_contact: true,
      public_key: identities[DEMO_ANNE_ID]!,
    }),
    demoNode({
      node_id: DEMO_LARS_ID,
      name: 'Lars',
      class: 'client',
      caps: ['gps', 'ble', 'usb'],
      hops: 3,
      last_seen_ms: nowMs - 14 * MINUTE,
      rssi: -108,
      snr: 2.25,
      position: position(59.936, 10.785, nowMs - 14 * MINUTE, 34, 12),
      battery_pct: 91,
      is_contact: true,
      public_key: identities[DEMO_LARS_ID]!,
    }),
    demoNode({
      node_id: DEMO_SOLAR_RIDGE_ID,
      name: 'solar-ridge',
      class: 'repeater',
      caps: ['relay', 'store', 'gps', 'solar'],
      hops: 2,
      last_seen_ms: nowMs - 40_000,
      rssi: -101,
      snr: 4.25,
      position: position(59.9286, 10.5147, nowMs - 40_000, 328, 15),
      battery_pct: 87,
      is_contact: false,
      public_key: identities[DEMO_SOLAR_RIDGE_ID]!,
    }),
    demoNode({
      node_id: DEMO_NORDMARKA_RELAY_ID,
      name: 'nordmarka-relay',
      class: 'repeater',
      caps: ['relay', 'store', 'solar', 'gps'],
      hops: 2,
      last_seen_ms: nowMs - 6 * MINUTE,
      rssi: -112,
      snr: 1.75,
      position: position(60.0205, 10.812, nowMs - 6 * MINUTE, 486, 25),
      battery_pct: 100,
      is_contact: false,
      public_key: identities[DEMO_NORDMARKA_RELAY_ID]!,
    }),
    demoNode({
      node_id: DEMO_HYTTA_GATEWAY_ID,
      name: 'hytta-gateway',
      class: 'gateway',
      caps: ['ethernet', 'wifi', 'relay', 'store', 'gps'],
      hops: 1,
      last_seen_ms: nowMs - 2 * MINUTE,
      rssi: -94,
      snr: 9,
      position: position(60.052, 10.654, nowMs - 2 * MINUTE, 512, 10),
      battery_pct: null,
      is_contact: false,
      public_key: identities[DEMO_HYTTA_GATEWAY_ID]!,
    }),
    demoNode({
      node_id: DEMO_T1000E_ID,
      name: 't1000e-hike',
      class: 'tracker',
      caps: ['gps', 'ble'],
      hops: 2,
      last_seen_ms: nowMs - 9 * MINUTE,
      rssi: -104,
      snr: 3.5,
      position: position(59.976, 10.733, nowMs - 9 * MINUTE, 194, 9),
      battery_pct: 64,
      is_contact: false,
      public_key: identities[DEMO_T1000E_ID]!,
    }),
    demoNode({
      node_id: DEMO_TRAIL_TRACKER_ID,
      name: 'trail-tracker',
      class: 'tracker',
      caps: ['gps', 'solar', 'ble'],
      hops: 3,
      last_seen_ms: nowMs - 21 * MINUTE,
      rssi: -117,
      snr: 0.5,
      position: position(59.958, 10.669, nowMs - 21 * MINUTE, 287, 18),
      battery_pct: 78,
      is_contact: false,
      public_key: identities[DEMO_TRAIL_TRACKER_ID]!,
    }),
    demoNode({
      node_id: DEMO_KOLSAS_TRACKER_ID,
      name: 'kolsas-tracker',
      class: 'tracker',
      caps: ['gps', 'ble'],
      hops: 1,
      last_seen_ms: nowMs - 32 * MINUTE,
      rssi: -97,
      snr: 5.25,
      position: position(59.924, 10.53, nowMs - 32 * MINUTE, 342, 11),
      battery_pct: 53,
      is_contact: false,
      public_key: identities[DEMO_KOLSAS_TRACKER_ID]!,
    }),
  ];

  const contacts: ContactData[] = [
    { node_id: DEMO_ANNE_ID, name: 'Anne', identity: identities[DEMO_ANNE_ID]!, last_seen_ms: nowMs - 3 * MINUTE },
    { node_id: DEMO_LARS_ID, name: 'Lars', identity: identities[DEMO_LARS_ID]!, last_seen_ms: nowMs - 14 * MINUTE },
    {
      node_id: DEMO_HYTTA_GATEWAY_ID,
      name: 'Hytta gateway',
      identity: identities[DEMO_HYTTA_GATEWAY_ID]!,
      last_seen_ms: nowMs - 2 * MINUTE,
    },
  ];

  const channels: ConfigChannel[] = [
    { channel_id: DEMO_CHANNEL_FAMILY, name: 'familien', psk_set: true },
    { channel_id: DEMO_CHANNEL_TUR, name: 'nordmarka-tur', psk_set: true },
  ];

  const trailPhoto: DemoMedia = {
    dataUrl: ridgePhotoSvg(),
    mime: 'image/svg+xml',
    width: 1200,
    height: 800,
    name: 'ridge-sunset.svg',
  };
  const cabinMap: DemoMedia = {
    dataUrl: cabinMapSvg(),
    mime: 'image/svg+xml',
    width: 900,
    height: 1100,
    name: 'hytta-trail-sketch.svg',
  };
  const voiceDuration = 7_400;
  const voiceNote = {
    dataUrl: demoVoiceDataUrl(voiceDuration, 7),
    waveform: speechWaveform(voiceDuration, 32, 7),
    durationMs: voiceDuration,
  };

  const messages: DemoMessage[] = [
    // Yesterday — day separator + normal history.
    directMessage({
      msg_id: 1001,
      peer: DEMO_ANNE_ID,
      from: DEMO_ANNE_ID,
      to: ownId,
      text: 'Ferry leaves 16:40 on Friday — don’t be late 😄',
      sent_at_ms: nowMs - DAY - 5 * HOUR,
      read: true,
      delivered: true,
      outgoing: false,
    }),
    directMessage({
      msg_id: 1002,
      peer: DEMO_ANNE_ID,
      from: ownId,
      to: DEMO_ANNE_ID,
      text: 'I’ll be at the dock by 16:20. Bringing the good thermos.',
      sent_at_ms: nowMs - DAY - 5 * HOUR + 4 * MINUTE,
      delivered: true,
      read: true,
      outgoing: true,
    }),
    directMessage({
      msg_id: 1003,
      peer: DEMO_ANNE_ID,
      from: DEMO_ANNE_ID,
      to: ownId,
      text: 'Good. The ridge road is still closed, so we go via Sognsvann.',
      sent_at_ms: nowMs - DAY - 4 * HOUR - 55 * MINUTE,
      read: true,
      delivered: true,
      outgoing: false,
    }),
    // Today — unread, photo, voice, failed message.
    directMessage({
      msg_id: 1004,
      peer: DEMO_ANNE_ID,
      from: DEMO_ANNE_ID,
      to: ownId,
      text: 'Do you have the cabin key? I’m heading up after work.',
      sent_at_ms: nowMs - 62 * MINUTE,
      read: false,
      delivered: true,
      outgoing: false,
    }),
    directMessage({
      msg_id: 1005,
      peer: DEMO_ANNE_ID,
      from: ownId,
      to: DEMO_ANNE_ID,
      text: 'It’s in the blue locker by the door 🔑',
      sent_at_ms: nowMs - 58 * MINUTE,
      read: true,
      delivered: true,
      outgoing: true,
      reply_to: 1004,
    }),
    directMessage({
      msg_id: 1006,
      peer: DEMO_ANNE_ID,
      from: DEMO_ANNE_ID,
      to: ownId,
      kind: 'attachment',
      text: 'Found it! Trail was muddy but the ridge was clear.',
      sent_at_ms: nowMs - 51 * MINUTE,
      read: false,
      delivered: true,
      outgoing: false,
      attachment: {
        id: 'att_a1b2c3d4e5f6',
        mime: trailPhoto.mime,
        size: 42_180,
        sha256: seededBase64(404, 32),
        duration_ms: null,
        name: trailPhoto.name,
        progress: 1,
        width: trailPhoto.width,
        height: trailPhoto.height,
      },
      localUrl: trailPhoto.dataUrl,
    }),
    directMessage({
      msg_id: 1007,
      peer: DEMO_ANNE_ID,
      from: ownId,
      to: DEMO_ANNE_ID,
      kind: 'attachment',
      text: null,
      sent_at_ms: nowMs - 44 * MINUTE,
      read: true,
      delivered: true,
      outgoing: true,
      attachment: {
        id: 'att_voice0000001',
        mime: 'audio/ogg; codecs=opus',
        size: 6_475,
        sha256: seededBase64(405, 32),
        duration_ms: voiceNote.durationMs,
        name: 'voice-note.ogg',
        progress: 1,
      },
      localUrl: voiceNote.dataUrl,
      waveform: voiceNote.waveform,
    }),
    directMessage({
      msg_id: 1016,
      peer: DEMO_ANNE_ID,
      from: DEMO_ANNE_ID,
      to: ownId,
      kind: 'attachment',
      text: null,
      sent_at_ms: nowMs - 40 * MINUTE,
      read: true,
      delivered: true,
      outgoing: false,
      attachment: {
        id: 'att_voicec2tny01',
        mime: 'audio/c2-1300',
        size: 1_295,
        sha256: seededBase64(415, 32),
        duration_ms: 7_400,
        name: 'voice-tiny.c2',
        progress: 1,
      },
      localUrl: voiceNote.dataUrl,
      waveform: voiceNote.waveform,
    }),
    directMessage({
      msg_id: 1008,
      peer: DEMO_ANNE_ID,
      from: ownId,
      to: DEMO_ANNE_ID,
      text: 'Take the east route — the west gully is still icy. Sending the track after this.',
      sent_at_ms: nowMs - 12 * MINUTE,
      read: false,
      delivered: false,
      outgoing: true,
      demoDelivery: 'failed',
    }),
    // Lars thread — one unread + one failed send.
    directMessage({
      msg_id: 2001,
      peer: DEMO_LARS_ID,
      from: DEMO_LARS_ID,
      to: ownId,
      text: 'Are you joining the ski trip next month?',
      sent_at_ms: nowMs - DAY - 2 * HOUR,
      read: true,
      delivered: true,
      outgoing: false,
    }),
    directMessage({
      msg_id: 2002,
      peer: DEMO_LARS_ID,
      from: ownId,
      to: DEMO_LARS_ID,
      text: 'Maybe — depends where the snow line ends up.',
      sent_at_ms: nowMs - DAY - 90 * MINUTE,
      read: true,
      delivered: true,
      outgoing: true,
    }),
    directMessage({
      msg_id: 2003,
      peer: DEMO_LARS_ID,
      from: DEMO_LARS_ID,
      to: ownId,
      text: 'I’ll send the cabin link tonight. Cell coverage is gone up here, as usual.',
      sent_at_ms: nowMs - 95 * MINUTE,
      read: false,
      delivered: true,
      outgoing: false,
    }),
    directMessage({
      msg_id: 2004,
      peer: DEMO_LARS_ID,
      from: ownId,
      to: DEMO_LARS_ID,
      text: 'No rush — the mesh will hold it until you’re back in range.',
      sent_at_ms: nowMs - 88 * MINUTE,
      read: false,
      delivered: false,
      outgoing: true,
      demoDelivery: 'failed',
    }),
  ];

  const makeGroup = (
    partial: Partial<DemoGroupMessage> & Pick<GroupMessageData, 'msg_id' | 'channel_id' | 'from' | 'from_name' | 'sent_at_ms' | 'outgoing'>,
  ): DemoGroupMessage => ({
    channel_name: partial.channel_id === DEMO_CHANNEL_FAMILY ? 'familien' : 'nordmarka-tur',
    sender_key: identities[partial.from] ?? identities[ownId]!,
    kind: 'text',
    text: null,
    signature_valid: true,
    attachment: null,
    position: null,
    ...partial,
  });

  const groupMessages: DemoGroupMessage[] = [
    makeGroup({
      msg_id: 3001,
      channel_id: DEMO_CHANNEL_FAMILY,
      from: DEMO_ANNE_ID,
      from_name: 'Anne',
      text: 'Sunday dinner at ours? I’m making the lamb.',
      sent_at_ms: nowMs - 3 * HOUR,
      outgoing: false,
      signature_valid: true,
    }),
    makeGroup({
      msg_id: 3002,
      channel_id: DEMO_CHANNEL_FAMILY,
      from: ownId,
      from_name: 'martin-nord',
      text: 'I’m in. Bringing bread and the good mustard.',
      sent_at_ms: nowMs - 3 * HOUR + 6 * MINUTE,
      outgoing: true,
    }),
    makeGroup({
      msg_id: 3003,
      channel_id: DEMO_CHANNEL_FAMILY,
      from: DEMO_LARS_ID,
      from_name: 'Lars',
      text: 'Count me in 🙂 I’ll drive from Drammen.',
      sent_at_ms: nowMs - 3 * HOUR + 12 * MINUTE,
      outgoing: false,
      signature_valid: true,
    }),
    makeGroup({
      msg_id: 4001,
      channel_id: DEMO_CHANNEL_TUR,
      from: DEMO_LARS_ID,
      from_name: 'Lars',
      text: 'Repeater solar-ridge is at 87% — the new panel is doing its job.',
      sent_at_ms: nowMs - 6 * HOUR,
      outgoing: false,
      signature_valid: true,
    }),
    makeGroup({
      msg_id: 4002,
      channel_id: DEMO_CHANNEL_TUR,
      from: DEMO_ANNE_ID,
      from_name: 'Anne',
      kind: 'attachment',
      text: 'Ridge line is clear today. Sketch of the route up:',
      sent_at_ms: nowMs - 5 * HOUR,
      outgoing: false,
      signature_valid: true,
      attachment: {
        id: 'att_f0e1d2c3b4a5',
        mime: cabinMap.mime,
        size: 31_240,
        sha256: seededBase64(406, 32),
        duration_ms: null,
        name: cabinMap.name,
        progress: 1,
        width: cabinMap.width,
        height: cabinMap.height,
      },
      localUrl: cabinMap.dataUrl,
    }),
    makeGroup({
      msg_id: 4003,
      channel_id: DEMO_CHANNEL_TUR,
      from: ownId,
      from_name: 'martin-nord',
      kind: 'position',
      text: null,
      sent_at_ms: nowMs - 4 * HOUR,
      outgoing: true,
      position: ownPosition,
    }),
    makeGroup({
      msg_id: 4004,
      channel_id: DEMO_CHANNEL_TUR,
      from: ownId,
      from_name: 'martin-nord',
      text: 'Heading up from Sognsvann around 10. Meet at the junction?',
      sent_at_ms: nowMs - 4 * HOUR + 2 * MINUTE,
      outgoing: true,
    }),
  ];

  const status: StatusData = {
    uptime_s: 7_320,
    radio: {
      connected: true,
      state: 'virtual',
      last_rx_ms: null,
      reconnect_attempts: 0,
      tx_queue: 0,
      airtime_used_pct: 0.42,
      rssi: -87,
      snr: 6.5,
      frequency_mhz: 869.525,
      tx_power_dbm: 14,
    },
    mesh: {
      tx_frames: 1_204,
      rx_frames: 2_381,
      forwarded: 312,
      dropped: 6,
      known_nodes: 8,
      stored_envelopes: 2,
      duty_cycle_pct: 0.42,
    },
    storage: { used_bytes: 2_411_724, db_path: '~/.local/share/openmesh/openmesh.redb' },
    regulatory_override_active: false,
    regulatory_override_warning: null,
  };

  const hello: HelloData = {
    bridge_version: '0.7.1-demo',
    protocol: 'omp/1',
    node: { node_id: ownId, name: 'martin-nord', class: 'client', caps: 0b1010001 },
    radios: [
      {
        id: 'sim0',
        kind: 'sim',
        label: 'Simulated SX1262',
        connected: true,
        state: 'virtual',
        profile: 'MEDIUM_FAST',
        region: 'EU868',
      },
      {
        id: 'usb0',
        kind: 'serial',
        label: 'Heltec Wireless Tracker',
        connected: false,
        state: 'disconnected',
        profile: 'MEDIUM_FAST',
        region: 'EU868',
      },
    ],
    features: ['images', 'voice', 'map', 'groups', 'store_forward', 'backbone'],
    store: { messages: messages.length + groupMessages.length, contacts: contacts.length, channels: channels.length },
  };

  const config: ConfigData = {
    node: { name: 'martin-nord', class: 'client', announce_interval_s: 1800 },
    radio: {
      profile: 'MEDIUM_FAST',
      region: 'EU868',
      tx_power_dbm: 14,
      frequency_mhz: 869.525,
      interface: 'sim',
      max_power_dbm: 27,
      duty_cycle_pct: 10,
      max_bw_khz: 250,
    },
    features: { auto_announce: true, store_forward: true, position_sharing: true },
    channels,
    ...demoRegulatoryConfigFields('EU868'),
  };

  const positionHistories: Record<string, Position[]> = {
    [ownId]: makeTrail(ownPosition.lat, ownPosition.lon, ownPosition.timestamp_ms, 22, 11),
    [DEMO_SOLAR_RIDGE_ID]: makeTrail(59.9286, 10.5147, nowMs - 40_000, 14, 12),
    [DEMO_T1000E_ID]: makeTrail(59.976, 10.733, nowMs - 9 * MINUTE, 18, 13),
    [DEMO_LARS_ID]: makeTrail(59.936, 10.785, nowMs - 14 * MINUTE, 10, 14),
    [DEMO_ANNE_ID]: makeTrail(59.9137, 10.7522, nowMs - 3 * MINUTE, 8, 15),
  };
  positionHistories[ownId]!.push(ownPosition);

  /* ---------------- backbone mode (OBB/1, v0.2) ---------------- */
  // Deliberately obvious demo token — never a real-looking bearer secret.
  const backboneToken = 'demo-backbone-token-not-a-real-secret';
  const backboneConfig: BackboneConfig = {
    enabled: true,
    running: true,
    bind: '0.0.0.0',
    port: 9443,
    auth: 'token',
    token_set: true,
    max_peers: 32,
    allowlist: [],
    store_forward: true,
    name: 'oslo-backbone',
    public_host: 'backbone.openmesh.example',
    ping_interval_s: 30,
    totals: { frames_in: 1_842_112, frames_out: 1_799_450, bytes_in: 41_889_120, bytes_out: 38_912_450, peers: 3 },
  };
  const backboneStatus: BackboneStatus = {
    running: true,
    peers: 3,
    uptime_s: 1_234_567,
    frames_in: 1_842_112,
    frames_out: 1_799_450,
    bytes_in: 41_889_120,
    bytes_out: 38_912_450,
    throughput_in_bps: 8_240,
    throughput_out_bps: 7_610,
    store_forward_envelopes: 12,
    auth_failures: 17,
    last_error: null,
  };
  const backboneConnectionString = buildBackboneConnectionString({
    host: backboneConfig.public_host,
    port: backboneConfig.port,
    token: backboneToken,
    nodeId: ownId,
    name: backboneConfig.name,
  });
  const backbonePeers: BackbonePeer[] = [
    {
      node_id: DEMO_BACKBONE_PEER_LOFOTEN,
      name: 'lofoten-gateway',
      direction: 'inbound',
      remote_addr: '84.209.44.19:51824',
      connected_at_ms: nowMs - 2 * HOUR - 13 * MINUTE,
      last_seen_ms: nowMs - 8_000,
      frames_in: 642_118,
      frames_out: 598_004,
      bytes_in: 18_412_880,
      bytes_out: 16_940_120,
      rtt_ms: 38,
      app_version: '0.2.0',
      auth: 'token',
    },
    {
      node_id: DEMO_BACKBONE_PEER_TROMSO,
      name: 'tromso-relay',
      direction: 'outbound',
      remote_addr: 'vps-2.openmesh.example:9443',
      connected_at_ms: nowMs - 47 * MINUTE,
      last_seen_ms: nowMs - 31_000,
      frames_in: 184_220,
      frames_out: 201_988,
      bytes_in: 4_912_400,
      bytes_out: 5_388_120,
      rtt_ms: 61,
      app_version: '0.2.0',
      auth: 'token',
    },
    {
      node_id: DEMO_HYTTA_GATEWAY_ID,
      name: 'hytta-gateway',
      direction: 'inbound',
      remote_addr: '192.168.1.44:60122',
      connected_at_ms: nowMs - 6 * DAY,
      last_seen_ms: nowMs - 2_000,
      frames_in: 1_015_774,
      frames_out: 999_458,
      bytes_in: 18_563_840,
      bytes_out: 16_584_210,
      rtt_ms: 4,
      app_version: '0.2.0',
      auth: 'token',
    },
  ];
  const backboneSetup: BackboneSetup = buildFallbackSetup({
    config: backboneConfig,
    connectionString: backboneConnectionString,
    panelUrl: 'http://127.0.0.1:8787/backbone',
  });
  const backbone: DemoBackbone = {
    config: backboneConfig,
    status: backboneStatus,
    peers: backbonePeers,
    setup: backboneSetup,
    token: backboneToken,
  };

  /* ---------------- beginner setup + AI agents (v0.3) ---------------- */
  const setupRecommendation: SetupRecommendation = recommendedFor('NO', 'client');
  const setup: DemoSetupSeed = {
    state: {
      first_run: false,
      checks: { identity: true, radio: true, api_token: true, channel: true, backbone: true, agent: true },
      recommended: setupRecommendation,
      counts: {
        nodes: nodes.length,
        contacts: contacts.length,
        channels: channels.length,
        messages: messages.length + groupMessages.length,
      },
      ui_mode: 'beginner',
      regulatory_override: defaultRegulatoryOverride(),
      override_warning: null,
    },
    recommendation: setupRecommendation,
  };

  const agentActivities: ActivityEntry[] = [
    { id: 1, at_ms: nowMs - 2 * MINUTE, source: 'agent:claude', method: 'POST', path: '/api/messages', summary: 'send message to anne (24 bytes)', ok: true, error: null },
    { id: 2, at_ms: nowMs - 14 * MINUTE, source: 'user', method: 'POST', path: '/api/channels', summary: 'create channel “Familien”', ok: true, error: null },
    { id: 3, at_ms: nowMs - 27 * MINUTE, source: 'agent:claude', method: 'POST', path: '/api/announce', summary: 'force announce', ok: true, error: null },
    { id: 4, at_ms: nowMs - 46 * MINUTE, source: 'web', method: 'POST', path: '/api/position', summary: 'share position with channel “Familien”', ok: true, error: null },
    { id: 5, at_ms: nowMs - 65 * MINUTE, source: 'agent:claude', method: 'POST', path: '/api/contacts', summary: 'add contact d3e4f5a6b7c8d901', ok: false, error: 'contact already exists' },
    { id: 6, at_ms: nowMs - 2 * 60 * MINUTE, source: 'user', method: 'POST', path: '/api/messages/retry', summary: 'retry messages for lars', ok: true, error: null },
    { id: 7, at_ms: nowMs - 3 * 60 * MINUTE, source: 'cli', method: 'POST', path: '/api/config', summary: 'store-and-forward on', ok: true, error: null },
    { id: 8, at_ms: nowMs - 4 * 60 * MINUTE, source: 'agent:claude', method: 'POST', path: '/api/channels/messages', summary: 'send channel message to “Familien” (41 bytes)', ok: true, error: null },
  ];
  const agents: DemoAgentSeed = { configured: true, name: 'claude', activities: agentActivities };

  const gateways: GatewayConnector[] = [
    {
      id: 'sms-ops',
      kind: 'webhook',
      enabled: true,
      outbound: {
        url: 'https://gateway.example/openmesh',
        secret_env: 'OPENMESH_SMS_OUT_SECRET',
        channel_ids: [],
        include_direct: false,
        include_attachments: false,
        timeout_ms: 10_000,
        max_retries: 3,
      },
      inbound: {
        enabled: true,
        secret_env: 'OPENMESH_SMS_IN_SECRET',
        channel_id: channels[0]?.channel_id ?? 'dba1337a96acc686',
        max_chars: 400,
      },
      status: {
        last_success_ms: nowMs - 4 * MINUTE,
        last_failure_ms: null,
        last_error: null,
        retries: 0,
        out_secret_present: true,
        in_secret_present: true,
      },
    },
    {
      id: 'matrix-ops',
      kind: 'webhook',
      enabled: true,
      outbound: {
        url: 'https://matrix.example/_openmesh/webhook',
        secret_env: 'OPENMESH_MATRIX_OUT_SECRET',
        channel_ids: [channels[1]?.channel_id ?? 'c1a2b3d4e5f60718'],
        include_direct: true,
        include_attachments: false,
        timeout_ms: 10_000,
        max_retries: 3,
      },
      inbound: {
        enabled: false,
        secret_env: null,
        channel_id: null,
        max_chars: 400,
      },
      status: {
        last_success_ms: nowMs - 3 * 60 * MINUTE,
        last_failure_ms: nowMs - 6 * MINUTE,
        last_error: 'adapter connect timeout',
        retries: 3,
        out_secret_present: false,
        in_secret_present: false,
      },
    },
  ];

  const backboneLinks: BackboneLink[] = [
    {
      name: 'oslo',
      url: `openmesh://backbone?host=oslo.backbone.example&port=9443&token=demo-backbone-token-not-a-real-secret&node=${ownId}`,
      mode: 'client',
      enabled: true,
      connected: true,
      last_error: null,
      stats: { peers: 3, uptime_s: 7_320, frames_in: 1_284, frames_out: 964 },
    },
    {
      name: 'cabin',
      url: 'openmesh://backbone?host=cabin.backbone.example&port=9443&token=demo-cabin-token-not-a-real-secret',
      mode: 'client',
      enabled: true,
      connected: false,
      last_error: 'connection refused (retrying)',
      stats: null,
    },
  ];

  const mcp: McpHttpStatus = {
    enabled: true,
    path: '/mcp',
    allow_write: false,
    require_confirm: true,
    rate_limit_per_min: 60,
    max_sessions: 8,
    endpoint_url: 'http://127.0.0.1:8787/mcp',
    source: 'demo',
    active_sessions: 1,
    requests_per_min: 4,
  };

  return {
    now: nowMs,
    ownNodeId: ownId,
    backbone,
    setup,
    agents,
    gateways,
    backboneLinks,
    mcp,
    ownName: 'martin-nord',
    ownPosition,
    hello,
    status,
    config,
    nodes,
    contacts,
    channels,
    messages: [...messages].sort((a, b) => a.sent_at_ms - b.sent_at_ms),
    groupMessages: [...groupMessages].sort((a, b) => a.sent_at_ms - b.sent_at_ms),
    positionHistories,
    identities,
    channelPsks,
    trailPhoto,
    cabinMap,
    voiceNote,
  };
}

/** Contact deep-link for a demo node (used by the QR panel and mock bridge). */
export function demoContactUrl(dataset: DemoDataset, nodeId: string): string {
  const contact = dataset.contacts.find((c) => c.node_id === nodeId);
  return buildContactLink({
    nodeId,
    name: contact?.name ?? dataset.nodes.find((n) => n.node_id === nodeId)?.name ?? null,
    identity: dataset.identities[nodeId] ?? null,
  });
}

/** Channel invite deep-link, including the PSK so it can be pasted elsewhere. */
export function demoChannelUrl(dataset: DemoDataset, channelId: string): string {
  const channel = dataset.channels.find((c) => c.channel_id === channelId);
  return buildChannelLink({
    channelId,
    name: channel?.name ?? 'channel',
    psk: dataset.channelPsks[channelId] ?? generateChannelPsk(),
  });
}
