/**
 * Application store (zustand).
 *
 * Owns connection lifecycle, the normalised mesh state, optimistic sends, read
 * cursors and all bridge commands. All bridge I/O goes through the
 * `MeshTransport` abstraction, so live and demo mode share one code path.
 */

import { create } from 'zustand';
import type { StoreApi } from 'zustand';
import type {
  AckData,
  Channel,
  ChannelCreatedData,
  ChannelsEventData,
  ChatGroupMessage,
  ChatMessage,
  ConfigData,
  ConnectionState,
  Contact,
  ContactsEventData,
  Envelope,
  ErrorData,
  GroupMessageData,
  HelloData,
  MessageData,
  MessageUpdateData,
  NodeData,
  Position,
  PositionEventData,
  ResourceProgressData,
  Screen,
  StatusData,
  StoreOfferData,
  TransportMode,
} from '../types/api';
import type { BackboneConfig, BackboneConfigPatch, BackbonePeer, BackbonePeerEvent, BackboneSetup, BackboneStatus, BackboneStatusEvent } from '../types/backbone';
import { BackboneRestClient } from '../lib/backbone-api';
import { applyPeerEvent, buildBackboneConnectionString, buildFallbackSetup, mergeBackboneConfig, normalizeBackboneConfig, normalizeBackboneSetup } from '../lib/backbone';
import type { ConfigPatch } from './reducers';
import { deriveDelivery, mergeConfig, removeContact as removeContactReducer, upsertChannel, upsertGroupMessage, upsertMessage, upsertNode, applyAck, applyMessageUpdate, appendPositionHistory } from './reducers';
import type { MeshTransport, TransportStatus } from '../lib/transport';
import { LiveTransport, buildHttpBaseUrl } from '../lib/ws';
import { MockTransport } from '../lib/mock/transport';
import { createDemoDataset } from '../lib/mock/data';
import type { DemoDataset } from '../lib/mock/data';
import { localId } from '../lib/utils';
import { DEFAULT_PROFILE_ID, DEFAULT_REGION_ID } from '../lib/airtime';

export type DemoReason = 'forced' | 'user' | 'offline' | 'none';

export interface Toast {
  id: string;
  kind: 'info' | 'success' | 'error' | 'warn';
  title: string;
  message?: string;
}

export interface ActiveThread {
  kind: 'direct' | 'group';
  id: string;
}

export interface AppSettings {
  bridgeUrl: string;
  token: string;
  demoForced: boolean;
}

interface PendingSend {
  tempId: string;
  kind: 'direct' | 'group';
  createdAt: number;
}

type Resolver<T> = { resolve: (value: T) => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> };

const SETTINGS_KEY = 'openmesh.settings.v1';

const SCREEN_NAMES: Screen[] = ['chats', 'map', 'nodes', 'contacts', 'backbone', 'settings'];

/** Allow deep links/screenshots: ?screen=map&demo=1 */
function initialScreenFromUrl(): Screen {
  try {
    // Deep link: /backbone (route form) or ?screen=backbone (query form).
    if (/\/backbone\/?$/.test(window.location.pathname)) return 'backbone';
    const requested = new URLSearchParams(window.location.search).get('screen');
    if (requested && (SCREEN_NAMES as string[]).includes(requested)) return requested as Screen;
  } catch {
    // ignore
  }
  return 'chats';
}
const DEMO_KEY = 'openmesh.demo';
const PROBE_TIMEOUT_MS = 1800;
const COMMAND_TIMEOUT_MS = 12_000;

function readSettings(): AppSettings {
  const defaults: AppSettings = {
    bridgeUrl: 'http://127.0.0.1:8787',
    token: '',
    demoForced: false,
  };
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    if (!raw) return defaults;
    const parsed = JSON.parse(raw) as Partial<AppSettings>;
    return {
      bridgeUrl: parsed.bridgeUrl || defaults.bridgeUrl,
      token: parsed.token || '',
      demoForced: Boolean(parsed.demoForced),
    };
  } catch {
    return defaults;
  }
}

function persistSettings(settings: AppSettings): void {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
  } catch {
    // Storage can be unavailable (private mode, Tauri sandbox); not fatal.
  }
}

function forcedDemoFromEnvironment(): boolean {
  try {
    const params = new URLSearchParams(window.location.search);
    if (params.get('demo') === '1' || params.get('demo') === 'true') return true;
    if (localStorage.getItem(DEMO_KEY) === '1') return true;
  } catch {
    // ignore
  }
  return false;
}

async function probeBridge(baseUrl: string): Promise<boolean> {
  try {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), PROBE_TIMEOUT_MS);
    const response = await fetch(`${buildHttpBaseUrl(baseUrl)}/api/health`, {
      signal: controller.signal,
      headers: { Accept: 'application/json' },
    });
    clearTimeout(timer);
    return response.ok;
  } catch {
    return false;
  }
}

/* ------------------------------------------------------------------ */
/* Module-level transport + command resolvers                         */
/* ------------------------------------------------------------------ */

let transport: MeshTransport | null = null;
const pendingSends = new Map<string, PendingSend>();
const pendingRetries = new Map<number, ReturnType<typeof setTimeout>>();
const commandResolvers = new Map<string, Resolver<unknown>>();

function resolveCommand<T>(commandId: string, value: T): void {
  const resolver = commandResolvers.get(commandId);
  if (!resolver) return;
  clearTimeout(resolver.timer);
  commandResolvers.delete(commandId);
  resolver.resolve(value);
}

function rejectCommand(commandId: string, error: Error): void {
  const resolver = commandResolvers.get(commandId);
  if (!resolver) return;
  clearTimeout(resolver.timer);
  commandResolvers.delete(commandId);
  resolver.reject(error);
}

function registerCommand<T>(commandId: string, timeoutMs = COMMAND_TIMEOUT_MS): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => {
      commandResolvers.delete(commandId);
      reject(new Error('The bridge did not answer in time'));
    }, timeoutMs);
    commandResolvers.set(commandId, {
      resolve: (value) => resolve(value as T),
      reject,
      timer,
    });
  });
}

/* ------------------------------------------------------------------ */
/* Store types                                                         */
/* ------------------------------------------------------------------ */

export interface AttachmentDraft {
  blob: Blob;
  mime: string;
  filename?: string;
  durationMs?: number;
  waveform?: number[];
  localUrl: string;
  width?: number | null;
  height?: number | null;
  caption?: string;
}

export interface AppState {
  /* connection */
  connection: ConnectionState;
  connectionDetail: string | null;
  reconnectAttempt: number;
  mode: TransportMode;
  demoReason: DemoReason;
  syncing: boolean;
  bridgeVersion: string | null;
  hello: HelloData | null;
  config: ConfigData | null;
  bridgeStatus: StatusData | null;
  storeOffer: (StoreOfferData & { at_ms: number }) | null;

  /* identity */
  ownNodeId: string | null;
  ownName: string;
  ownPosition: Position | null;
  ownAccuracyM: number | null;

