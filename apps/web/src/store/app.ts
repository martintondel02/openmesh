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
  CancelResultData,
  DeliveryState,
  MessageCancelledData,
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
import type { SetupRecommendation, SetupRole, SetupState, QuickSetupRequest, QuickSetupResult, OnboardingState, OnboardingDraft } from '../types/setup';
import type { ActivityEntry } from '../types/agents';
import { type UiMode, DEFAULT_UI_MODE, loadUiMode, persistUiMode, resolveUiModeOverride } from '../lib/mode';
import {
  buildDemoQuickSetupResult,
  createOnboardingState,
  normalizeQuickSetupRequest,
  onboardingReducer,
  recommendedFor,
  suggestNodeName,
} from '../lib/setup';
import { SetupRestClient } from '../lib/setup-api';
import { AgentsRestClient } from '../lib/agents-api';
import { OpsApiError, OpsRestClient } from '../lib/ops-api';
import type { BackboneLink, BackboneLinkInput, GatewayConfigInput, GatewayConnector, GatewayTestResult, McpHttpStatus } from '../types/ops';
import { GATEWAYS_HELP_TOPIC } from '../lib/ops';
import { normalizeActivityPayload } from '../lib/agents';
import { parseHelpRoute } from '../lib/help';
import { cancelAvailability } from '../lib/message-cancel';
import type { RegulatoryOverride } from '../types/regulatory';
import {
  buildRegulatoryOverridePatch,
  defaultRegulatoryOverride,
  deriveRegulatoryState,
  legalLimitsForRegion,
  mergeRegulatoryConfig,
  OVERRIDE_WARNING,
} from '../lib/regulatory';
import { ConfigApiError, patchBridgeConfig } from '../lib/config-api';
import type { ConfigPatch } from './reducers';
import { deriveDelivery, mergeConfig, removeContact as removeContactReducer, upsertChannel, upsertGroupMessage, upsertMessage, upsertNode, applyAck, applyMessageUpdate, applyGroupMessageUpdate, applyResourceProgress, markMediaActivity, markCancelRequested, clearCancelRequested, settleCancelledMessage, resourceProgressFraction, appendPositionHistory } from './reducers';
import type { MeshTransport, TransportStatus } from '../lib/transport';
import { LiveTransport, buildHttpBaseUrl } from '../lib/ws';
import { MockTransport } from '../lib/mock/transport';
import { createDemoDataset } from '../lib/mock/data';
import type { DemoDataset } from '../lib/mock/data';
import { localId } from '../lib/utils';
import { clearClientLocalState, clientResetUrl } from '../lib/client-reset';
import { formatBridgeResetCounts, requestBridgeReset, type BridgeResetResult } from '../lib/bridge-admin';
import {
  PUBLIC_CHANNEL_ID,
  PUBLIC_CHANNEL_NAME,
  PUBLIC_CHANNEL_PSK,
  findPublicChannel,
  isPublicChannel,
  publicChannelLeaveBlocked,
  shouldAutoSelectPublicChannel,
} from '../lib/public-channel';
import { DEFAULT_PROFILE_ID, DEFAULT_REGION_ID } from '../lib/airtime';

export type DemoReason = 'forced' | 'user' | 'offline' | 'none';

