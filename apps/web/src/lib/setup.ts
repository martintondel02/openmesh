/** First-run setup data: presets, recommendations, normalization and the wizard state machine. */
import type {
  OnboardingDraft,
  OnboardingState,
  OnboardingStep,
  QuickSetupCheckItem,
  QuickSetupNextStep,
  QuickSetupRequest,
  QuickSetupResult,
  SetupChecks,
  SetupPrivacy,
  SetupRecommendation,
  SetupRecommended,
  SetupRole,
  SetupState,
} from '../types/setup';
import type { UiMode } from './mode';

export type RegionCode = 'EU868' | 'US915' | 'AU915' | 'IN865' | 'KR920' | 'AS923';

export interface CountryOption {
  code: string;
  name: string;
  region: RegionCode;
}

/** Country → legal band (UX spec §5). */
export const COUNTRIES: CountryOption[] = [
  { code: 'NO', name: 'Norway', region: 'EU868' },
  { code: 'SE', name: 'Sweden', region: 'EU868' },
  { code: 'DK', name: 'Denmark', region: 'EU868' },
  { code: 'FI', name: 'Finland', region: 'EU868' },
  { code: 'IS', name: 'Iceland', region: 'EU868' },
  { code: 'GB', name: 'United Kingdom', region: 'EU868' },
  { code: 'IE', name: 'Ireland', region: 'EU868' },
  { code: 'DE', name: 'Germany', region: 'EU868' },
  { code: 'FR', name: 'France', region: 'EU868' },
  { code: 'NL', name: 'Netherlands', region: 'EU868' },
  { code: 'BE', name: 'Belgium', region: 'EU868' },
  { code: 'ES', name: 'Spain', region: 'EU868' },
  { code: 'PT', name: 'Portugal', region: 'EU868' },
  { code: 'IT', name: 'Italy', region: 'EU868' },
  { code: 'CH', name: 'Switzerland', region: 'EU868' },
  { code: 'AT', name: 'Austria', region: 'EU868' },
  { code: 'PL', name: 'Poland', region: 'EU868' },
  { code: 'CZ', name: 'Czechia', region: 'EU868' },
  { code: 'US', name: 'United States', region: 'US915' },
  { code: 'CA', name: 'Canada', region: 'US915' },
  { code: 'MX', name: 'Mexico', region: 'US915' },
  { code: 'AU', name: 'Australia', region: 'AU915' },
  { code: 'NZ', name: 'New Zealand', region: 'AU915' },
  { code: 'IN', name: 'India', region: 'IN865' },
  { code: 'KR', name: 'South Korea', region: 'KR920' },
  { code: 'SG', name: 'Singapore', region: 'AS923' },
  { code: 'MY', name: 'Malaysia', region: 'AS923' },
  { code: 'TH', name: 'Thailand', region: 'AS923' },
  { code: 'ID', name: 'Indonesia', region: 'AS923' },
  { code: 'PH', name: 'Philippines', region: 'AS923' },
  { code: 'VN', name: 'Vietnam', region: 'AS923' },
  { code: 'JP', name: 'Japan', region: 'AS923' },
];

export function countryOption(code: string | null | undefined): CountryOption | null {
  if (!code) return null;
  return COUNTRIES.find((country) => country.code === code.toUpperCase()) ?? null;
}

export function regionForCountry(code: string | null | undefined): { region: RegionCode; known: boolean; warning: string | null } {
  const country = countryOption(code);
  if (country) return { region: country.region, known: true, warning: null };
  return {
    region: 'EU868',
    known: false,
    warning: 'We could not match that country, so EU868 (Europe) is selected. Check Settings → Region before transmitting.',
  };
}

export interface SetupRoleOption {
  id: SetupRole;
  label: string;
  plainTitle: string;
  description: string;
  examples: string;
  profile: string;
  announceMinutes: number;
  storeForward: boolean;
  privacy: SetupPrivacy;
  batteryFriendly: boolean;
  helpTopic: string;
}