  /* mesh state */
  nodes: Record<string, NodeData>;
  contacts: Record<string, Contact>;
  channels: Record<string, Channel>;
  messages: ChatMessage[];
  groupMessages: ChatGroupMessage[];
  positionHistory: Record<string, Position[]>;
  readCursors: Record<string, number>;

  /* ui state */
  screen: Screen;
  activeThread: ActiveThread | null;
  replyTo: number | null;
  settings: AppSettings;
  toasts: Toast[];
  lastCreatedChannel: ChannelCreatedData | null;
  lastInvite: { kind: 'contact' | 'channel'; url: string } | null;
  commandError: string | null;
  selectedNodeId: string | null;
  resolveAttachmentUrl: (attachmentId: string, mime?: string | null) => Promise<string>;
  inspectNode: (nodeId: string | null) => void;

  /* backbone mode (OBB/1, v0.2) */
  backboneConfig: BackboneConfig | null;
  backboneStatus: BackboneStatus | null;
  backbonePeers: BackbonePeer[];
  backboneSetup: BackboneSetup | null;
  /** Bearer token only known after a rotate or from the demo seed; never persisted. */
  backboneSessionToken: string | null;
  /** True only when the session token came from a rotation in this panel session. */
  backboneTokenFresh: boolean;
  backboneLoading: boolean;
  backboneSaving: boolean;
  backboneError: string | null;
  backboneLastUpdated: number | null;

  /* actions */
  bootstrap: () => Promise<void>;
  retryConnection: () => Promise<void>;
  setScreen: (screen: Screen) => void;
  openThread: (thread: ActiveThread) => void;
  closeThread: () => void;
  setReplyTo: (msgId: number | null) => void;
  saveSettings: (settings: Partial<AppSettings>) => void;
  setDemoMode: (demo: boolean) => Promise<void>;
  sendText: (text: string) => Promise<void>;
  sendGroupText: (text: string) => Promise<void>;
  sendAttachment: (draft: AttachmentDraft, thread?: ActiveThread) => Promise<void>;
  retryMessage: (msgId: number) => void;
  markActiveThreadRead: () => void;
  sharePosition: (target: { channelId?: string; peer?: string }) => Promise<void>;
  updateConfig: (patch: ConfigPatch) => void;
  announceNow: () => void;
  refreshNodes: () => void;
  addContact: (contact: { nodeId: string; name: string; identity?: string | null }) => Promise<void>;
  removeContact: (nodeId: string) => Promise<void>;
  createChannel: (name: string) => Promise<ChannelCreatedData>;
  joinChannel: (name: string, psk: string) => Promise<ChannelCreatedData>;
  leaveChannel: (channelId: string) => void;
  requestContactQr: (nodeId: string) => Promise<string>;
  requestChannelInvite: (channelId: string) => Promise<string>;
  clearLastCreatedChannel: () => void;
  clearInvite: () => void;
  resetIdentity: () => Promise<void>;
  setOwnPositionFromGeo: (position: Position, accuracyM?: number | null) => void;
  loadBackbone: () => Promise<void>;
  saveBackboneConfig: (patch: BackboneConfigPatch) => Promise<boolean>;
  setBackboneEnabled: (enabled: boolean) => Promise<boolean>;
  kickBackbonePeer: (nodeId: string) => Promise<void>;
  kickAllBackbonePeers: () => Promise<number>;
  rotateBackboneToken: () => Promise<string>;
  loadBackboneSetup: () => Promise<BackboneSetup | null>;
  pushToast: (toast: Omit<Toast, 'id'>) => void;
  dismissToast: (id: string) => void;
  handleEvent: (event: Envelope) => void;
  handleStatus: (status: TransportStatus) => void;
  hydrateDemo: (dataset: DemoDataset) => void;
}

/* ------------------------------------------------------------------ */
/* Helpers                                                            */
/* ------------------------------------------------------------------ */

type SetState = StoreApi<AppState>['setState'];
type GetState = () => AppState;

function cloneValue<T>(value: T): T {
  if (value === undefined || value === null) return value;
  if (typeof structuredClone === 'function') return structuredClone(value);
  return JSON.parse(JSON.stringify(value)) as T;
}

