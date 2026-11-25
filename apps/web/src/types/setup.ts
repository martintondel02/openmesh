/** Types for first-run setup (docs/design/ux-help-system.md §3/§5). */

export type SetupRole = 'client' | 'repeater' | 'gateway' | 'tracker';
export type SetupPrivacy = 'private' | 'shared';

export interface SetupChecks {
  identity: boolean;
  radio: boolean;
  api_token: boolean;
  channel: boolean;
  backbone: boolean;
  agent: boolean;
}

export interface SetupRecommended {
  region: string;
  profile: string;
  role: SetupRole;
  /** Country the recommendation was computed for, when known. */
  country?: string | null;
  announce_interval_s?: number | null;
  store_forward?: boolean | null;
  /** Normalised privacy preference (the API calls the boolean `position_sharing`). */
  position?: SetupPrivacy | null;
  position_sharing?: boolean | null;
  duty_cycle_pct?: number | null;
  frequency_mhz?: number | null;
  notes?: string[];
  /** Visible warning from the recommendation (e.g. unknown country → EU868). */
  warning?: string | null;
}

export interface SetupCounts {
  nodes?: number;
  contacts?: number;
  channels?: number;
  messages?: number;
  radios?: number;
  backbone_peers?: number;
}

export interface SetupState {
  first_run: boolean;
  setup_completed?: boolean;
  node_id?: string | null;
  node_name?: string | null;
  checks: SetupChecks;
  recommended: SetupRecommended;
  counts: SetupCounts;
  /** Present when the bridge reports a recommended ui mode. */
  ui_mode?: 'beginner' | 'advanced' | null;
}

export interface QuickSetupRequest {
  role: SetupRole;
  country: string;
  node_name?: string;
  privacy: SetupPrivacy;
  create_channel?: boolean;
}

export interface QuickSetupChannel {
  channel_id: string;
  name: string;
  invite_url: string;
  /** Returned exactly once; shown with a "shown once" label. */
  psk_once?: string | null;
}

export interface QuickSetupCheckItem {
  key: keyof SetupChecks | string;
  label: string;
  ok: boolean;
  detail?: string | null;
}

export type NextStepAction = 'message' | 'contact' | 'backbone' | 'agent' | 'invite' | 'help';

export interface QuickSetupNextStep {
  title: string;
  detail: string;
  help_topic: string;
  action?: NextStepAction | string | null;
}

export interface QuickSetupResult {
  applied: Record<string, unknown>;
  channel: QuickSetupChannel | null;
  checklist: QuickSetupCheckItem[];
  next_steps: QuickSetupNextStep[];
  recommended?: SetupRecommended;
  first_run?: boolean;
  setup_completed?: boolean;
}

/** GET /api/setup/recommend response. */
export type SetupRecommendation = Omit<SetupRecommended, 'role'> & { role: SetupRole };

/** Wizard step machine (pure reducer tested). */
export type OnboardingStep = 'role' | 'place' | 'secure' | 'success';

export interface OnboardingDraft {
  role: SetupRole;
  country: string;
  nodeName: string;
  privacy: SetupPrivacy;
}

export interface OnboardingState {
  open: boolean;
  step: OnboardingStep;
  draft: OnboardingDraft;
  submitting: boolean;
  error: string | null;
  result: QuickSetupResult | null;
}