/** Quick-setup presets are server-owned (UX spec §5); these mirror them for demo/offline. */
export const SETUP_ROLES: SetupRoleOption[] = [
  {
    id: 'client',
    label: 'Phone or laptop',
    plainTitle: 'I carry this with me',
    description: 'Sends and receives messages, and can share your location when you choose.',
    examples: 'phone, laptop, tablet',
    profile: 'MEDIUM_FAST',
    announceMinutes: 30,
    storeForward: false,
    privacy: 'private',
    batteryFriendly: false,
    helpTopic: 'nodes-telemetry',
  },
  {
    id: 'repeater',
    label: 'Repeater',
    plainTitle: 'I leave this in one place',
    description: 'Passes messages along so the mesh reaches further. Put it high up and power it continuously.',
    examples: 'roof, attic, solar box',
    profile: 'LONG_FAST',
    announceMinutes: 60,
    storeForward: true,
    privacy: 'private',
    batteryFriendly: false,
    helpTopic: 'nodes-telemetry',
  },
  {
    id: 'gateway',
    label: 'Gateway',
    plainTitle: 'I bridge to the internet',
    description: 'Connects your local mesh to distant meshes, usually with a backbone running on the same machine.',
    examples: 'home server, Raspberry Pi, VPS',
    profile: 'LONG_FAST',
    announceMinutes: 60,
    storeForward: true,
    privacy: 'private',
    batteryFriendly: false,
    helpTopic: 'backbone',
  },
  {
    id: 'tracker',
    label: 'Tracker',
    plainTitle: 'I track something moving',
    description: 'Reports position and battery on a strict power budget, such as a bike or a pet collar.',
    examples: 'bike, boat, pet collar',
    profile: 'MEDIUM_FAST',
    announceMinutes: 60,
    storeForward: false,
    privacy: 'private',
    batteryFriendly: true,
    helpTopic: 'map-and-position',
  },
];

export function setupRole(role: unknown): SetupRoleOption {
  return SETUP_ROLES.find((option) => option.id === role) ?? SETUP_ROLES[0]!;
}

export function recommendedFor(country: string, role: SetupRole): SetupRecommended {
  const preset = setupRole(role);
  const region = regionForCountry(country).region;
  return {
    country: country.toUpperCase(),
    region,
    profile: preset.profile,
    role: preset.id,
    announce_interval_s: preset.announceMinutes * 60,
    store_forward: preset.storeForward,
    position: preset.privacy,
    duty_cycle_pct: preset.id === 'repeater' || preset.id === 'gateway' ? 1 : null,
  };
}

export function suggestNodeName(role: SetupRole, suffix?: string): string {
  const base = role === 'client' ? 'my-node' : `my-${role}`;
  if (!suffix) return base;
  const cleaned = suffix
    .toLowerCase()
    .replace(/[^a-z0-9-]/g, '-')
    .replace(/-+/g, '-')
    .replace(/^-|-$/g, '')
    .slice(0, 20);
  return cleaned ? `${base}-${cleaned}` : base;
}

export function createOnboardingState(overrides: Partial<OnboardingDraft> = {}): OnboardingState {
  return {
    open: false,
    step: 'role',
    draft: {
      role: overrides.role ?? 'client',
      country: overrides.country ?? 'NO',
      nodeName: overrides.nodeName ?? '',
      privacy: overrides.privacy ?? 'private',
    },
    submitting: false,
    error: null,
    result: null,
  };
}

export type OnboardingAction =
  | { type: 'open' }
  | { type: 'close' }
  | { type: 'next' }
  | { type: 'back' }
  | { type: 'draft'; patch: Partial<OnboardingDraft> }
  | { type: 'submit-start' }
  | { type: 'submit-error'; message: string }
  | { type: 'submit-success'; result: QuickSetupResult }
  | { type: 'reset' };