function backboneRestClient(settings: AppSettings): BackboneRestClient {
  return new BackboneRestClient({
    baseUrl: settings.bridgeUrl,
    token: settings.token || null,
    timeoutMs: 6000,
  });
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

/** Best-effort connection string from the current panel state (token may be unknown). */
function connectionStringFromState(state: Pick<AppState, 'backboneConfig' | 'backboneSessionToken' | 'ownNodeId' | 'backboneSetup'>): string {
  const config = state.backboneConfig;
  if (!config) return state.backboneSetup?.connection_string ?? '';
  const token = config.auth === 'token' ? state.backboneSessionToken : null;
  return buildBackboneConnectionString({
    host: config.public_host || config.bind || '127.0.0.1',
    port: config.port,
    token,
    nodeId: state.ownNodeId,
    name: config.name,
  });
}

function syncScreenUrl(screen: Screen): void {
  if (typeof window === 'undefined' || !window.history?.pushState) return;
  try {
    const url = new URL(window.location.href);
    const wantsBackbone = screen === 'backbone';
    const isBackbone = /\/backbone\/?$/.test(url.pathname);
    if (wantsBackbone && !isBackbone) {
      url.pathname = `${url.pathname.replace(/\/+$/, '')}/backbone`;
      window.history.pushState({ screen }, '', url.toString());
    } else if (!wantsBackbone && isBackbone) {
      url.pathname = url.pathname.replace(/\/backbone\/?$/, '/');
      window.history.pushState({ screen }, '', url.toString());
    }
  } catch {
    // History API unavailable (sandboxed webview) — deep links still work on load.
  }
}

function demoChatMessage(message: DemoDataset['messages'][number]): ChatMessage {
  const { localUrl, waveform, demoDelivery, ...wire } = message;
  return {
    ...wire,
    delivery: demoDelivery ?? deriveDelivery(wire),
    seeded: true,
    ...(localUrl ? { localUrl } : {}),
    ...(waveform ? { waveform } : {}),
  };
}

function demoGroupMessage(message: DemoDataset['groupMessages'][number]): ChatGroupMessage {
  const { localUrl, waveform, ...wire } = message;
  return {
    ...wire,
    seeded: true,
    ...(localUrl ? { localUrl } : {}),
    ...(waveform ? { waveform } : {}),
  };
}

function upsertAllMessages(list: ChatMessage[], incoming: MessageData[]): ChatMessage[] {
  let next = list;
  for (const message of incoming) next = upsertMessage(next, message);
  return next.sort((a, b) => a.sent_at_ms - b.sent_at_ms);
}

/** Find an optimistic row that an outgoing echo should replace (same thread + text). */
function findOptimisticDirect(messages: ChatMessage[], data: MessageData): ChatMessage | undefined {
  if (!data.outgoing) return undefined;
  const threeMinutesAgo = Date.now() - 3 * 60_000;
  return messages.find(
    (m) =>
      m.outgoing &&
      m.tempId !== undefined &&
      m.peer === data.peer &&
      m.text === data.text &&
      m.sent_at_ms >= threeMinutesAgo &&
      m.msg_id === 0,
  );
}

function findOptimisticGroup(messages: ChatGroupMessage[], data: GroupMessageData): ChatGroupMessage | undefined {
  if (!data.outgoing) return undefined;
  const threeMinutesAgo = Date.now() - 3 * 60_000;
  return messages.find(
    (m) =>
      m.outgoing &&
      m.tempId !== undefined &&
      m.channel_id === data.channel_id &&
      m.text === data.text &&
      m.sent_at_ms >= threeMinutesAgo &&
      m.msg_id === 0,
  );
}

function attachTransport(set: SetState, get: GetState, next: MeshTransport): void {
  transport?.dispose();
  transport = next;
  next.onEvent((event) => get().handleEvent(event));
  next.onStatus((status) => get().handleStatus(status));
  set({ mode: next.mode });
  next.connect();
}

/** Simulate a read for a freshly opened thread and tell the bridge where possible. */
function markThreadRead(set: SetState, get: GetState): void {
  const state = get();
  const thread = state.activeThread;
  if (!thread) return;
  const now = Date.now();
  if (thread.kind === 'direct') {
    let lastUnreadId = null as number | null;
    const messages = state.messages.map((m) => {
      if (m.peer === thread.id && !m.outgoing && !m.read) {
        lastUnreadId = m.msg_id;
        return { ...m, read: true };
      }
      return m;
    });
    set({ messages });
    if (lastUnreadId && transport) {
      transport.send('mark_read', { msg_id: lastUnreadId });
    }
  } else {
    set({ readCursors: { ...state.readCursors, [thread.id]: now } });
  }
}

function watchdogPendingSend(set: SetState, get: GetState, commandId: string, tempId: string): void {
  setTimeout(() => {
    const pending = pendingSends.get(commandId);
    if (!pending) return;
    pendingSends.delete(commandId);
    const state = get();
    set({
      messages: state.messages.map((m) =>
        m.tempId === tempId && m.delivery === 'pending'
          ? { ...m, delivery: 'failed', error: 'No response from the bridge' }
          : m,
      ),
      groupMessages: state.groupMessages.map((m) =>
        m.tempId === tempId ? { ...m, error: 'No response from the bridge' } : m,
      ),
      commandError: 'The bridge did not acknowledge the send. Check the connection.',
    });
  }, COMMAND_TIMEOUT_MS);
}

/* ------------------------------------------------------------------ */
/* Store                                                              */
/* ------------------------------------------------------------------ */

const initialSettings = readSettings();

export const useAppStore = create<AppState>()((set, get) => ({
  connection: 'idle',
  connectionDetail: null,
  reconnectAttempt: 0,
  mode: 'live',
  demoReason: 'none',
  syncing: false,
  bridgeVersion: null,
  hello: null,
  config: null,
  bridgeStatus: null,
  storeOffer: null,

  ownNodeId: null,
  ownName: 'this node',
  ownPosition: null,
  ownAccuracyM: null,

  nodes: {},
  contacts: {},
  channels: {},
  messages: [],
  groupMessages: [],
  positionHistory: {},
  readCursors: {},

  screen: initialScreenFromUrl(),
  activeThread: null,
  replyTo: null,
  settings: initialSettings,
  toasts: [],
  lastCreatedChannel: null,
  lastInvite: null,
  commandError: null,
  selectedNodeId: null,

  backboneConfig: null,
  backboneStatus: null,
  backbonePeers: [],
  backboneSetup: null,
  backboneSessionToken: null,
  backboneTokenFresh: false,
  backboneLoading: false,
  backboneSaving: false,
  backboneError: null,
  backboneLastUpdated: null,

  resolveAttachmentUrl: async (attachmentId, mime) => {
    if (!transport) throw new Error('Not connected');
    return transport.resolveAttachmentUrl(attachmentId, mime);
  },

  inspectNode: (nodeId) => {
    set({ selectedNodeId: nodeId });
    if (nodeId) set({ screen: 'nodes' });
  },

  /* -------------------- lifecycle -------------------- */

  bootstrap: async () => {
    if (get().connection !== 'idle') return;
    set({ connection: 'connecting', syncing: true });

    const settings = get().settings;
    const forced = settings.demoForced || forcedDemoFromEnvironment();
    if (!forced) {
      const reachable = await probeBridge(settings.bridgeUrl);
      if (reachable) {
        const live = new LiveTransport({ baseUrl: settings.bridgeUrl, token: settings.token });
        attachTransport(set, get, live);
        return;
      }
    }

    const dataset = createDemoDataset();
    get().hydrateDemo(dataset);
    set({ demoReason: forced ? (settings.demoForced ? 'user' : 'forced') : 'offline' });
    // Deep link for docs/screenshots: ?thread=anne or ?thread=familien (or a raw id).
    try {
      const requested = new URLSearchParams(window.location.search).get('thread');
      if (requested) {
        const needle = requested.trim().toLowerCase();
        const channel = Object.values(dataset.channels).find(
          (candidate) => candidate.name.toLowerCase() === needle || candidate.channel_id === needle,
        );
        const node = [...dataset.nodes, ...dataset.contacts.map((contact) => ({ node_id: contact.node_id, name: contact.name }))].find(
          (candidate) => candidate.name.toLowerCase() === needle || candidate.node_id === needle,
        );
        if (channel) set({ activeThread: { kind: 'group', id: channel.channel_id } });
        else if (node) set({ activeThread: { kind: 'direct', id: node.node_id } });
      }
    } catch {
      // ignore malformed query params
    }
    const mock = new MockTransport({ dataset });
    attachTransport(set, get, mock);
    if (!forced) {
      get().pushToast({
        kind: 'warn',
        title: 'Bridge not found — demo mode',
        message: 'Showing a seeded Oslo mesh. Start the bridge or set the URL in Settings, then retry.',
      });
    }
  },

  retryConnection: async () => {
    const settings = get().settings;
    set({ connection: 'connecting', connectionDetail: 'Probing bridge…' });
    const reachable = await probeBridge(settings.bridgeUrl);
    if (!reachable) {
      set({ connection: 'demo', connectionDetail: 'Bridge still unreachable' });
      get().pushToast({
        kind: 'error',
        title: 'Still no bridge',
        message: `Nothing answered at ${settings.bridgeUrl}. Check the URL and token in Settings.`,
      });
      return;
    }
    const live = new LiveTransport({ baseUrl: settings.bridgeUrl, token: settings.token });
    set({ demoReason: 'none', toasts: [] });
    attachTransport(set, get, live);
    get().pushToast({ kind: 'success', title: 'Connected to bridge', message: settings.bridgeUrl });
  },

  handleStatus: (status) => {
    set({
      connection: status.state,
      connectionDetail: status.detail ?? null,
      reconnectAttempt: status.attempt ?? 0,
      ...(status.state === 'connected' || status.state === 'demo' ? { syncing: false } : {}),
    });
  },

  /* -------------------- demo hydration -------------------- */

  hydrateDemo: (dataset) => {
    const nodes: Record<string, NodeData> = {};
    for (const node of dataset.nodes) nodes[node.node_id] = node;
    const contacts: Record<string, Contact> = {};
    for (const contact of dataset.contacts) contacts[contact.node_id] = { ...contact, seeded: true };
    const channels: Record<string, Channel> = {};
    for (const channel of dataset.channels) channels[channel.channel_id] = { ...channel, seeded: true };
    const familyChannel = dataset.channels[0];
    const turChannel = dataset.channels[1];
    const familyMessages = familyChannel
      ? dataset.groupMessages.filter((m) => m.channel_id === familyChannel.channel_id)
      : [];
    const lastFamilyIncoming = [...familyMessages].reverse().find((m) => !m.outgoing);
    const readCursors: Record<string, number> = {};
    if (familyChannel && lastFamilyIncoming) {
      readCursors[familyChannel.channel_id] = lastFamilyIncoming.sent_at_ms - 1;
    }
    if (turChannel) readCursors[turChannel.channel_id] = dataset.now;

    set({
      mode: 'demo',
      connection: 'demo',
      syncing: false,
      demoReason: get().demoReason === 'none' ? 'user' : get().demoReason,
      bridgeVersion: dataset.hello.bridge_version,
      hello: dataset.hello,
      config: dataset.config,
      bridgeStatus: dataset.status,
      ownNodeId: dataset.ownNodeId,
      ownName: dataset.ownName,
      ownPosition: dataset.ownPosition,
      ownAccuracyM: dataset.ownPosition.accuracy_m ?? null,
      nodes,
      contacts,
      channels,
      messages: dataset.messages.map(demoChatMessage).sort((a, b) => a.sent_at_ms - b.sent_at_ms),
      groupMessages: dataset.groupMessages.map(demoGroupMessage).sort((a, b) => a.sent_at_ms - b.sent_at_ms),
      positionHistory: Object.fromEntries(
        Object.entries(dataset.positionHistories).map(([key, value]) => [key, [...value]]),
      ),
      readCursors,
      backboneConfig: cloneValue(dataset.backbone.config),
      backboneStatus: cloneValue(dataset.backbone.status),
      backbonePeers: cloneValue(dataset.backbone.peers),
      backboneSetup: cloneValue(dataset.backbone.setup),
      backboneSessionToken: dataset.backbone.token,
      backboneTokenFresh: false,
      backboneLoading: false,
      backboneSaving: false,
      backboneError: null,
      backboneLastUpdated: Date.now(),
    });
  },

  /* -------------------- navigation -------------------- */

  setScreen: (screen) => {
    set({ screen });
    syncScreenUrl(screen);
  },

  openThread: (thread) => {
    set({ activeThread: thread, replyTo: null });
    markThreadRead(set, get);
  },

  closeThread: () => set({ activeThread: null, replyTo: null }),

  setReplyTo: (msgId) => set({ replyTo: msgId }),

  /* -------------------- settings -------------------- */

  saveSettings: (patch) => {
    const settings: AppSettings = { ...get().settings, ...patch };
    persistSettings(settings);
    set({ settings });
  },

  setDemoMode: async (demo) => {
    get().saveSettings({ demoForced: demo });
    try {
      if (demo) {
        const dataset = createDemoDataset();
        get().hydrateDemo(dataset);
        set({ demoReason: 'user' });
        attachTransport(set, get, new MockTransport({ dataset }));
      } else {
        get().retryConnection();
      }
    } catch (error) {
      get().pushToast({ kind: 'error', title: 'Could not switch mode', message: String(error) });
    }
  },

  /* -------------------- threading -------------------- */

  sendText: async (text) => {
    const state = get();
    const thread = state.activeThread;
    const trimmed = text.trim();
    if (!thread || !trimmed) return;
    set({ replyTo: null });
    const tempId = localId('msg');
    const now = Date.now();
    const replyTo = state.replyTo;

    if (thread.kind === 'direct') {
      const node = state.nodes[thread.id];
      const optimistic: ChatMessage = {
        msg_id: 0,
        peer: thread.id,
        from: state.ownNodeId ?? '',
        to: thread.id,
        kind: 'text',
        text: trimmed,
        sent_at_ms: now,
        received_at_ms: null,
        outgoing: true,
        delivered: false,
        read: false,
        reply_to: replyTo,
        thread: null,
        attachment: null,
        position: null,
        delivery: 'pending',
        tempId,
      };
      set({ messages: [...state.messages, optimistic].sort((a, b) => a.sent_at_ms - b.sent_at_ms) });
      void node;
      if (!transport) return;
      const commandId = transport.send('send_message', {
        to: thread.id,
        text: trimmed,
        reply_to: replyTo,
        attachment_id: null,
      });
      pendingSends.set(commandId, { tempId, kind: 'direct', createdAt: now });
      watchdogPendingSend(set, get, commandId, tempId);
      return;
    }

    const channel = state.channels[thread.id];
    const optimistic: ChatGroupMessage = {
      msg_id: 0,
      channel_id: thread.id,
      channel_name: channel?.name ?? 'channel',
      from: state.ownNodeId ?? '',
      from_name: state.ownName,
      sender_key: '',
      kind: 'text',
      text: trimmed,
      sent_at_ms: now,
      outgoing: true,
      signature_valid: true,
      attachment: null,
      position: null,
      tempId,
    };
    set({
      groupMessages: [...state.groupMessages, optimistic].sort((a, b) => a.sent_at_ms - b.sent_at_ms),
    });
    if (!transport) return;
    const commandId = transport.send('send_group_message', {
      channel_id: thread.id,
      text: trimmed,
      attachment_id: null,
    });
    pendingSends.set(commandId, { tempId, kind: 'group', createdAt: now });
    watchdogPendingSend(set, get, commandId, tempId);
  },

  sendGroupText: async (text) => {
    const state = get();
    if (state.activeThread?.kind !== 'group') return;
    await state.sendText(text);
  },

  sendAttachment: async (draft, threadOverride) => {
    const state = get();
    const thread = threadOverride ?? state.activeThread;
    if (!thread || !transport) return;
    const tempId = localId('att');
    const now = Date.now();
    const caption = draft.caption?.trim() || null;
    const attachment = {
      id: '',
      mime: draft.mime,
      size: draft.blob.size,
      sha256: '',
      duration_ms: draft.durationMs ?? null,
      name: draft.filename ?? null,
      progress: 0,
      width: draft.width ?? null,
      height: draft.height ?? null,
    };

    if (thread.kind === 'direct') {
      const optimistic: ChatMessage = {
        msg_id: 0,
        peer: thread.id,
        from: state.ownNodeId ?? '',
        to: thread.id,
        kind: 'attachment',
        text: caption,
        sent_at_ms: now,
        received_at_ms: null,
        outgoing: true,
        delivered: false,
        read: false,
        reply_to: null,
        thread: null,
        attachment,
        position: null,
        delivery: 'pending',
        tempId,
        localUrl: draft.localUrl,
        waveform: draft.waveform,
      };
      set({ messages: [...state.messages, optimistic].sort((a, b) => a.sent_at_ms - b.sent_at_ms) });
    } else {
      const channel = state.channels[thread.id];
      const optimistic: ChatGroupMessage = {
        msg_id: 0,
        channel_id: thread.id,
        channel_name: channel?.name ?? 'channel',
        from: state.ownNodeId ?? '',
        from_name: state.ownName,
        sender_key: '',
        kind: 'attachment',
        text: caption,
        sent_at_ms: now,
        outgoing: true,
        signature_valid: true,
        attachment,
        position: null,
        tempId,
        localUrl: draft.localUrl,
        waveform: draft.waveform,
      };
      set({
        groupMessages: [...state.groupMessages, optimistic].sort((a, b) => a.sent_at_ms - b.sent_at_ms),
      });
    }

    const updateAttachmentProgress = (patch: Partial<typeof attachment>) => {
      const current = get();
      set({
        messages: current.messages.map((m) =>
          m.tempId === tempId ? { ...m, attachment: { ...attachment, ...m.attachment, ...patch } } : m,
        ),
        groupMessages: current.groupMessages.map((m) =>
          m.tempId === tempId ? { ...m, attachment: { ...attachment, ...m.attachment, ...patch } } : m,
        ),
      });
    };

    try {
      const uploaded = await transport.uploadAttachment(draft.blob, draft.filename);
      updateAttachmentProgress({ id: uploaded.id, size: uploaded.size, sha256: uploaded.sha256, progress: 1 });
      const commandId =
        thread.kind === 'direct'
          ? transport.send('send_message', {
              to: thread.id,
              text: caption,
              reply_to: null,
              attachment_id: uploaded.id,
            })
          : transport.send('send_group_message', {
              channel_id: thread.id,
              text: caption,
              attachment_id: uploaded.id,
            });
      pendingSends.set(commandId, { tempId, kind: thread.kind, createdAt: now });
      watchdogPendingSend(set, get, commandId, tempId);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      const current = get();
      set({
        messages: current.messages.map((m) =>
          m.tempId === tempId ? { ...m, delivery: 'failed', error: message } : m,
        ),
        groupMessages: current.groupMessages.map((m) =>
          m.tempId === tempId ? { ...m, error: message } : m,
        ),
        commandError: message,
      });
      get().pushToast({ kind: 'error', title: 'Upload failed', message });
    }
  },

  retryMessage: (msgId) => {
    const state = get();
    const target = state.messages.find((m) => m.msg_id === msgId) ?? state.groupMessages.find((m) => m.msg_id === msgId);
    if (!target || !target.outgoing || !transport) return;
    set({
      messages: state.messages.map((m) =>
        m.msg_id === msgId ? { ...m, delivery: 'pending', error: undefined } : m,
      ),
      groupMessages: state.groupMessages.map((m) =>
        m.msg_id === msgId ? { ...m, error: undefined } : m,
      ),
    });
    transport.send('retry', { msg_id: msgId });
    const timer = setTimeout(() => {
      if (!pendingRetries.has(msgId)) return;
      pendingRetries.delete(msgId);
      const current = get();
      set({
        messages: current.messages.map((m) =>
          m.msg_id === msgId ? { ...m, delivery: 'failed', error: 'Still no acknowledgement' } : m,
        ),
      });
    }, COMMAND_TIMEOUT_MS);
    pendingRetries.set(msgId, timer);
    get().pushToast({ kind: 'info', title: 'Retrying', message: 'Asking the bridge to resend…' });
  },

  markActiveThreadRead: () => markThreadRead(set, get),

  /* -------------------- position -------------------- */

  sharePosition: async (target) => {
    if (!transport) return;
    const own = get().ownPosition;
    const payload: Record<string, unknown> = target.channelId
      ? { channel_id: target.channelId }
      : { to: target.peer ?? '' };
    if (own) {
      payload.position = {
        lat: own.lat,
        lon: own.lon,
        accuracy_m: own.accuracy_m ?? null,
        timestamp_ms: Date.now(),
      };
    }
    transport.send('send_position', payload);
    get().pushToast({ kind: 'success', title: 'Position queued', message: 'The bridge will transmit on the next slot.' });
  },

  setOwnPositionFromGeo: (position, accuracyM) => {
    const state = get();
    const ownId = state.ownNodeId;
    set({
      ownPosition: position,
      ownAccuracyM: accuracyM ?? position.accuracy_m ?? null,
      positionHistory: ownId
        ? appendPositionHistory(state.positionHistory, ownId, position)
        : state.positionHistory,
    });
  },

  /* -------------------- config -------------------- */

  updateConfig: (patch) => {
    const state = get();
    const merged = mergeConfig(state.config, patch);
    set({ config: merged });
    if (patch.node?.name) set({ ownName: patch.node.name });
    transport?.send('set_config', patch as unknown as Record<string, unknown>);
  },

  refreshNodes: () => {
    transport?.send('get_nodes', {});
  },

  announceNow: () => {
    transport?.send('announce', {});
    get().pushToast({
      kind: 'info',
      title: 'Announcement queued',
      message: 'Nodes within range will hear you on the next slot.',
    });
  },

  /* -------------------- contacts -------------------- */

  addContact: async (contact) => {
    if (!transport) throw new Error('Not connected');
    const commandId = transport.send('add_contact', {
      node_id: contact.nodeId,
      name: contact.name,
      identity: contact.identity ?? '',
      verify_key: true,
    });
    try {
      await registerCommand<unknown>(commandId, 4_000);
      get().pushToast({ kind: 'success', title: 'Contact added', message: contact.name });
    } catch {
      // The mock/bridge may reply with the `contacts` event instead of an echo.
      get().pushToast({ kind: 'info', title: 'Adding contact', message: 'Waiting for the bridge to confirm…' });
    }
  },

  removeContact: async (nodeId) => {
    if (!transport) return;
    transport.send('remove_contact', { node_id: nodeId });
    const state = get();
    set({ contacts: removeContactReducer(state.contacts, nodeId) });
    get().pushToast({ kind: 'info', title: 'Contact removed' });
  },

  /* -------------------- channels -------------------- */

  createChannel: async (name) => {
    if (!transport) throw new Error('Not connected to a bridge');
    const commandId = transport.send('create_channel', { name });
    const result = await registerCommand<ChannelCreatedData>(commandId);
    set({ lastCreatedChannel: result });
    applyChannelCreated(set, get, result);
    return result;
  },

  joinChannel: async (name, psk) => {
    if (!transport) throw new Error('Not connected to a bridge');
    const commandId = transport.send('join_channel', { name, psk });
    const result = await registerCommand<ChannelCreatedData>(commandId);
    set({ lastCreatedChannel: result });
    applyChannelCreated(set, get, result);
    return result;
  },

  leaveChannel: (channelId) => {
    transport?.send('leave_channel', { channel_id: channelId });
    const state = get();
    const channels = { ...state.channels };
    delete channels[channelId];
    set({
      channels,
      activeThread: state.activeThread?.id === channelId ? null : state.activeThread,
    });
  },

  requestContactQr: async (nodeId) => {
    if (!transport) throw new Error('Not connected to a bridge');
    const commandId = transport.send('contact_qr', { node_id: nodeId });
    const result = await registerCommand<{ url: string }>(commandId);
    set({ lastInvite: { kind: 'contact', url: result.url } });
    return result.url;
  },

  requestChannelInvite: async (channelId) => {
    if (!transport) throw new Error('Not connected to a bridge');
    const commandId = transport.send('channel_invite', { channel_id: channelId });
    const result = await registerCommand<{ url: string }>(commandId);
    set({ lastInvite: { kind: 'channel', url: result.url } });
    return result.url;
  },

  clearLastCreatedChannel: () => set({ lastCreatedChannel: null }),
  clearInvite: () => set({ lastInvite: null }),

  resetIdentity: async () => {
    if (get().mode === 'demo') {
      const dataset = createDemoDataset();
      get().hydrateDemo(dataset);
      set({ activeThread: null, screen: 'settings' });
      get().pushToast({
        kind: 'success',
        title: 'Demo identity reset',
        message: 'A fresh seeded identity was generated locally.',
      });
      return;
    }
    get().pushToast({
      kind: 'warn',
      title: 'Reset from the bridge, not the UI',
      message:
        'The bridge API does not expose identity reset yet. Stop the bridge and run it with --reset-identity to generate a new keypair.',
    });
  },


  /* -------------------- backbone mode (OBB/1) -------------------- */

  loadBackbone: async () => {
    const state = get();
    if (state.connection === 'idle' || state.connection === 'connecting') return;
    set({ backboneLoading: true, backboneError: null });
    if (state.mode === 'demo') {
      transport?.send('get_backbone', {});
      set({ backboneLoading: false, backboneLastUpdated: Date.now() });
      return;
    }
    const client = backboneRestClient(state.settings);
    try {
      const config = await client.getConfig();
      if (!config.enabled) {
        set({
          backboneConfig: config,
          backboneStatus: null,
          backbonePeers: [],
          backboneLoading: false,
          backboneError: null,
          backboneLastUpdated: Date.now(),
        });
        return;
      }
      const [peersResult, setupResult] = await Promise.allSettled([client.getPeers(), client.getSetup()]);
      const fallback = buildFallbackSetup({
        config,
        connectionString: buildBackboneConnectionString({
          host: config.public_host || config.bind,
          port: config.port,
          token: null,
          nodeId: state.ownNodeId,
          name: config.name,
        }),
      });
      set({
        backboneConfig: config,
        backbonePeers: peersResult.status === 'fulfilled' ? peersResult.value : get().backbonePeers,
        backboneSetup:
          setupResult.status === 'fulfilled' ? normalizeBackboneSetup(setupResult.value, fallback) : fallback,
        backboneLoading: false,
        backboneError: null,
        backboneLastUpdated: Date.now(),
      });
    } catch (error) {
      const message = errorMessage(error);
      set({
        backboneLoading: false,
        backboneError:
          message.includes('401') || /unauthori[sz]ed/i.test(message)
            ? 'The bridge rejected the token. Add the bridge bearer token in Settings, then retry.'
            : message,
      });
    }
  },

  saveBackboneConfig: async (patch) => {
    const state = get();
    const previous = state.backboneConfig;
    if (!previous) return false;
    set({ backboneConfig: mergeBackboneConfig(previous, patch), backboneSaving: true, backboneError: null });
    try {
      if (state.mode === 'demo') {
        transport?.send('set_backbone_config', patch as Record<string, unknown>);
        set({ backboneSaving: false, backboneLastUpdated: Date.now() });
        get().pushToast({
          kind: 'success',
          title: 'Backbone config applied',
          message: 'Demo backbone — changes are simulated locally.',
        });
        return true;
      }
      const response = await backboneRestClient(state.settings).updateConfig(patch);
      set({
        backboneConfig: { ...response, totals: response.totals ?? previous.totals },
        backboneSaving: false,
        backboneLastUpdated: Date.now(),
      });
      if (patch.enabled !== undefined || response.restarted) void get().loadBackbone();
      get().pushToast({
        kind: 'success',
        title: 'Backbone config saved',
        message: response.restarted ? 'Listener restarted with the new settings.' : 'Applied live — no restart needed.',
      });
      return true;
    } catch (error) {
      set({ backboneConfig: previous, backboneSaving: false, backboneError: errorMessage(error) });
      get().pushToast({ kind: 'error', title: 'Could not save backbone config', message: errorMessage(error) });
      return false;
    }
  },

  setBackboneEnabled: async (enabled) => {
    const ok = await get().saveBackboneConfig({ enabled });
    if (ok) {
      get().pushToast({
        kind: enabled ? 'success' : 'info',
        title: enabled ? 'Backbone enabled' : 'Backbone disabled',
        message: enabled ? 'Listening for authenticated peers on the configured port.' : 'The public listener is now closed.',
      });
    }
    return ok;
  },

  kickBackbonePeer: async (nodeId) => {
    const state = get();
    const peer = state.backbonePeers.find((candidate) => candidate.node_id === nodeId);
    if (!peer) return;
    set({ backbonePeers: state.backbonePeers.filter((candidate) => candidate.node_id !== nodeId) });
    try {
      if (state.mode === 'demo') {
        transport?.send('backbone_kick', { node_id: nodeId });
      } else {
        await backboneRestClient(state.settings).kickPeer(nodeId);
      }
      set({ backboneLastUpdated: Date.now() });
      get().pushToast({ kind: 'info', title: 'Peer disconnected', message: peer.name || nodeId });
    } catch (error) {
      set({ backbonePeers: state.backbonePeers, backboneError: errorMessage(error) });
      get().pushToast({ kind: 'error', title: 'Could not kick peer', message: errorMessage(error) });
    }
  },

  kickAllBackbonePeers: async () => {
    const state = get();
    const peers = [...state.backbonePeers];
    if (peers.length === 0) return 0;
    set({ backbonePeers: [] });
    if (state.mode === 'demo') {
      for (const peer of peers) transport?.send('backbone_kick', { node_id: peer.node_id });
    } else {
      const client = backboneRestClient(state.settings);
      await Promise.allSettled(peers.map((peer) => client.kickPeer(peer.node_id)));
    }
    set({ backboneLastUpdated: Date.now() });
    return peers.length;
  },

  rotateBackboneToken: async () => {
    const state = get();
    try {
      let token: string;
      if (state.mode === 'demo') {
        if (!transport) throw new Error('Demo transport is not connected');
        const commandId = transport.send('backbone_rotate_token', {});
        const result = await registerCommand<{ token: string }>(commandId, 6000);
        token = result.token;
      } else {
        token = await backboneRestClient(state.settings).rotateToken();
      }
      set({
        backboneSessionToken: token,
        backboneTokenFresh: true,
        backboneConfig: state.backboneConfig ? { ...state.backboneConfig, token_set: true, auth: 'token' } : state.backboneConfig,
        backboneLastUpdated: Date.now(),
      });
      get().pushToast({
        kind: 'warn',
        title: 'Backbone token rotated',
        message: 'The old token is invalid immediately. Show the new token once, then update your gateways.',
      });
      return token;
    } catch (error) {
      get().pushToast({ kind: 'error', title: 'Token rotation failed', message: errorMessage(error) });
      throw error;
    }
  },

  loadBackboneSetup: async () => {
    const state = get();
    const connectionString = connectionStringFromState(state);
    const fallback = buildFallbackSetup({ config: state.backboneConfig, connectionString });
    if (state.mode === 'demo') {
      const setup = normalizeBackboneSetup(state.backboneSetup ?? fallback, fallback);
      set({ backboneSetup: setup });
      return setup;
    }
    try {
      const setup = normalizeBackboneSetup(await backboneRestClient(state.settings).getSetup(), fallback);
      set({ backboneSetup: setup });
      return setup;
    } catch {
      // Setup endpoint may be unavailable on older/disabled bridges — the local template is honest and usable.
      set({ backboneSetup: fallback });
      return fallback;
    }
  },

  /* -------------------- toasts + errors -------------------- */

  pushToast: (toast) => {
    const id = localId('toast');
    set({ toasts: [...get().toasts, { ...toast, id }].slice(-4) });
    setTimeout(() => {
      set({ toasts: get().toasts.filter((t) => t.id !== id) });
    }, toast.kind === 'error' ? 8_000 : 5_000);
  },

  dismissToast: (id) => set({ toasts: get().toasts.filter((t) => t.id !== id) }),

  /* -------------------- bridge events -------------------- */

  handleEvent: (event) => {
    const state = get();
    switch (event.type) {
      case 'hello': {
        const data = event.data as HelloData;
        set({
          hello: data,
          bridgeVersion: data.bridge_version,
          ownNodeId: data.node.node_id,
          ownName: data.node.name,
          syncing: false,
        });
        break;
      }
      case 'status':
        set({ bridgeStatus: event.data as StatusData });
        break;
      case 'node':
        set({ nodes: upsertNode(state.nodes, event.data as NodeData) });
        break;
      case 'message': {
        const data = event.data as MessageData;
        const optimistic = findOptimisticDirect(state.messages, data);
        const messages = upsertMessage(state.messages, data, optimistic?.tempId);
        let next = {
          messages: messages.sort((a, b) => a.sent_at_ms - b.sent_at_ms),
          groupMessages: state.groupMessages,
          readCursors: state.readCursors,
        };
        if (!data.outgoing) {
          const open = state.activeThread;
          if (open?.kind === 'direct' && open.id === data.peer) {
            next = { ...next, messages: next.messages.map((m) => (m.msg_id === data.msg_id ? { ...m, read: true } : m)) };
            transport?.send('mark_read', { msg_id: data.msg_id });
          }
        }
        set(next);
        break;
      }
      case 'message_update': {
        const data = event.data as { msg_id: number } & Record<string, unknown>;
        const messages = applyMessageUpdate(state.messages, data as MessageUpdateData);
        set({ messages });
        break;
      }
      case 'group_message': {
        const data = event.data as GroupMessageData;
        const optimistic = findOptimisticGroup(state.groupMessages, data);
        const groupMessages = upsertGroupMessage(state.groupMessages, data, optimistic?.tempId);
        const openGroup = state.activeThread?.kind === 'group' && state.activeThread.id === data.channel_id;
        set({
          groupMessages: groupMessages.sort((a, b) => a.sent_at_ms - b.sent_at_ms),
          ...(openGroup && !data.outgoing
            ? { readCursors: { ...state.readCursors, [data.channel_id]: Date.now() } }
            : {}),
        });
        break;
      }
      case 'ack': {
        const data = event.data as AckData;
        const retryTimer = pendingRetries.get(data.msg_id);
        if (retryTimer) {
          clearTimeout(retryTimer);
          pendingRetries.delete(data.msg_id);
        }
        set({ messages: applyAck(state.messages, data.msg_id, data.kind) });
        break;
      }
      case 'position': {
        const data = event.data as PositionEventData;
        const nodes = state.nodes[data.node_id]
          ? {
              ...state.nodes,
              [data.node_id]: {
                ...state.nodes[data.node_id]!,
                position: data.position,
                last_seen_ms: data.position.timestamp_ms,
              },
            }
          : state.nodes;
        set({
          nodes,
          positionHistory: appendPositionHistory(state.positionHistory, data.node_id, data.position),
          ...(data.node_id === state.ownNodeId
            ? {
                ownPosition: data.position,
                ownAccuracyM: data.position.accuracy_m ?? null,
              }
            : {}),
        });
        break;
      }
      case 'telemetry': {
        const data = event.data as { node_id: string; battery_pct?: number | null; timestamp_ms: number };
        const node = state.nodes[data.node_id];
        if (node) {
          set({
            nodes: {
              ...state.nodes,
              [data.node_id]: {
                ...node,
                battery_pct: data.battery_pct ?? node.battery_pct,
                last_seen_ms: data.timestamp_ms,
              },
            },
          });
        }
        break;
      }
      case 'store_offer': {
        const data = event.data as StoreOfferData;
        set({ storeOffer: { ...data, at_ms: Date.now() } });
        break;
      }
      case 'resource_progress': {
        const data = event.data as ResourceProgressData;
        const progress = data.total > 0 ? Math.min(1, data.received / data.total) : 0;
        set({
          messages: state.messages.map((m) =>
            m.msg_id === data.msg_id && m.attachment
              ? { ...m, attachment: { ...m.attachment, progress } }
              : m,
          ),
        });
        break;
      }
      case 'backbone_status': {
        const data = event.data as BackboneStatusEvent;
        // The bridge may piggyback the redacted config on responses (get_backbone,
        // set_backbone_config) — BACKBONE.md §7.2 / runtime contract.
        const pushedConfig = data.config ? normalizeBackboneConfig(data.config) : null;
        const configNow = pushedConfig ?? state.backboneConfig;
        set({
          backboneStatus: {
            running: data.running,
            peers: data.peers,
            uptime_s: data.uptime_s,
            frames_in: data.frames_in,
            frames_out: data.frames_out,
            bytes_in: data.bytes_in,
            bytes_out: data.bytes_out,
            throughput_in_bps: data.throughput_in_bps,
            throughput_out_bps: data.throughput_out_bps,
            store_forward_envelopes: data.store_forward_envelopes,
            auth_failures: data.auth_failures,
            last_error: data.last_error ?? null,
          },
          backboneLoading: false,
          backboneLastUpdated: Date.now(),
          ...(configNow && data.running && !configNow.enabled
            ? { backboneConfig: { ...configNow, enabled: true, running: true } }
            : configNow
              ? { backboneConfig: { ...configNow, running: data.running } }
              : {}),
        });
        break;
      }
      case 'backbone_peer': {
        const data = event.data as BackbonePeerEvent;
        set({
          backbonePeers: applyPeerEvent(state.backbonePeers, data),
          backboneLastUpdated: Date.now(),
        });
        break;
      }
      case 'backbone_token': {
        const data = event.data as { token?: string };
        if (data?.token) set({ backboneSessionToken: data.token, backboneLastUpdated: Date.now() });
        if (event.id) resolveCommand(event.id, data);
        break;
      }
      case 'config': {
        const data = event.data as ConfigData;
        const channels: Record<string, Channel> = {};
        for (const channel of data.channels ?? []) {
          channels[channel.channel_id] = { ...state.channels[channel.channel_id], ...channel };
        }
        // Keep known channels that the snapshot does not include (e.g. joined but not in the radio config yet).
        for (const [id, channel] of Object.entries(state.channels)) {
          if (!channels[id]) channels[id] = channel;
        }
        set({
          config: data,
          channels,
          ...(data.node?.name ? { ownName: data.node.name } : {}),
          ...(data.backbone ? { backboneConfig: data.backbone } : {}),
        });
        break;
      }
      case 'sync_done': {
        set({ syncing: false });
        // Backbone events are pushed while connected; only ask when advertised (§3.12).
        const features = get().hello?.features ?? [];
        if (get().mode === 'demo' || features.includes('backbone')) {
          transport?.send('get_backbone', {});
        }
        break;
      }
      case 'nodes': {
        const data = event.data as { nodes: NodeData[] };
        let nodes = state.nodes;
        for (const node of data.nodes ?? []) nodes = upsertNode(nodes, node);
        set({ nodes });
        break;
      }
      case 'messages': {
        const data = event.data as { messages: MessageData[] };
        set({ messages: upsertAllMessages(state.messages, data.messages ?? []) });
        break;
      }
      case 'contacts': {
        const data = event.data as ContactsEventData;
        if (event.id) resolveCommand(event.id, data);
        const contacts: Record<string, Contact> = {};
        for (const contact of data.contacts ?? []) {
          contacts[contact.node_id] = { ...contact, ...(state.contacts[contact.node_id]?.seeded ? { seeded: true } : {}) };
        }
        set({ contacts });
        break;
      }
      case 'channels': {
        const data = event.data as ChannelsEventData;
        if (event.id) resolveCommand(event.id, data);
        const channels: Record<string, Channel> = {};
        for (const channel of data.channels ?? []) {
          channels[channel.channel_id] = { ...channel, ...(state.channels[channel.channel_id]?.seeded ? { seeded: true } : {}) };
        }
        set({ channels });
        break;
      }
      case 'channel_created': {
        const data = event.data as ChannelCreatedData;
        applyChannelCreated(set, get, data);
        if (event.id) resolveCommand(event.id, data);
        break;
      }
      case 'channel_url': {
        const data = event.data as { url: string };
        set({ lastInvite: { kind: 'channel', url: data.url } });
        if (event.id) resolveCommand(event.id, data);
        break;
      }
      case 'contact_url': {
        const data = event.data as { url: string };
        set({ lastInvite: { kind: 'contact', url: data.url } });
        if (event.id) resolveCommand(event.id, data);
        break;
      }
      case 'send_result': {
        const data = event.data as { msg_id: number; est_airtime_ms: number; queued: boolean };
        const pending = event.id ? pendingSends.get(event.id) : undefined;
        if (event.id && pending) {
          pendingSends.delete(event.id);
          if (pending.kind === 'direct') {
            set({
              messages: state.messages
                .map((m) =>
                  m.tempId === pending.tempId
                    ? { ...m, msg_id: data.msg_id, delivery: 'sent' as const, tempId: undefined }
                    : m,
                )
                .sort((a, b) => a.sent_at_ms - b.sent_at_ms),
            });
          } else {
            set({
              groupMessages: state.groupMessages
                .map((m) => (m.tempId === pending.tempId ? { ...m, msg_id: data.msg_id, tempId: undefined } : m))
                .sort((a, b) => a.sent_at_ms - b.sent_at_ms),
            });
          }
        }
        break;
      }
      case 'error': {
        const data = event.data as ErrorData;
        set({ commandError: data.message });
        if (event.id) {
          rejectCommand(event.id, new Error(data.message));
          const pending = pendingSends.get(event.id);
          if (pending) {
            pendingSends.delete(event.id);
            set({
              messages: state.messages.map((m) =>
                m.tempId === pending.tempId ? { ...m, delivery: 'failed', error: data.message } : m,
              ),
            });
          }
        }
        get().pushToast({ kind: 'error', title: 'Bridge error', message: data.message });
        break;
      }
      default:
        // Unknown event types MUST be ignored (WEBSOCKET-API §7).
        break;
    }
  },
}));

function applyChannelCreated(set: SetState, get: GetState, data: ChannelCreatedData): void {
  const state = get();
  const channel: Channel = {
    channel_id: data.channel_id,
    name: data.name,
    psk_set: true,
  };
  set({
    channels: upsertChannel(state.channels, channel),
    config: state.config
      ? {
          ...state.config,
          channels: state.config.channels.some((c) => c.channel_id === data.channel_id)
            ? state.config.channels
            : [...state.config.channels, { channel_id: data.channel_id, name: data.name, psk_set: true }],
        }
      : state.config,
  });
}

/* Convenience selector helpers used across screens. */

export function activeThreadMessages(state: Pick<AppState, 'activeThread' | 'messages' | 'groupMessages'>): {
  direct: ChatMessage[];
  group: ChatGroupMessage[];
} {
  const thread = state.activeThread;
  if (!thread) return { direct: [], group: [] };
  if (thread.kind === 'direct') {
    return { direct: state.messages.filter((m) => m.peer === thread.id), group: [] };
  }
  return { direct: [], group: state.groupMessages.filter((m) => m.channel_id === thread.id) };
}

export function nodeById(state: Pick<AppState, 'nodes'>, nodeId: string | null): NodeData | null {
  if (!nodeId) return null;
  return state.nodes[nodeId] ?? null;
}

export const DEFAULT_CONFIG_FALLBACK: ConfigData = {
  node: { name: 'openmesh-node', class: 'client', announce_interval_s: 1800 },
  radio: {
    profile: DEFAULT_PROFILE_ID,
    region: DEFAULT_REGION_ID,
    tx_power_dbm: 14,
    frequency_mhz: 869.525,
    interface: 'sim',
  },
  features: { auto_announce: true, store_forward: true, position_sharing: true },
  channels: [],
};