export interface Toast {
  id: string;
  kind: 'info' | 'success' | 'error' | 'warn';
  title: string;
  message?: string;
  /** Help Center topic shown as an actionable "Get help" link. */
  helpTopic?: string;
  /** Optional redacted context for the copy-diagnostics button. */
  diagnostics?: Record<string, unknown>;
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

const SCREEN_NAMES: Screen[] = ['chats', 'map', 'nodes', 'contacts', 'backbone', 'agents', 'help', 'settings'];

/** Allow deep links/screenshots: ?screen=map&demo=1, /help/slug, /agents. */
function initialScreenFromUrl(): Screen {
  try {
    if (/\/backbone\/?$/.test(window.location.pathname)) return 'backbone';
    if (/\/agents\/?$/.test(window.location.pathname)) return 'agents';
    if (parseHelpRoute(window.location.pathname, window.location.search, window.location.hash)) return 'help';
    const requested = new URLSearchParams(window.location.search).get('screen');
    if (requested && (SCREEN_NAMES as string[]).includes(requested)) return requested as Screen;
  } catch {
    // ignore
  }
  return 'chats';
}

function initialHelpRoute(): { slug: string | null; anchor: string | null } {
  try {
    const route = parseHelpRoute(window.location.pathname, window.location.search, window.location.hash);
    if (route) return route;
  } catch {
    // ignore
  }
  return { slug: null, anchor: null };
}

function initialUiMode(): UiMode {
  try {
    const override = resolveUiModeOverride(window.location.search);
    if (override) return override;
    return loadUiMode(window.localStorage);
  } catch {
    return DEFAULT_UI_MODE;
  }
}

function forceSetupCardFromUrl(): boolean {
  try {
    return new URLSearchParams(window.location.search).get('setup') === 'card';
  } catch {
    return false;
  }
}

function demoRegulatoryPreviewFromUrl(): boolean {
  try {
    return new URLSearchParams(window.location.search).get('regulatory') === 'active';
  } catch {
    return false;
  }
}

function wantsOnboardingFromUrl(): boolean {
  try {
    const value = new URLSearchParams(window.location.search).get('onboarding');
    return value === '1' || value === 'true';
  } catch {
    return false;
  }
}

const CHECKLIST_STORAGE_KEY = 'openmesh.checklist.v1';
function loadChecklistFlags(): Record<string, boolean> {
  try {
    const raw = window.localStorage.getItem(CHECKLIST_STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    return parsed && typeof parsed === 'object' ? (parsed as Record<string, boolean>) : {};
  } catch {
    return {};
  }
}
function persistChecklistFlags(flags: Record<string, boolean>): void {
  try {
    window.localStorage.setItem(CHECKLIST_STORAGE_KEY, JSON.stringify(flags));
  } catch {
    // ignore
  }
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
  /** Whether this bridge supports cancelling sends: unknown until first attempt. */
  cancelSupport: 'unknown' | 'supported' | 'unavailable';
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

  /* beginner/advanced mode + help (v0.3) */
  uiMode: UiMode;
  helpSlug: string | null;
  helpAnchor: string | null;
  setupState: SetupState | null;
  setupRecommendation: SetupRecommendation | null;
  setupLoading: boolean;
  setupError: string | null;
  onboarding: OnboardingState;
  setupCardDismissed: boolean;
  forceSetupCard: boolean;
  advancedChecklistDone: Record<string, boolean>;
  composerDrafts: Record<string, string>;

  /* AI agents (v0.3) */
  agentActivities: ActivityEntry[];
  agentConnected: boolean;
  lastAgentAtMs: number | null;
  activityCapacity: number;
  activitySources: string[];
  agentLoading: boolean;
  agentError: string | null;

  /* remote operations (v0.7): gateways, remote MCP guidance, multi-link backbone */
  gateways: GatewayConnector[] | null;
  gatewaysLoading: boolean;
  gatewaysError: string | null;
  /** True when GET /api/gateways 404s (older bridge): show guidance instead of forms. */
  gatewaysUnsupported: boolean;
  gatewaySaving: string | null;
  gatewayTesting: string | null;
  backboneLinks: BackboneLink[] | null;
  backboneLinksLoading: boolean;
  backboneLinksError: string | null;
  backboneLinksUnsupported: boolean;
  backboneLinkSaving: string | null;
  /** Remote MCP HTTP status when exposed/seeded; null means render file-config guidance only. */
  mcpStatus: McpHttpStatus | null;

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
  cancelMessage: (msgId: number) => Promise<void>;
  markActiveThreadRead: () => void;
  sharePosition: (target: { channelId?: string; peer?: string }) => Promise<void>;
  updateConfig: (patch: ConfigPatch) => void;
  announceNow: (options?: { silent?: boolean }) => void;
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
  /** Clear this browser's local state (token/settings/drafts/demo flags/read markers) and reload. */
  resetClient: () => Promise<void>;
  /** Authenticated bridge wipe + identity regeneration (task-40); live mode only. */
  resetBridgeData: (options?: { newIdentity?: boolean }) => Promise<BridgeResetResult>;
  setOwnPositionFromGeo: (position: Position, accuracyM?: number | null) => void;
  loadBackbone: () => Promise<void>;

  /* v0.3 mode / onboarding / help / agents */
  setUiMode: (mode: UiMode) => void;
  openHelp: (topic?: string | null, anchor?: string | null) => void;
  setHelpTopic: (slug: string | null, anchor?: string | null) => void;
  loadSetupState: () => Promise<void>;
  openOnboarding: () => void;
  closeOnboarding: () => void;
  onboardingNext: () => void;
  onboardingBack: () => void;
  updateOnboardingDraft: (patch: Partial<OnboardingDraft>) => void;
  submitQuickSetup: (overrides?: Partial<QuickSetupRequest>) => Promise<void>;
  loadSetupRecommendation: (country?: string, role?: SetupRole) => Promise<void>;
  dismissSetupCard: () => void;
  toggleAdvancedChecklist: (key: string) => void;
  setComposerDraft: (key: string, text: string) => void;
  clearComposerDraft: (key: string) => void;
  loadActivity: () => Promise<void>;
  revokeApiToken: () => Promise<string | null>;

  /* v0.5 legal maxima + emergency regulatory override */
  saveRegulatoryOverride: (
    override: RegulatoryOverride,
  ) => Promise<{ ok: boolean; message?: string; warning?: string }>;
  disableRegulatoryOverride: () => Promise<boolean>;
  setDemoOverrideActive: (active: boolean) => void;
  saveBackboneConfig: (patch: BackboneConfigPatch) => Promise<boolean>;
  setBackboneEnabled: (enabled: boolean) => Promise<boolean>;
  kickBackbonePeer: (nodeId: string) => Promise<void>;
  kickAllBackbonePeers: () => Promise<number>;
  rotateBackboneToken: () => Promise<string>;
  loadBackboneSetup: () => Promise<BackboneSetup | null>;
  loadGateways: () => Promise<void>;
  saveGateway: (input: GatewayConfigInput) => Promise<boolean>;
  setGatewayEnabled: (id: string, enabled: boolean) => Promise<boolean>;
  deleteGateway: (id: string) => Promise<boolean>;
  testGateway: (id: string) => Promise<GatewayTestResult | null>;
  loadBackboneLinks: () => Promise<void>;
  saveBackboneLink: (input: BackboneLinkInput) => Promise<boolean>;
  setBackboneLinkEnabled: (name: string, enabled: boolean) => Promise<boolean>;
  deleteBackboneLink: (name: string) => Promise<boolean>;
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

let demoDatasetRef: DemoDataset | null = null;
let setupAutoOpenAttempted = false;

function setupRestClient(settings: AppSettings): SetupRestClient {
  return new SetupRestClient({
    baseUrl: settings.bridgeUrl,
    token: settings.token || null,
    timeoutMs: 8000,
  });
}

function agentsRestClient(settings: AppSettings): AgentsRestClient {
  return new AgentsRestClient({
    baseUrl: settings.bridgeUrl,
    token: settings.token || null,
    timeoutMs: 8000,
  });
}

function opsRestClient(settings: AppSettings): OpsRestClient {
  return new OpsRestClient({
    baseUrl: settings.bridgeUrl,
    token: settings.token || null,
    timeoutMs: 8000,
  });
}

function demoChannelInvite(channel: { channel_id: string; name: string } | undefined): string {
  const id = channel?.channel_id ?? 'demo-channel';
  const name = channel?.name ?? 'Private channel';
  return `openmesh://channel?channel=${encodeURIComponent(id)}&name=${encodeURIComponent(name)}&psk=demo-channel-key-not-a-real-secret&v=1`;
}

function syncUiModeToBridge(mode: UiMode, config: ConfigData | null): void {
  if (!transport || !config) return;
  try {
    transport.send('set_config', { features: { ...config.features, ui_mode: mode } });
  } catch {
    // Best effort: the local preference still applies to layout.
  }
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

function syncScreenUrl(screen: Screen, helpSlug: string | null = null, helpAnchor: string | null = null): void {
  if (typeof window === 'undefined' || !window.history?.pushState) return;
  try {
    const url = new URL(window.location.href);
    const previous = url.toString();
    if (screen === 'backbone') {
      url.pathname = `${url.pathname.replace(/\/(backbone|agents|help(?:\/[A-Za-z0-9-]+)?)\/?$/, '')}/backbone`.replace(/\/+/g, '/');
      url.searchParams.delete('screen');
      url.hash = '';
    } else if (screen === 'agents') {
      url.pathname = `${url.pathname.replace(/\/(backbone|agents|help(?:\/[A-Za-z0-9-]+)?)\/?$/, '')}/agents`.replace(/\/+/g, '/');
      url.searchParams.delete('screen');
      url.hash = '';
    } else if (screen === 'help') {
      const base = url.pathname.replace(/\/(backbone|agents|help(?:\/[A-Za-z0-9-]+)?)\/?$/, '');
      url.pathname = `/help${helpSlug ? `/${encodeURIComponent(helpSlug)}` : ''}`.replace(/\/+/g, '/');
      if (base && base !== '/') url.pathname = `${base.replace(/\/$/, '')}${url.pathname}`.replace(/\/+/g, '/');
      url.searchParams.delete('screen');
      url.hash = helpAnchor ? `#${helpAnchor}` : '';
    } else {
      url.pathname = url.pathname.replace(/\/(backbone|agents|help(?:\/[A-Za-z0-9-]+)?)\/?$/, '/');
      url.searchParams.delete('screen');
      url.hash = '';
    }
    if (url.toString() !== previous) window.history.pushState({ screen, helpSlug }, '', url.toString());
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

let publicJoinAttempted = false;

/** True after the user/browser asked for a specific thread via `?thread=` (deep link wins). */
function hasThreadDeepLink(): boolean {
  try {
    return Boolean(new URLSearchParams(window.location.search).get('thread'));
  } catch {
    return false;
  }
}

/** Legacy-bridge fallback: join the built-in Public Mesh when the channels snapshot omits it. */
function maybeAutoJoinPublicChannel(get: GetState): void {
  const state = get();
  if (publicJoinAttempted || state.mode !== 'live' || !transport) return;
  if (Object.values(state.channels).some((channel) => isPublicChannel(channel))) return;
  // Raw send rather than the UI action: join_channel replies with a `channels` snapshot on the
  // task-40 bridge, so we must not treat that payload as a `channel_created` result.
  publicJoinAttempted = true;
  try {
    transport.send('join_channel', { name: PUBLIC_CHANNEL_NAME, psk: PUBLIC_CHANNEL_PSK });
  } catch {
    // Older/offline bridge: the next startup sync or a manual join can still surface it.
  }
}

/** On a fresh/empty live client, open the pinned Public Mesh channel automatically. */
function maybeSelectPublicChannel(set: SetState, get: GetState): void {
  const state = get();
  const publicChannel = findPublicChannel(state.channels);
  if (!publicChannel) return;
  if (
    !shouldAutoSelectPublicChannel({
      mode: state.mode,
      hasActiveThread: Boolean(state.activeThread),
      hasThreadDeepLink: hasThreadDeepLink(),
      messageCount: state.messages.length + state.groupMessages.length,
      channels: state.channels,
    })
  ) {
    return;
  }
  set({ activeThread: { kind: 'group', id: publicChannel.channel_id } });
}

function attachTransport(set: SetState, get: GetState, next: MeshTransport): void {
  transport?.dispose();
  transport = next;
  publicJoinAttempted = false;
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

type CancelOutcome = 'cancelled' | 'already_finished' | 'unavailable';

/**
 * REST fallback for cancel (task-30 contract: POST /api/messages/{id}/cancel).
 *
 * - 2xx + `cancelled: true` → cancelled (idempotent repeats included)
 * - 400 (inbound / already delivered) and 404 (unknown id) → already finished
 * - anything else (no route, auth, network) → unavailable
 */
async function cancelMessageViaRest(settings: AppSettings, msgId: number): Promise<CancelOutcome> {
  try {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 6000);
    const response = await fetch(`${buildHttpBaseUrl(settings.bridgeUrl)}/api/messages/${msgId}/cancel`, {
      method: 'POST',
      signal: controller.signal,
      headers: {
        Accept: 'application/json',
        'X-OpenMesh-Client': 'web',
        ...(settings.token ? { Authorization: `Bearer ${settings.token}` } : {}),
      },
    });
    clearTimeout(timer);
    if (response.ok) {
      const data = (await response.json().catch(() => null)) as CancelResultData | null;
      return data && data.cancelled ? 'cancelled' : 'unavailable';
    }
    if (response.status === 400 || response.status === 404) return 'already_finished';
    return 'unavailable';
  } catch {
    return 'unavailable';
  }
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
  cancelSupport: 'unknown',

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

  uiMode: initialUiMode(),
  helpSlug: initialHelpRoute().slug,
  helpAnchor: initialHelpRoute().anchor,
  setupState: null,
  setupRecommendation: null,
  setupLoading: false,
  setupError: null,
  onboarding: createOnboardingState(),
  setupCardDismissed: false,
  forceSetupCard: forceSetupCardFromUrl(),
  advancedChecklistDone: loadChecklistFlags(),
  composerDrafts: {},
  agentActivities: [],
  agentConnected: false,
  lastAgentAtMs: null,
  activityCapacity: 256,
  activitySources: [],
  agentLoading: false,
  agentError: null,
  gateways: null,
  gatewaysLoading: false,
  gatewaysError: null,
  gatewaysUnsupported: false,
  gatewaySaving: null,
  gatewayTesting: null,
  backboneLinks: null,
  backboneLinksLoading: false,
  backboneLinksError: null,
  backboneLinksUnsupported: false,
  backboneLinkSaving: null,
  mcpStatus: null,

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
    if (wantsOnboardingFromUrl()) {
      get().openOnboarding();
    }
    if (demoRegulatoryPreviewFromUrl()) {
      // The mock transport re-emits its seeded config shortly after connect; activate
      // the demo-only preview after those snapshots so screenshots show the banner.
      setTimeout(() => {
        if (get().mode === 'demo') get().setDemoOverrideActive(true);
      }, 500);
    }
    if (!forced) {
      get().pushToast({
        kind: 'warn',
        title: 'Bridge not found — demo mode',
        message: 'Showing a seeded Oslo mesh. Start the bridge or set the URL in Settings, then retry.',
        helpTopic: 'troubleshooting',
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
    const state = get();
    if (status.state === 'connected' && state.mode === 'live' && !state.setupState && !state.setupLoading && !setupAutoOpenAttempted) {
      void state.loadSetupState();
    }
  },

  /* -------------------- demo hydration -------------------- */

  hydrateDemo: (dataset) => {
    demoDatasetRef = dataset;
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
      config: mergeRegulatoryConfig(null, dataset.config) ?? dataset.config,
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
      setupState: cloneValue(dataset.setup.state),
      setupRecommendation: cloneValue(dataset.setup.recommendation),
      setupLoading: false,
      setupError: null,
      onboarding: {
        ...createOnboardingState({ nodeName: suggestNodeName('client') }),
        open: false,
      },
      setupCardDismissed: false,
      forceSetupCard: forceSetupCardFromUrl(),
      advancedChecklistDone: loadChecklistFlags(),
      agentActivities: cloneValue(dataset.agents.activities),
      agentConnected: dataset.agents.configured,
      lastAgentAtMs: dataset.agents.activities[0]?.at_ms ?? null,
      activityCapacity: 256,
      activitySources: [...new Set(dataset.agents.activities.map((entry) => entry.source))],
      agentLoading: false,
      agentError: null,
      gateways: cloneValue(dataset.gateways ?? []),
      gatewaysLoading: false,
      gatewaysError: null,
      gatewaysUnsupported: false,
      gatewaySaving: null,
      gatewayTesting: null,
      backboneLinks: cloneValue(dataset.backboneLinks ?? []),
      backboneLinksLoading: false,
      backboneLinksError: null,
      backboneLinksUnsupported: false,
      backboneLinkSaving: null,
      mcpStatus: cloneValue(dataset.mcp ?? null),
      composerDrafts: {},
    });
  },

  /* -------------------- navigation -------------------- */

  setScreen: (screen) => {
    set({ screen, ...(screen === 'help' ? {} : { helpAnchor: null }) });
    const state = get();
    syncScreenUrl(screen, state.helpSlug, state.helpAnchor);
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
        mediaActivityAtMs: now,
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
        mediaActivityAtMs: now,
      };
      set({
        groupMessages: [...state.groupMessages, optimistic].sort((a, b) => a.sent_at_ms - b.sent_at_ms),
      });
    }

    const updateAttachmentProgress = (patch: Partial<typeof attachment>) => {
      const current = get();
      const atMs = Date.now();
      set({
        messages: current.messages.map((m) =>
          m.tempId === tempId
            ? { ...m, attachment: { ...attachment, ...m.attachment, ...patch }, mediaActivityAtMs: atMs }
            : m,
        ),
        groupMessages: current.groupMessages.map((m) =>
          m.tempId === tempId
            ? { ...m, attachment: { ...attachment, ...m.attachment, ...patch }, mediaActivityAtMs: atMs }
            : m,
        ),
      });
    };

    try {
      const uploaded = await transport.uploadAttachment(draft.blob, draft.filename);
      // The bridge has the bytes now; the radio transfer has not started yet, so the
      // bubble must not read 100 % until the bridge reports chunk progress/completion.
      updateAttachmentProgress({ id: uploaded.id, size: uploaded.size, sha256: uploaded.sha256, progress: 0 });
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
    if (target.cancelled || ('delivered' in target && target.delivered)) return;
    const atMs = Date.now();
    const isMedia = Boolean(target.attachment);
    const previousTimer = pendingRetries.get(msgId);
    if (previousTimer) {
      clearTimeout(previousTimer);
      pendingRetries.delete(msgId);
    }
    // Media retries restart the radio transfer from the copy already stored on the bridge:
    // reset the progress + stall clock but keep the same message row and id. Media stays
    // "sent to bridge" while it transfers; the radio progress line owns the state from here.
    set({
      messages: state.messages.map((m) =>
        m.msg_id === msgId
          ? {
              ...m,
              delivery: isMedia ? 'sent' : 'pending',
              error: undefined,
              mediaActivityAtMs: atMs,
              ...(m.attachment ? { attachment: { ...m.attachment, progress: 0 } } : {}),
            }
          : m,
      ),
      groupMessages: state.groupMessages.map((m) =>
        m.msg_id === msgId
          ? {
              ...m,
              error: undefined,
              mediaActivityAtMs: atMs,
              ...(m.attachment ? { attachment: { ...m.attachment, progress: 0 } } : {}),
            }
          : m,
      ),
    });
    transport.send('retry', { msg_id: msgId });
    if (isMedia) {
      // Media completion can legitimately take minutes at 10 % duty; the stall detector
      // watches radio progress instead of a fixed acknowledgement timer.
      get().pushToast({
        kind: 'info',
        title: 'Retrying upload',
        message: 'Asking the bridge to send the saved copy again — it can take minutes on the radio.',
      });
      return;
    }
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

  cancelMessage: async (msgId) => {
    const state = get();
    const target = state.messages.find((m) => m.msg_id === msgId) ?? state.groupMessages.find((m) => m.msg_id === msgId);
    if (!target || !target.outgoing || !transport) return;
    const group = 'channel_id' in target;
    const delivery: DeliveryState = group ? target.delivery ?? 'sent' : target.delivery;
    const availability = cancelAvailability({
      outgoing: true,
      msgId,
      delivery,
      cancelRequested: target.cancelRequested,
      error: target.error,
      group,
      sentAtMs: target.sent_at_ms,
      nowMs: Date.now(),
      hasAttachment: Boolean(target.attachment),
      attachmentProgress: target.attachment?.progress ?? null,
      bridgeSupportsCancel: state.cancelSupport === 'unavailable' ? false : null,
    });
    if (!availability.canCancel) return;

    set({
      messages: markCancelRequested(state.messages, msgId),
      groupMessages: markCancelRequested(state.groupMessages, msgId),
    });

    const isCancelledNow = () => {
      const current = get();
      const direct = current.messages.find((m) => m.msg_id === msgId);
      if (direct) return direct.delivery === 'cancelled';
      const channel = current.groupMessages.find((m) => m.msg_id === msgId);
      return channel?.delivery === 'cancelled';
    };

    const refreshRow = () => {
      const direct = get().messages.find((m) => m.msg_id === msgId);
      if (direct) {
        transport?.send('get_messages', { peer: direct.peer, limit: 20 });
        return;
      }
      const channel = get().groupMessages.find((m) => m.msg_id === msgId);
      if (channel) transport?.send('get_messages', { channel_id: channel.channel_id, limit: 20 });
    };

    let outcome: CancelOutcome = 'unavailable';
    try {
      const commandId = transport.send('cancel_message', { msg_id: msgId });
      const data = await registerCommand<CancelResultData>(commandId, 8000);
      outcome = data.cancelled || data.already_cancelled ? 'cancelled' : 'already_finished';
    } catch {
      outcome = isCancelledNow() ? 'cancelled' : 'unavailable';
    }
    if (outcome === 'unavailable') {
      // REST fallback for bridge builds that expose the endpoint but not the WS command.
      outcome = isCancelledNow() ? 'cancelled' : await cancelMessageViaRest(get().settings, msgId);
    }

    const current = get();
    if (outcome === 'cancelled') {
      set({
        cancelSupport: 'supported',
        messages: settleCancelledMessage(current.messages, msgId),
        groupMessages: settleCancelledMessage(current.groupMessages, msgId),
      });
      get().pushToast({
        kind: 'info',
        title: 'Send cancelled',
        message: 'No more radio airtime will be used for this message.',
      });
      return;
    }

    const after = get();
    set({
      messages: clearCancelRequested(after.messages, msgId),
      groupMessages: clearCancelRequested(after.groupMessages, msgId),
      ...(outcome === 'already_finished' ? { cancelSupport: 'supported' as const } : { cancelSupport: 'unavailable' as const }),
    });
    if (outcome === 'already_finished') {
      get().pushToast({
        kind: 'info',
        title: 'Already finished',
        message: 'This message was delivered or is no longer queued, so there was nothing to cancel.',
      });
      refreshRow();
      return;
    }
    get().pushToast({
      kind: 'warn',
      title: 'Cancel unavailable',
      message: 'This bridge does not support cancelling a send yet. Update the bridge to stop in-flight media.',
    });
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

  announceNow: (options) => {
    transport?.send('announce', {});
    if (options?.silent) return;
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
    const state = get();
    if (publicChannelLeaveBlocked(channelId, state.channels)) {
      get().pushToast({
        kind: 'warn',
        title: 'Public Mesh cannot be left',
        message: 'The built-in public channel is recreated by the bridge and is always available.',
      });
      return;
    }
    transport?.send('leave_channel', { channel_id: channelId });
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

  resetClient: async () => {
    try {
      transport?.dispose();
    } catch {
      // Best effort: the reload below drops the socket anyway.
    }
    transport = null;
    clearClientLocalState({ local: window.localStorage, session: window.sessionStorage });
    try {
      if (typeof caches !== 'undefined') {
        const keys = await caches.keys();
        await Promise.all(keys.map((key) => caches.delete(key)));
      }
    } catch {
      // Cache Storage is optional (Tauri/private mode); localStorage/sessionStorage cleared above.
    }
    window.location.replace(clientResetUrl(import.meta.env.BASE_URL || '/', window.location.href));
  },

  resetBridgeData: async (options) => {
    if (get().mode === 'demo') {
      throw new Error('Bridge reset is not available in demo mode. Turn demo mode off and connect to the bridge first.');
    }
    const result = await requestBridgeReset(get().settings, options);
    const channelId = result.public_channel?.channel_id ?? PUBLIC_CHANNEL_ID;
    const publicChannel: Channel = {
      channel_id: channelId,
      name: result.public_channel?.name ?? PUBLIC_CHANNEL_NAME,
      psk_set: true,
      public: true,
      ...(result.public_channel ?? {}),
    };
    const state = get();
    set({
      messages: [],
      groupMessages: [],
      contacts: {},
      nodes: {},
      channels: { [channelId]: publicChannel },
      readCursors: {},
      activeThread: { kind: 'group', id: channelId },
      lastCreatedChannel: null,
      lastInvite: null,
      selectedNodeId: null,
      ...(result.node_id ? { ownNodeId: result.node_id } : {}),
      ...(state.config
        ? {
            config: {
              ...state.config,
              channels: [{ channel_id: channelId, name: publicChannel.name, psk_set: true, public: true }],
            },
          }
        : {}),
    });
    get().pushToast({
      kind: 'success',
      title: result.identity_changed === false ? 'Bridge data cleared' : 'Bridge reset — new node identity',
      message: `${formatBridgeResetCounts(result.counts)} cleared. ${
        result.node_id ? `New node ${result.node_id.slice(0, 8)}… · ` : ''
      }Public Mesh was re-created; old contacts and QR codes are no longer valid.`,
    });
    return result;
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

  /* -------------------- mode, help, setup, agents (v0.3) -------------------- */

  setUiMode: (mode) => {
    if (get().uiMode === mode) return;
    try {
      persistUiMode(mode, window.localStorage);
    } catch {
      // ignore
    }
    set({ uiMode: mode });
    const state = get();
    if (state.connection === 'connected' && state.config) {
      syncUiModeToBridge(mode, state.config);
    }
  },

  openHelp: (topic = null, anchor = null) => {
    set({ screen: 'help', helpSlug: topic, helpAnchor: anchor });
    syncScreenUrl('help', topic, anchor);
  },

  setHelpTopic: (slug, anchor = null) => {
    set({ screen: 'help', helpSlug: slug, helpAnchor: anchor });
    syncScreenUrl('help', slug, anchor);
  },

  loadSetupState: async () => {
    const state = get();
    if (state.mode === 'demo') {
      if (demoDatasetRef) {
        set({
          setupState: cloneValue(demoDatasetRef.setup.state),
          setupRecommendation: cloneValue(demoDatasetRef.setup.recommendation),
          setupLoading: false,
          setupError: null,
        });
      }
      return;
    }
    if (state.connection !== 'connected' || state.setupLoading) return;
    set({ setupLoading: true, setupError: null });
    try {
      const next = await setupRestClient(state.settings).getState();
      set({ setupState: next, setupRecommendation: next.recommended, setupLoading: false });
      const now = get();
      if (
        now.uiMode === 'beginner' &&
        next.first_run &&
        !now.setupCardDismissed &&
        !now.onboarding.open &&
        !setupAutoOpenAttempted
      ) {
        setupAutoOpenAttempted = true;
        set({
          onboarding: {
            ...now.onboarding,
            open: true,
            step: 'role',
            draft: { ...now.onboarding.draft, nodeName: now.onboarding.draft.nodeName || suggestNodeName(now.onboarding.draft.role) },
          },
        });
      }
    } catch (error) {
      set({ setupLoading: false, setupError: errorMessage(error) });
    }
  },

  openOnboarding: () => {
    const state = get();
    set({
      onboarding: onboardingReducer(
        {
          ...state.onboarding,
          draft: {
            ...state.onboarding.draft,
            nodeName: state.onboarding.draft.nodeName || suggestNodeName(state.onboarding.draft.role, state.ownName),
          },
        },
        { type: 'open' },
      ),
    });
  },

  closeOnboarding: () => {
    set({ onboarding: onboardingReducer(get().onboarding, { type: 'close' }), setupCardDismissed: false });
  },

  onboardingNext: () => set({ onboarding: onboardingReducer(get().onboarding, { type: 'next' }) }),
  onboardingBack: () => set({ onboarding: onboardingReducer(get().onboarding, { type: 'back' }) }),

  updateOnboardingDraft: (patch) => {
    set({ onboarding: onboardingReducer(get().onboarding, { type: 'draft', patch }) });
  },

  submitQuickSetup: async (overrides = {}) => {
    const state = get();
    const draft = {
      role: overrides.role ?? state.onboarding.draft.role,
      country: overrides.country ?? state.onboarding.draft.country,
      nodeName: overrides.node_name ?? state.onboarding.draft.nodeName,
      privacy: overrides.privacy ?? state.onboarding.draft.privacy,
    };
    const request = normalizeQuickSetupRequest({
      role: draft.role,
      country: draft.country,
      node_name: draft.nodeName || suggestNodeName(draft.role),
      privacy: draft.privacy,
      create_channel: true,
    });
    const recommended = recommendedFor(request.country, request.role);
    set({ onboarding: onboardingReducer(state.onboarding, { type: 'submit-start' }) });
    try {
      let result: QuickSetupResult;
      if (state.mode === 'demo') {
        const dataset = demoDatasetRef;
        const channel =
          dataset?.channels[0] ??
          Object.values(state.channels)[0] ?? { channel_id: 'demo-channel', name: 'Private channel' };
        result = buildDemoQuickSetupResult({
          role: request.role,
          country: request.country,
          nodeName: request.node_name ?? suggestNodeName(request.role),
          privacy: request.privacy,
          channelId: channel.channel_id,
          channelName: channel.name,
          inviteUrl: demoChannelInvite(channel),
        });
        const config = state.config
          ? mergeConfig(state.config, {
              node: { ...state.config.node, name: request.node_name ?? state.config.node.name, class: request.role },
              radio: { ...state.config.radio, region: recommended.region, profile: recommended.profile },
              features: {
                ...state.config.features,
                position_sharing: request.privacy === 'shared',
                store_forward: recommended.store_forward ?? state.config.features.store_forward,
                ui_mode: state.uiMode,
              },
            })
          : state.config;
        const checks = { identity: true, radio: true, api_token: true, channel: true, backbone: true, agent: true };
        set({
          config,
          ownName: request.node_name ?? state.ownName,
          setupState: {
            first_run: false,
            checks,
            recommended,
            counts: state.setupState?.counts ?? {},
            ui_mode: state.uiMode,
          },
          setupRecommendation: recommended,
          onboarding: onboardingReducer(state.onboarding, { type: 'submit-success', result }),
          setupCardDismissed: true,
          setupError: null,
        });
      } else {
        result = await setupRestClient(state.settings).quickSetup(request);
        set({
          onboarding: onboardingReducer(get().onboarding, { type: 'submit-success', result }),
          setupCardDismissed: true,
          setupError: null,
        });
        void get().loadSetupState();
      }
      get().pushToast({
        kind: 'success',
        title: 'Your node is secured',
        message: 'A private channel is ready and safe defaults are on. You can change anything later.',
        helpTopic: 'getting-started',
      });
    } catch (error) {
      set({
        onboarding: onboardingReducer(get().onboarding, { type: 'submit-error', message: errorMessage(error) }),
      });
      get().pushToast({
        kind: 'error',
        title: 'Secure setup did not finish',
        message: errorMessage(error),
        helpTopic: 'troubleshooting',
        diagnostics: {
          step: 'setup/quick',
          bridge: state.settings.bridgeUrl,
          role: request.role,
          country: request.country,
        },
      });
    }
  },

  loadSetupRecommendation: async (country, role) => {
    const state = get();
    const targetCountry = country ?? state.onboarding.draft.country;
    const targetRole = role ?? state.onboarding.draft.role;
    const local = recommendedFor(targetCountry, targetRole);
    set({ setupRecommendation: local });
    if (state.mode !== 'live' || state.connection !== 'connected') return;
    try {
      const remote = await setupRestClient(state.settings).getRecommendation(targetCountry, targetRole);
      set({ setupRecommendation: remote });
    } catch {
      // Keep the local preset-derived recommendation; the wizard stays usable offline.
    }
  },

  dismissSetupCard: () => set({ setupCardDismissed: true }),

  toggleAdvancedChecklist: (key) => {
    const flags = { ...get().advancedChecklistDone, [key]: !get().advancedChecklistDone[key] };
    persistChecklistFlags(flags);
    set({ advancedChecklistDone: flags });
  },

  setComposerDraft: (key, text) => {
    const drafts = get().composerDrafts;
    if (drafts[key] === text) return;
    set({ composerDrafts: { ...drafts, [key]: text } });
  },

  clearComposerDraft: (key) => {
    const drafts = { ...get().composerDrafts };
    delete drafts[key];
    set({ composerDrafts: drafts });
  },

  loadActivity: async () => {
    const state = get();
    if (state.mode === 'demo') {
      if (demoDatasetRef) {
        set({
          agentActivities: cloneValue(demoDatasetRef.agents.activities),
          agentConnected: demoDatasetRef.agents.configured,
          lastAgentAtMs: demoDatasetRef.agents.activities[0]?.at_ms ?? null,
          activitySources: [...new Set(demoDatasetRef.agents.activities.map((entry) => entry.source))],
          agentError: null,
        });
      }
      return;
    }
    if (state.connection !== 'connected' || state.agentLoading) return;
    set({ agentLoading: true, agentError: null });
    try {
      const payload = normalizeActivityPayload(await agentsRestClient(state.settings).getActivityPayload(100));
      set({
        agentActivities: payload.entries,
        agentConnected: payload.agentConnected,
        lastAgentAtMs: payload.lastAgentAtMs,
        activityCapacity: payload.capacity,
        activitySources: payload.sources,
        agentLoading: false,
      });
    } catch (error) {
      set({ agentLoading: false, agentError: errorMessage(error) });
      get().pushToast({
        kind: 'error',
        title: 'Could not load agent activity',
        message: `${errorMessage(error)} Check the bridge URL and token, then retry.`,
        helpTopic: 'agents',
        diagnostics: { endpoint: '/api/activity', bridge: state.settings.bridgeUrl },
      });
    }
  },

  /* -------------------- remote operations: gateways + multi-link (v0.7) -------------------- */

  loadGateways: async () => {
    const state = get();
    if (state.mode === 'demo') {
      set({
        gateways: cloneValue(demoDatasetRef?.gateways ?? []),
        gatewaysLoading: false,
        gatewaysError: null,
        gatewaysUnsupported: false,
      });
      return;
    }
    if (state.connection !== 'connected' || state.gatewaysLoading) return;
    set({ gatewaysLoading: true, gatewaysError: null });
    try {
      const connectors = await opsRestClient(state.settings).listGateways();
      set({ gateways: connectors, gatewaysLoading: false, gatewaysUnsupported: false });
    } catch (error) {
      if (error instanceof OpsApiError && error.status === 404) {
        set({ gateways: [], gatewaysLoading: false, gatewaysUnsupported: true, gatewaysError: null });
        return;
      }
      set({ gatewaysLoading: false, gatewaysError: errorMessage(error) });
      get().pushToast({
        kind: 'error',
        title: 'Could not load gateways',
        message: `${errorMessage(error)} Check the bridge URL and token, then retry.`,
        helpTopic: GATEWAYS_HELP_TOPIC,
        diagnostics: { endpoint: '/api/gateways', bridge: state.settings.bridgeUrl },
      });
    }
  },

  saveGateway: async (input) => {
    const state = get();
    if (state.mode === 'demo') {
      const existing = (state.gateways ?? []).find((connector) => connector.id === input.id);
      const status = existing?.status ?? {
        last_success_ms: null,
        last_failure_ms: null,
        last_error: null,
        retries: 0,
        out_secret_present: Boolean(input.outbound.secret_env),
        in_secret_present: Boolean(input.inbound.secret_env),
      };
      const next: GatewayConnector = {
        ...input,
        status: {
          ...status,
          out_secret_present: Boolean(input.outbound.secret_env) || status.out_secret_present,
          in_secret_present: Boolean(input.inbound.secret_env) || status.in_secret_present,
        },
      };
      const list = [...(state.gateways ?? []).filter((connector) => connector.id !== input.id), next].sort((left, right) =>
        left.id.localeCompare(right.id),
      );
      set({ gateways: list, gatewaysError: null, gatewaysUnsupported: false });
      get().pushToast({
        kind: 'success',
        title: existing ? 'Demo gateway updated' : 'Demo gateway added',
        message: `${input.id} saved locally — demo mode never calls an adapter.`,
        helpTopic: GATEWAYS_HELP_TOPIC,
      });
      return true;
    }
    set({ gatewaySaving: input.id });
    try {
      await opsRestClient(state.settings).putGateway(input);
      set({ gatewaySaving: null, gatewaysUnsupported: false });
      await get().loadGateways();
      get().pushToast({
        kind: 'success',
        title: 'Gateway saved',
        message: `${input.id} is ${input.enabled ? 'enabled' : 'disabled'}; secrets stay on the bridge host.`,
        helpTopic: GATEWAYS_HELP_TOPIC,
      });
      return true;
    } catch (error) {
      set({ gatewaySaving: null });
      get().pushToast({
        kind: 'error',
        title: 'Could not save gateway',
        message: errorMessage(error),
        helpTopic: GATEWAYS_HELP_TOPIC,
        diagnostics: { endpoint: `/api/gateways/${input.id}`, bridge: state.settings.bridgeUrl },
      });
      return false;
    }
  },

  setGatewayEnabled: async (id, enabled) => {
    const state = get();
    const connector = (state.gateways ?? []).find((item) => item.id === id);
    if (!connector) return false;
    return get().saveGateway({
      id: connector.id,
      kind: connector.kind,
      enabled,
      outbound: connector.outbound,
      inbound: connector.inbound,
    });
  },

  deleteGateway: async (id) => {
    const state = get();
    if (state.mode === 'demo') {
      set({ gateways: (state.gateways ?? []).filter((connector) => connector.id !== id) });
      get().pushToast({ kind: 'success', title: 'Demo gateway removed', message: id, helpTopic: GATEWAYS_HELP_TOPIC });
      return true;
    }
    try {
      await opsRestClient(state.settings).deleteGateway(id);
      set({ gateways: (state.gateways ?? []).filter((connector) => connector.id !== id) });
      get().pushToast({ kind: 'success', title: 'Gateway removed', message: id, helpTopic: GATEWAYS_HELP_TOPIC });
      return true;
    } catch (error) {
      get().pushToast({
        kind: 'error',
        title: 'Could not remove gateway',
        message: errorMessage(error),
        helpTopic: GATEWAYS_HELP_TOPIC,
        diagnostics: { endpoint: `/api/gateways/${id}`, bridge: state.settings.bridgeUrl },
      });
      return false;
    }
  },

  testGateway: async (id) => {
    const state = get();
    if (state.mode === 'demo') {
      const connector = (state.gateways ?? []).find((item) => item.id === id);
      const failing = Boolean(connector?.status.last_error);
      const result: GatewayTestResult = {
        ok: !failing,
        status: failing ? 502 : 200,
        message: failing ? connector?.status.last_error ?? 'adapter unreachable' : 'signed test payload accepted',
        at_ms: Date.now(),
      };
      get().pushToast({
        kind: result.ok ? 'success' : 'error',
        title: result.ok ? 'Demo gateway test delivered' : 'Demo gateway test failed',
        message: result.message ?? undefined,
        helpTopic: GATEWAYS_HELP_TOPIC,
      });
      return result;
    }
    set({ gatewayTesting: id });
    try {
      const result = await opsRestClient(state.settings).testGateway(id);
      set({ gatewayTesting: null });
      get().pushToast({
        kind: result.ok ? 'success' : 'error',
        title: result.ok ? 'Gateway test delivered' : 'Gateway test failed',
        message: result.message ?? (result.status !== null ? `HTTP ${result.status}` : undefined),
        helpTopic: GATEWAYS_HELP_TOPIC,
        diagnostics: { endpoint: `/api/gateways/${id}/test`, bridge: state.settings.bridgeUrl },
      });
      await get().loadGateways();
      return result;
    } catch (error) {
      set({ gatewayTesting: null });
      get().pushToast({
        kind: 'error',
        title: 'Gateway test failed',
        message: errorMessage(error),
        helpTopic: GATEWAYS_HELP_TOPIC,
      });
      return null;
    }
  },

  loadBackboneLinks: async () => {
    const state = get();
    if (state.mode === 'demo') {
      set({
        backboneLinks: cloneValue(demoDatasetRef?.backboneLinks ?? []),
        backboneLinksLoading: false,
        backboneLinksError: null,
        backboneLinksUnsupported: false,
      });
      return;
    }
    if (state.connection !== 'connected' || state.backboneLinksLoading) return;
    set({ backboneLinksLoading: true, backboneLinksError: null });
    try {
      const links = await opsRestClient(state.settings).listBackboneLinks();
      set({ backboneLinks: links, backboneLinksLoading: false, backboneLinksUnsupported: false });
    } catch (error) {
      if (error instanceof OpsApiError && error.status === 404) {
        set({ backboneLinks: null, backboneLinksLoading: false, backboneLinksUnsupported: true, backboneLinksError: null });
        return;
      }
      set({ backboneLinksLoading: false, backboneLinksError: errorMessage(error) });
      get().pushToast({
        kind: 'error',
        title: 'Could not load backbone links',
        message: errorMessage(error),
        helpTopic: 'backbone',
        diagnostics: { endpoint: '/api/backbone/links', bridge: state.settings.bridgeUrl },
      });
    }
  },

  saveBackboneLink: async (input) => {
    const state = get();
    if (state.mode === 'demo') {
      const existing = (state.backboneLinks ?? []).find((link) => link.name === input.name);
      const next: BackboneLink = {
        name: input.name,
        url: input.url,
        mode: existing?.mode ?? 'client',
        enabled: input.enabled,
        connected: false,
        last_error: null,
        stats: existing?.stats ?? null,
      };
      set({
        backboneLinks: [...(state.backboneLinks ?? []).filter((link) => link.name !== input.name), next].sort((left, right) =>
          left.name.localeCompare(right.name),
        ),
        backboneLinksUnsupported: false,
      });
      get().pushToast({
        kind: 'success',
        title: existing ? 'Demo backbone link updated' : 'Demo backbone link added',
        message: `${input.name} saved locally — demo mode never dials a backbone.`,
        helpTopic: 'backbone',
      });
      return true;
    }
    set({ backboneLinkSaving: input.name });
    try {
      await opsRestClient(state.settings).putBackboneLink(input);
      set({ backboneLinkSaving: null, backboneLinksUnsupported: false });
      await get().loadBackboneLinks();
      get().pushToast({ kind: 'success', title: 'Backbone link saved', message: input.name, helpTopic: 'backbone' });
      return true;
    } catch (error) {
      set({ backboneLinkSaving: null });
      get().pushToast({
        kind: 'error',
        title: 'Could not save backbone link',
        message: errorMessage(error),
        helpTopic: 'backbone',
        diagnostics: { endpoint: `/api/backbone/links/${input.name}`, bridge: state.settings.bridgeUrl },
      });
      return false;
    }
  },

  setBackboneLinkEnabled: async (name, enabled) => {
    const state = get();
    const link = (state.backboneLinks ?? []).find((item) => item.name === name);
    if (!link) return false;
    if (!link.url) {
      get().pushToast({
        kind: 'warn',
        title: 'Link URL is redacted',
        message: 'This bridge did not return the invite URL, so the link can only be enabled from the CLI or the config file.',
        helpTopic: 'backbone',
      });
      return false;
    }
    return get().saveBackboneLink({ name: link.name, url: link.url, enabled });
  },

  deleteBackboneLink: async (name) => {
    const state = get();
    if (state.mode === 'demo') {
      set({ backboneLinks: (state.backboneLinks ?? []).filter((link) => link.name !== name) });
      get().pushToast({ kind: 'success', title: 'Demo backbone link removed', message: name, helpTopic: 'backbone' });
      return true;
    }
    try {
      await opsRestClient(state.settings).deleteBackboneLink(name);
      set({ backboneLinks: (state.backboneLinks ?? []).filter((link) => link.name !== name) });
      get().pushToast({ kind: 'success', title: 'Backbone link removed', message: name, helpTopic: 'backbone' });
      return true;
    } catch (error) {
      get().pushToast({
        kind: 'error',
        title: 'Could not remove backbone link',
        message: errorMessage(error),
        helpTopic: 'backbone',
        diagnostics: { endpoint: `/api/backbone/links/${name}`, bridge: state.settings.bridgeUrl },
      });
      return false;
    }
  },

  revokeApiToken: async () => {
    const state = get();
    get().pushToast({
      kind: 'warn',
      title: 'Rotate the token on the bridge host',
      message:
        'The v0.3 bridge has no API to rotate its own token. Replace the token file and restart the bridge, then paste the new token into Settings. Steps are in the revoke dialog.',
      helpTopic: 'security',
      diagnostics: { bridge: state.settings.bridgeUrl, bridge_version: state.bridgeVersion, mode: state.mode },
    });
    return null;
  },

  /* -------------------- legal maxima + regulatory override (v0.5) -------------------- */

  saveRegulatoryOverride: async (override) => {
    const state = get();
    const current = deriveRegulatoryState(state.config, state.bridgeStatus);
    const acknowledged = Boolean(override.acknowledged);
    if (state.mode === 'demo') {
      const nextConfig =
        mergeRegulatoryConfig(state.config, {
          ...(state.config ?? {}),
          legal: current.legal,
          regulatory_override: override,
          override_active: override.enabled && acknowledged,
          override_warning: override.enabled && acknowledged ? OVERRIDE_WARNING : null,
        }) ?? state.config;
      set({ config: nextConfig });
      get().pushToast({
        kind: override.enabled ? 'warn' : 'success',
        title: override.enabled ? 'Demo override active' : 'Override disabled',
        message: override.enabled
          ? 'Demo only — nothing is transmitted. The red banner is now visible on every screen.'
          : 'The demo override is off again.',
        helpTopic: 'regulatory-and-emergency',
      });
      return { ok: true };
    }
    try {
      const response = await patchBridgeConfig({
        baseUrl: state.settings.bridgeUrl,
        token: state.settings.token || null,
        patch: buildRegulatoryOverridePatch(override, acknowledged),
        timeoutMs: 8000,
      });
      const merged = mergeRegulatoryConfig(state.config, response);
      set({ config: merged });
      get().pushToast(
        override.enabled
          ? {
              kind: 'warn',
              title: 'Regulatory override saved',
              message: OVERRIDE_WARNING,
              helpTopic: 'regulatory-and-emergency',
              diagnostics: {
                endpoint: 'PATCH /api/config',
                bridge: state.settings.bridgeUrl,
                duty_cycle_pct: override.duty_cycle_pct,
                max_power_dbm: override.max_power_dbm,
                frequency_khz: override.frequency_khz,
              },
            }
          : {
              kind: 'success',
              title: 'Regulatory override disabled',
              message: 'The radio is back on the legal region defaults.',
              helpTopic: 'regulatory-and-emergency',
            },
      );
      return { ok: true };
    } catch (error) {
      const configError = error instanceof ConfigApiError ? error : null;
      const message = error instanceof Error ? error.message : String(error);
      get().pushToast({
        kind: 'error',
        title: 'The bridge rejected the override',
        message: configError?.warning ? `${message} ${configError.warning}` : message,
        helpTopic: 'regulatory-and-emergency',
        diagnostics: {
          endpoint: 'PATCH /api/config',
          bridge: state.settings.bridgeUrl,
          status: configError?.status ?? null,
          code: configError?.code ?? null,
        },
      });
      return { ok: false, message, warning: configError?.warning ?? undefined };
    }
  },

  disableRegulatoryOverride: async () => {
    const state = get();
    const current = deriveRegulatoryState(state.config, state.bridgeStatus);
    const disabled: RegulatoryOverride = {
      ...current.override,
      enabled: false,
      acknowledged: false,
      reason: current.override.reason,
    };
    const result = await get().saveRegulatoryOverride(disabled);
    return result.ok;
  },

  setDemoOverrideActive: (active) => {
    const state = get();
    if (state.mode !== 'demo') return;
    const current = deriveRegulatoryState(state.config, state.bridgeStatus);
    const override: RegulatoryOverride = active
      ? {
          ...current.override,
          enabled: true,
          acknowledged: true,
          duty_cycle_pct: current.override.duty_cycle_pct ?? current.legal.duty_cycle_pct,
          max_power_dbm: current.override.max_power_dbm ?? current.legal.max_power_dbm,
          reason: current.override.reason ?? 'Demo preview (no transmissions)',
        }
      : defaultRegulatoryOverride();
    const nextConfig =
      mergeRegulatoryConfig(state.config, {
        ...(state.config ?? {}),
        legal: state.config?.legal ?? legalLimitsForRegion(state.config?.radio.region),
        regulatory_override: override,
        override_active: active,
        override_warning: active ? OVERRIDE_WARNING : null,
      }) ?? state.config;
    set({ config: nextConfig });
  },

  /* -------------------- toasts + errors -------------------- */

  pushToast: (toast) => {
    const id = localId('toast');
    const state = get();
    const actionable =
      toast.kind === 'error' || toast.kind === 'warn'
        ? {
            helpTopic: toast.helpTopic ?? 'troubleshooting',
            diagnostics: toast.diagnostics ?? {
              bridge: state.settings.bridgeUrl,
              connection: state.connection,
              bridge_version: state.bridgeVersion ?? null,
              url: typeof window !== 'undefined' ? window.location.href : null,
            },
          }
        : {};
    set({ toasts: [...state.toasts, { ...toast, ...actionable, id }].slice(-4) });
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
        const upserted = upsertMessage(state.messages, data, optimistic?.tempId);
        // The echo of our own attachment starts the radio transfer: restart the stall clock.
        const messages =
          data.outgoing && data.attachment ? markMediaActivity(upserted, data.msg_id, Date.now()) : upserted;
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
        const data = event.data as MessageUpdateData;
        const atMs = Date.now();
        const touchesMedia = data.attachment !== undefined;
        const cancelledUpdate = data.cancelled === true || data.delivery === 'cancelled';
        const nextMessages = applyMessageUpdate(state.messages, data);
        const nextGroups = applyGroupMessageUpdate(state.groupMessages, data);
        set({
          messages: cancelledUpdate
            ? settleCancelledMessage(nextMessages, data.msg_id)
            : touchesMedia
              ? markMediaActivity(nextMessages, data.msg_id, atMs)
              : nextMessages,
          // Channel attachments arrive through `message_update` (the group_message event has no
          // attachment yet), so merge into both message lists.
          groupMessages: cancelledUpdate
            ? settleCancelledMessage(nextGroups, data.msg_id)
            : touchesMedia
              ? markMediaActivity(nextGroups, data.msg_id, atMs)
              : nextGroups,
          ...(cancelledUpdate ? { cancelSupport: 'supported' as const } : {}),
        });
        break;
      }
      case 'group_message': {
        const data = event.data as GroupMessageData;
        const optimistic = findOptimisticGroup(state.groupMessages, data);
        const upserted = upsertGroupMessage(state.groupMessages, data, optimistic?.tempId);
        const groupMessages =
          data.outgoing && data.attachment ? markMediaActivity(upserted, data.msg_id, Date.now()) : upserted;
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
      case 'cancel_result': {
        const data = event.data as CancelResultData;
        if (event.id) resolveCommand(event.id, data);
        if (data.cancelled !== false) {
          set({
            cancelSupport: 'supported',
            messages: settleCancelledMessage(state.messages, data.msg_id),
            groupMessages: settleCancelledMessage(state.groupMessages, data.msg_id),
          });
        }
        break;
      }
      case 'message_cancelled': {
        const data = event.data as MessageCancelledData;
        set({
          cancelSupport: 'supported',
          messages: settleCancelledMessage(state.messages, data.msg_id),
          groupMessages: settleCancelledMessage(state.groupMessages, data.msg_id),
        });
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
        const progress = resourceProgressFraction(data.received, data.total);
        const atMs = Date.now();
        set({
          messages: applyResourceProgress(state.messages, data.msg_id, progress, atMs),
          groupMessages: applyResourceProgress(state.groupMessages, data.msg_id, progress, atMs),
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
        const raw = event.data as ConfigData;
        const data = mergeRegulatoryConfig(state.config, raw) ?? raw;
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
        maybeSelectPublicChannel(set, get);
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
        const atMs = Date.now();
        // A sync snapshot of an in-flight outgoing attachment is current bridge state:
        // treat it as fresh media activity so a page reload does not fake a stall.
        const incoming = (data.messages ?? []).map((message) => {
          const attachment = message.attachment as { progress?: number } | null | undefined;
          const inFlight = message.outgoing && attachment && !message.delivered && (attachment.progress ?? 1) < 1;
          return inFlight ? { ...message, mediaActivityAtMs: atMs } : message;
        });
        set({ messages: upsertAllMessages(state.messages, incoming) });
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
        maybeAutoJoinPublicChannel(get);
        if (!get().syncing) maybeSelectPublicChannel(set, get);
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
          const atMs = Date.now();
          if (pending.kind === 'direct') {
            set({
              messages: state.messages
                .map((m) =>
                  m.tempId === pending.tempId
                    ? { ...m, msg_id: data.msg_id, delivery: 'sent' as const, tempId: undefined, mediaActivityAtMs: atMs }
                    : m,
                )
                .sort((a, b) => a.sent_at_ms - b.sent_at_ms),
            });
          } else {
            set({
              groupMessages: state.groupMessages
                .map((m) =>
                  m.tempId === pending.tempId
                    ? { ...m, msg_id: data.msg_id, tempId: undefined, mediaActivityAtMs: atMs }
                    : m,
                )
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
    ...(data.public ? { public: true } : {}),
  };
  set({
    channels: upsertChannel(state.channels, channel),
    config: state.config
      ? {
          ...state.config,
          channels: state.config.channels.some((c) => c.channel_id === data.channel_id)
            ? state.config.channels
            : [
                ...state.config.channels,
                {
                  channel_id: data.channel_id,
                  name: data.name,
                  psk_set: true,
                  ...(data.public ? { public: true } : {}),
                },
              ],
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