const STEP_ORDER: OnboardingStep[] = ['role', 'place', 'secure', 'success'];

export function onboardingReducer(state: OnboardingState, action: OnboardingAction): OnboardingState {
  switch (action.type) {
    case 'open':
      return { ...state, open: true, error: null };
    case 'close':
      return { ...state, open: false, submitting: false };
    case 'next': {
      const index = STEP_ORDER.indexOf(state.step);
      const step = STEP_ORDER[Math.min(index + 1, STEP_ORDER.length - 1)]!;
      return { ...state, step, error: null };
    }
    case 'back': {
      const index = STEP_ORDER.indexOf(state.step);
      const step = STEP_ORDER[Math.max(index - 1, 0)]!;
      return { ...state, step, error: null };
    }
    case 'draft': {
      const draft = { ...state.draft, ...action.patch };
      if (action.patch.role && action.patch.role !== state.draft.role && !action.patch.nodeName) {
        draft.nodeName = state.draft.nodeName || suggestNodeName(action.patch.role);
      }
      return { ...state, draft };
    }
    case 'submit-start':
      return { ...state, submitting: true, error: null };
    case 'submit-error':
      return { ...state, submitting: false, error: action.message };
    case 'submit-success':
      return { ...state, submitting: false, error: null, result: action.result, step: 'success' };
    case 'reset':
      return createOnboardingState(state.draft);
    default:
      return state;
  }
}

/** Bridge checks are `{ok, detail}` objects in v0.3; older/demo payloads use booleans. */
function checkFlag(value: unknown): boolean {
  if (typeof value === 'boolean') return value;
  if (value && typeof value === 'object') return Boolean((value as Record<string, unknown>).ok);
  return false;
}

export function normalizeSetupChecks(raw: unknown): SetupChecks {
  const record = raw && typeof raw === 'object' ? (raw as Record<string, unknown>) : {};
  return {
    identity: checkFlag(record.identity),
    radio: checkFlag(record.radio),
    api_token: checkFlag(record.api_token),
    channel: checkFlag(record.channel),
    backbone: checkFlag(record.backbone),
    agent: checkFlag(record.agent),
  };
}

export function normalizeRecommended(raw: unknown, fallback?: SetupRecommended): SetupRecommended {
  const record = raw && typeof raw === 'object' ? (raw as Record<string, unknown>) : {};
  const base =
    fallback ??
    recommendedFor(typeof record.country === 'string' ? record.country : 'NO', (record.role as SetupRole) ?? 'client');
  const role = SETUP_ROLES.some((option) => option.id === record.role) ? (record.role as SetupRole) : base.role;
  const preset = setupRole(role);
  return {
    region: typeof record.region === 'string' && record.region ? record.region : base.region,
    profile: typeof record.profile === 'string' && record.profile ? record.profile : preset.profile,
    role,
    country: typeof record.country === 'string' ? record.country : base.country ?? null,
    announce_interval_s:
      typeof record.announce_interval_s === 'number' ? record.announce_interval_s : preset.announceMinutes * 60,
    store_forward: typeof record.store_forward === 'boolean' ? record.store_forward : preset.storeForward,
    position:
      record.position === 'private' || record.position === 'shared'
        ? record.position
        : typeof record.position_sharing === 'boolean'
          ? record.position_sharing ? 'shared' : 'private'
          : preset.privacy,
    duty_cycle_pct: typeof record.duty_cycle_pct === 'number' ? record.duty_cycle_pct : base.duty_cycle_pct ?? null,
    frequency_mhz: typeof record.frequency_mhz === 'number' ? record.frequency_mhz : null,
    position_sharing:
      typeof record.position_sharing === 'boolean'
        ? record.position_sharing
        : (record.position === 'shared' ? true : record.position === 'private' ? false : null),
    notes: toStringArray(record.notes),
    warning: typeof record.warning === 'string' && record.warning ? record.warning : null,
  };
}

export function normalizeSetupState(raw: unknown): SetupState {
  const record = raw && typeof raw === 'object' ? (raw as Record<string, unknown>) : {};
  const counts = record.counts && typeof record.counts === 'object' ? (record.counts as Record<string, unknown>) : {};
  const uiMode = record.ui_mode === 'beginner' || record.ui_mode === 'advanced' ? record.ui_mode : null;
  const firstRun = record.first_run === true || (record.first_run === undefined && record.setup_completed === false);
  return {
    first_run: firstRun,
    setup_completed: record.setup_completed === true || (record.setup_completed === undefined ? !firstRun : false),
    node_id: typeof record.node_id === 'string' ? record.node_id : null,
    node_name: typeof record.node_name === 'string' ? record.node_name : null,
    checks: normalizeSetupChecks(record.checks),
    recommended: normalizeRecommended(record.recommended),
    counts: {
      nodes: typeof counts.nodes === 'number' ? counts.nodes : undefined,
      contacts: typeof counts.contacts === 'number' ? counts.contacts : undefined,
      channels: typeof counts.channels === 'number' ? counts.channels : undefined,
      messages: typeof counts.messages === 'number' ? counts.messages : undefined,
      radios: typeof counts.radios === 'number' ? counts.radios : undefined,
      backbone_peers: typeof counts.backbone_peers === 'number' ? counts.backbone_peers : undefined,
    },
    ui_mode: uiMode,
  };
}

export function normalizeQuickSetupRequest(raw: Partial<Omit<QuickSetupRequest, 'role' | 'country'>> & { role?: unknown; country?: unknown }): QuickSetupRequest {
  const role = SETUP_ROLES.some((option) => option.id === raw.role) ? (raw.role as SetupRole) : 'client';
  const country = typeof raw.country === 'string' && raw.country ? raw.country.toUpperCase() : 'NO';
  const privacy: SetupPrivacy = raw.privacy === 'shared' ? 'shared' : 'private';
  const nodeName = typeof raw.node_name === 'string' ? raw.node_name.trim().slice(0, 32) : '';
  return {
    role,
    country,
    privacy,
    ...(nodeName ? { node_name: nodeName } : {}),
    create_channel: raw.create_channel !== false,
  };
}

export function normalizeQuickSetupChecks(raw: unknown): QuickSetupCheckItem[] {
  if (!Array.isArray(raw)) return [];
  return raw
    .map((item): QuickSetupCheckItem | null => {
      if (!item || typeof item !== 'object') return null;
      const record = item as Record<string, unknown>;
      if (typeof record.label !== 'string' || !record.label) return null;
      return {
        key:
          typeof record.key === 'string'
            ? record.key
            : typeof record.id === 'string'
              ? record.id
              : record.label.toLowerCase().replace(/\s+/g, '-'),
        label: record.label,
        ok: Boolean(record.ok),
        detail: typeof record.detail === 'string' ? record.detail : null,
      };
    })
    .filter((item): item is QuickSetupCheckItem => Boolean(item));
}

export function normalizeQuickSetupNextSteps(raw: unknown): QuickSetupNextStep[] {
  if (!Array.isArray(raw)) return [];
  return raw
    .map((item): QuickSetupNextStep | null => {
      if (!item || typeof item !== 'object') return null;
      const record = item as Record<string, unknown>;
      if (typeof record.title !== 'string' || !record.title) return null;
      return {
        title: record.title,
        detail: typeof record.detail === 'string' ? record.detail : '',
        help_topic: typeof record.help_topic === 'string' ? record.help_topic : 'getting-started',
        action: typeof record.action === 'string' ? record.action : null,
      };
    })
    .filter((item): item is QuickSetupNextStep => Boolean(item));
}

export function normalizeQuickSetupResult(raw: unknown): QuickSetupResult {
  const record = raw && typeof raw === 'object' ? (raw as Record<string, unknown>) : {};
  const channelRaw = record.channel && typeof record.channel === 'object' ? (record.channel as Record<string, unknown>) : null;
  const channel = channelRaw && typeof channelRaw.channel_id === 'string'
    ? {
        channel_id: channelRaw.channel_id,
        name: typeof channelRaw.name === 'string' ? channelRaw.name : 'Private channel',
        invite_url: typeof channelRaw.invite_url === 'string' ? channelRaw.invite_url : '',
        psk_once: typeof channelRaw.psk_once === 'string' ? channelRaw.psk_once : null,
      }
    : null;
  return {
    applied: record.applied && typeof record.applied === 'object' ? (record.applied as Record<string, unknown>) : {},
    channel,
    checklist: normalizeQuickSetupChecks(record.checklist),
    next_steps: normalizeQuickSetupNextSteps(record.next_steps),
    recommended: record.recommended ? normalizeRecommended(record.recommended) : undefined,
    first_run: typeof record.first_run === 'boolean' ? record.first_run : undefined,
    setup_completed: typeof record.setup_completed === 'boolean' ? record.setup_completed : undefined,
  };
}

export interface SetupCheckContext {
  setupState?: SetupState | null;
  ownNodeId?: string | null;
  channelCount?: number;
  contactCount?: number;
  backboneEnabled?: boolean;
  agentConfigured?: boolean;
  radioConnected?: boolean | null;
  tokenSet?: boolean;
}

/** Merge server state with live client state so the checklist works offline too. */
export function deriveSetupChecks(context: SetupCheckContext): SetupChecks {
  const server = context.setupState?.checks;
  const channelsKnown = typeof context.channelCount === 'number';
  return {
    identity: Boolean(server?.identity || context.ownNodeId),
    radio: Boolean(server?.radio || context.radioConnected),
    api_token: Boolean(server?.api_token || context.tokenSet),
    channel: Boolean(server?.channel || (channelsKnown && (context.channelCount ?? 0) > 0)),
    backbone: Boolean(server?.backbone || context.backboneEnabled),
    agent: Boolean(server?.agent || context.agentConfigured),
  };
}

export function isSetupComplete(checks: SetupChecks): boolean {
  return checks.identity && checks.radio && checks.api_token && checks.channel;
}

export function isSetupStateComplete(state: SetupState | null | undefined): boolean {
  if (!state) return false;
  if (state.setup_completed === true) return true;
  return isSetupComplete(state.checks);
}

export interface ChecklistItem {
  key: string;
  label: string;
  ok: boolean;
  detail: string;
  helpTopic: string;
  action?: string;
}

const BEGINNER_CHECKLIST: Array<{ key: keyof SetupChecks; label: string; detail: string; helpTopic: string }> = [
  { key: 'identity', label: 'Name your node', detail: 'Give this device a name others can recognise.', helpTopic: 'contacts-identity' },
  { key: 'radio', label: 'Connect the radio', detail: 'Check the USB radio is plugged in and answering.', helpTopic: 'install' },
  { key: 'api_token', label: 'Secure the local API', detail: 'Use a token so only you and your tools can reach the bridge.', helpTopic: 'security' },
  { key: 'channel', label: 'Create a private channel', detail: 'A private group chat with its own key.', helpTopic: 'channels' },
];

const ADVANCED_CHECKLIST: Array<{ key: string; label: string; detail: string; helpTopic: string }> = [
  { key: 'backbone', label: 'Configure a backbone', detail: 'Run an always-on relay for distant meshes.', helpTopic: 'backbone' },
  { key: 'agent', label: 'Connect an agent', detail: 'Give an MCP client read-only access first.', helpTopic: 'agents' },
  { key: 'airtime', label: 'Review the airtime budget', detail: 'Set announce cadence and check duty-cycle limits.', helpTopic: 'radio-and-region' },
  { key: 'backup', label: 'Back up the identity', detail: 'Export the encrypted identity before a reset or migration.', helpTopic: 'security' },
];

export function checklistForMode(mode: UiMode, checks: SetupChecks): ChecklistItem[] {
  const base = (mode === 'advanced' ? ADVANCED_CHECKLIST : BEGINNER_CHECKLIST).map((item) => ({
    ...item,
    ok: item.key in checks ? Boolean(checks[item.key as keyof SetupChecks]) : false,
  }));
  return base.map((item) => ({
    key: item.key,
    label: item.label,
    detail: item.detail,
    helpTopic: item.helpTopic,
    ok: item.ok,
  }));
}

export function checklistProgress(items: ChecklistItem[]): { done: number; total: number; pct: number } {
  const done = items.filter((item) => item.ok).length;
  return { done, total: items.length, pct: items.length ? Math.round((done / items.length) * 100) : 0 };
}

export function inviteShareText(channel: { name: string; invite_url: string }): string {
  return `Join my OpenMesh channel “${channel.name}”:\n${channel.invite_url}\n\nOpen it in the OpenMesh app to join the channel. Treat this link like a password.`;
}

/** A safe demo quick-setup result (used by the mock transport; tokens are clearly fake). */
export function buildDemoQuickSetupResult(input: {
  role: SetupRole;
  country: string;
  nodeName: string;
  privacy: SetupPrivacy;
  channelId: string;
  channelName: string;
  inviteUrl: string;
}): QuickSetupResult {
  const recommendation = recommendedFor(input.country, input.role);
  return {
    applied: {
      node_name: input.nodeName,
      role: input.role,
      region: recommendation.region,
      profile: recommendation.profile,
      position: input.privacy,
      store_forward: recommendation.store_forward,
      announce_interval_s: recommendation.announce_interval_s,
      demo: true,
    },
    channel: {
      channel_id: input.channelId,
      name: input.channelName,
      invite_url: input.inviteUrl,
      psk_once: 'demo-channel-key-not-a-real-secret',
    },
    checklist: [
      { key: 'identity', label: 'Identity created', ok: true, detail: `Named “${input.nodeName}”` },
      { key: 'radio', label: 'Radio detected', ok: true, detail: 'Demo radio connected' },
      { key: 'api_token', label: 'Secure defaults applied', ok: true, detail: 'Token auth · loopback API' },
      { key: 'channel', label: 'Private channel created', ok: true, detail: input.channelName },
      { key: 'region', label: 'Region set', ok: true, detail: `${recommendation.region} · ${input.country}` },
    ],
    next_steps: [
      { title: 'Send your first message', detail: 'Open a conversation and say hello.', help_topic: 'messaging', action: 'message' },
      { title: 'Add a contact', detail: 'Verify someone you know by their invite.', help_topic: 'contacts-identity', action: 'contact' },
      { title: 'Run a backbone', detail: 'Turn a spare box into an always-on relay.', help_topic: 'backbone', action: 'backbone' },
      { title: 'Connect an AI agent', detail: 'Give an assistant read-only access through MCP.', help_topic: 'agents', action: 'agent' },
      { title: 'Invite someone', detail: 'Share the channel invite you just created.', help_topic: 'channels', action: 'invite' },
    ],
  };
}

export function nextStepsForResult(result: QuickSetupResult | null): QuickSetupNextStep[] {
  if (!result) return [];
  if (result.next_steps.length) return result.next_steps;
  return buildDemoQuickSetupResult({
    role: 'client',
    country: 'NO',
    nodeName: 'my-node',
    privacy: 'private',
    channelId: 'demo',
    channelName: 'Private channel',
    inviteUrl: 'openmesh://channel?demo',
  }).next_steps;
}

export function recommendedCountryOptions(): CountryOption[] {
  return [...COUNTRIES].sort((left, right) => left.name.localeCompare(right.name));
}

export function setupRecommendationFromState(state: SetupState | null): SetupRecommendation | null {
  if (!state) return null;
  return { ...state.recommended };
}

function toStringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : [];
}
