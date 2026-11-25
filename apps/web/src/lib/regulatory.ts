/**
 * Legal maxima + emergency regulatory override (v0.5).
 *
 * Normative: docs/design/regulatory-and-regions.md §2/§3. The bridge is the source
 * of truth (`legal` / `regulatory_override` / `override_active` on /api/config and
 * /api/setup/*); everything here is defensive normalisation and pure UI helpers.
 */
import type { ConfigData, StatusData } from '../types/api';
import type { LegalLimits, RegulatoryOverride, RegulatoryOverridePatch, RegulatoryState } from '../types/regulatory';

/** Exact contract warning (core `OVERRIDE_WARNING`). */
export const OVERRIDE_WARNING =
  "Regulatory override active — transmissions may violate local radio regulations and void your radio's certification. Use only in a life-threatening emergency where local law permits. You are responsible for compliance and for any interference or damage.";

/** Persistent top-bar banner copy required by the UX spec (§3.3). */
export const OVERRIDE_BANNER_TEXT =
  '⚠ Regulatory override active — may violate local law; you are responsible. Disable in Advanced → Regulatory.';

export const OVERRIDE_ACK_WORD = 'OVERRIDE';
export const OVERRIDE_REASON_MAX_CHARS = 120;
export const OVERRIDE_HELP_TOPIC = 'regulatory-and-emergency';
/** Used only if the Lead-owned 17th help topic is not present yet. */
export const REGULATORY_HELP_FALLBACK_TOPIC = 'radio-and-region';

export const REGION_ORDER = ['EU868', 'EU433', 'US915', 'AU915', 'IN865', 'KR920', 'AS923'] as const;

interface RegionLegalSeed {
  centreKhz: number;
  legal: LegalLimits;
}

/** Region table v2 from the normative design doc; only used when the bridge omits `legal`. */
export const REGION_LEGAL_SEEDS: Record<string, RegionLegalSeed> = {
  EU868: {
    centreKhz: 869_525,
    legal: {
      region: 'EU868',
      max_power_dbm: 27,
      duty_cycle_pct: 10,
      max_bw_khz: 250,
      source: 'CEPT ERC/REC 70-03 Annex 1 (verify local law)',
    },
  },
  EU433: {
    centreKhz: 433_920,
    legal: {
      region: 'EU433',
      max_power_dbm: 10,
      duty_cycle_pct: 10,
      max_bw_khz: 250,
      source: 'CEPT ERC/REC 70-03 Annex 1 (verify local law)',
    },
  },
  US915: {
    centreKhz: 915_000,
    legal: { region: 'US915', max_power_dbm: 30, duty_cycle_pct: null, max_bw_khz: 500, source: 'FCC Part 15.247 (verify local law)' },
  },
  AU915: {
    centreKhz: 915_000,
    legal: { region: 'AU915', max_power_dbm: 30, duty_cycle_pct: null, max_bw_khz: 500, source: 'ACMA (verify local law)' },
  },
  IN865: {
    centreKhz: 865_000,
    legal: { region: 'IN865', max_power_dbm: 30, duty_cycle_pct: null, max_bw_khz: 250, source: 'WPC India (audit pending; verify local law)' },
  },
  KR920: {
    centreKhz: 920_000,
    legal: { region: 'KR920', max_power_dbm: 23, duty_cycle_pct: null, max_bw_khz: 250, source: 'Korea MIC (audit pending; verify local law)' },
  },
  AS923: {
    centreKhz: 923_200,
    legal: { region: 'AS923', max_power_dbm: 20, duty_cycle_pct: 10, max_bw_khz: 250, source: 'AS923 group floor (audit pending; verify local law)' },
  },
};

export const DEFAULT_REGION_FOR_LEGAL = 'EU868';

export function legalLimitsForRegion(region: string | null | undefined): LegalLimits {
  const key = (region ?? '').trim().toUpperCase();
  return { ...(REGION_LEGAL_SEEDS[key] ?? REGION_LEGAL_SEEDS[DEFAULT_REGION_FOR_LEGAL]!).legal };
}

export function regionCentreKhz(region: string | null | undefined): number {
  const key = (region ?? '').trim().toUpperCase();
  return (REGION_LEGAL_SEEDS[key] ?? REGION_LEGAL_SEEDS[DEFAULT_REGION_FOR_LEGAL]!).centreKhz;
}

function finiteNumber(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

export function normalizeLegalLimits(raw: unknown, regionFallback = DEFAULT_REGION_FOR_LEGAL): LegalLimits {
  const fallback = legalLimitsForRegion(regionFallback);
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return fallback;
  const record = raw as Record<string, unknown>;
  const region = typeof record.region === 'string' && record.region ? record.region : fallback.region;
  const maxPower = finiteNumber(record.max_power_dbm);
  const maxBw = finiteNumber(record.max_bw_khz);
  const dutyRaw = record.duty_cycle_pct;
  const duty =
    dutyRaw === null || dutyRaw === undefined
      ? null
      : typeof dutyRaw === 'number' && Number.isFinite(dutyRaw)
        ? Math.max(0, Math.min(100, dutyRaw))
        : fallback.duty_cycle_pct;
  return {
    region,
    max_power_dbm: maxPower !== null ? maxPower : fallback.max_power_dbm,
    duty_cycle_pct: duty,
    max_bw_khz: maxBw !== null && maxBw > 0 ? maxBw : fallback.max_bw_khz,
    source: typeof record.source === 'string' && record.source ? record.source : fallback.source ?? null,
  };
}

export function defaultRegulatoryOverride(): RegulatoryOverride {
  return {
    enabled: false,
    acknowledged: false,
    duty_cycle_pct: null,
    max_power_dbm: null,
    frequency_khz: null,
    reason: null,
  };
}

export function normalizeRegulatoryOverride(raw: unknown): RegulatoryOverride {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return defaultRegulatoryOverride();
  const record = raw as Record<string, unknown>;
  const duty = finiteNumber(record.duty_cycle_pct);
  const power = finiteNumber(record.max_power_dbm);
  const frequency = finiteNumber(record.frequency_khz);
  const reason = typeof record.reason === 'string' ? record.reason.slice(0, OVERRIDE_REASON_MAX_CHARS) : null;
  return {
    enabled: Boolean(record.enabled),
    acknowledged: Boolean(record.acknowledged),
    duty_cycle_pct: duty === null ? null : Math.max(0, Math.min(100, Math.round(duty))),
    max_power_dbm: power === null ? null : Math.round(power),
    frequency_khz: frequency === null ? null : Math.round(frequency),
    reason: reason && reason.trim() ? reason.trim() : null,
  };
}

export interface RegulatoryConfigLike {
  radio: { region: string; frequency_mhz?: number | null };
  legal?: LegalLimits | null;
  regulatory_override?: RegulatoryOverride | null;
  override_active?: boolean;
  override_warning?: string | null;
}

export interface RegulatoryStatusLike {
  regulatory_override_active?: boolean;
  regulatory_override_warning?: string | null;
}

/** Normalise config/status into the full legal + override view (older bridges get region fallbacks). */
export function deriveRegulatoryState(
  config: RegulatoryConfigLike | null,
  status?: RegulatoryStatusLike | null,
): RegulatoryState {
  const region = config?.radio.region ?? DEFAULT_REGION_FOR_LEGAL;
  const legal = normalizeLegalLimits(config?.legal, region);
  const override = normalizeRegulatoryOverride(config?.regulatory_override);
  const enabled = override.enabled && override.acknowledged;
  const active = Boolean(config?.override_active ?? status?.regulatory_override_active ?? enabled);
  return {
    legal,
    override,
    active,
    warning: active ? config?.override_warning ?? status?.regulatory_override_warning ?? OVERRIDE_WARNING : null,
  };
}

export type LegalDisplayMode = 'beginner' | 'advanced';

/** Beginner gets the quiet, jargon-free line; advanced gets the full maxima. */
export function formatLegalLimit(legal: LegalLimits, mode: LegalDisplayMode): string {
  const duty =
    legal.duty_cycle_pct === null || legal.duty_cycle_pct === undefined
      ? 'no duty cap'
      : `${legal.duty_cycle_pct} % duty`;
  if (mode === 'beginner') return `Legal limit for ${legal.region}: ${legal.max_power_dbm} dBm · ${duty}`;
  return `Legal limit: ${legal.max_power_dbm} dBm · ${duty} · max ${legal.max_bw_khz} kHz`;
}

export function legalMaximaLine(legal: LegalLimits): string {
  return formatLegalLimit(legal, 'advanced');
}

export interface OverrideExceedContext {
  /** Current region centre in kHz; the override frequency is compared against this. */
  currentFrequencyKhz?: number | null;
  /** Optional explicit region centre (defaults to the region table). */
  regionCentreKhz?: number | null;
}

/** True when an enabled override goes beyond the legal maxima (needs typed acknowledgement). */
export function exceedsLegal(
  override: RegulatoryOverride,
  legal: LegalLimits,
  context: OverrideExceedContext = {},
): boolean {
  if (!override.enabled) return false;
  if (
    override.duty_cycle_pct !== null &&
    legal.duty_cycle_pct !== null &&
    override.duty_cycle_pct > legal.duty_cycle_pct
  ) {
    return true;
  }
  if (override.max_power_dbm !== null && override.max_power_dbm > legal.max_power_dbm) return true;
  if (override.frequency_khz !== null) {
    const centre =
      context.regionCentreKhz ?? context.currentFrequencyKhz ?? regionCentreKhz(legal.region);
    if (Math.round(override.frequency_khz) !== Math.round(centre)) return true;
  }
  return false;
}

export type OverrideIntent = 'enable' | 'edit' | 'disable';

/** UI gate: first enable always needs the emergency acknowledgement; edits only when exceeding. */
export function requiresTypedConfirmation(input: {
  intent: OverrideIntent;
  override: RegulatoryOverride;
  legal: LegalLimits;
  context?: OverrideExceedContext;
}): boolean {
  if (input.intent === 'disable') return false;
  if (!input.override.enabled) return false;
  if (input.intent === 'enable') return true;
  return exceedsLegal(input.override, input.legal, input.context);
}

export interface OverrideDraft {
  dutyCyclePct: string;
  maxPowerDbm: string;
  frequencyKhz: string;
  reason: string;
}

export function defaultOverrideDraft(legal: LegalLimits, override?: RegulatoryOverride | null): OverrideDraft {
  return {
    dutyCyclePct:
      override?.duty_cycle_pct !== null && override?.duty_cycle_pct !== undefined
        ? String(override.duty_cycle_pct)
        : legal.duty_cycle_pct === null
          ? ''
          : String(legal.duty_cycle_pct),
    maxPowerDbm:
      override?.max_power_dbm !== null && override?.max_power_dbm !== undefined
        ? String(override.max_power_dbm)
        : String(legal.max_power_dbm),
    frequencyKhz:
      override?.frequency_khz !== null && override?.frequency_khz !== undefined
        ? String(override.frequency_khz)
        : '',
    reason: override?.reason ?? '',
  };
}

export interface OverrideDraftValidation {
  errors: string[];
  override: RegulatoryOverride | null;
}

/** Parse/validate the modal fields. Empty duty/power/frequency fall back to legal defaults. */
export function validateOverrideDraft(
  draft: OverrideDraft,
  legal: LegalLimits,
  options: { enabled?: boolean; acknowledged?: boolean } = {},
): OverrideDraftValidation {
  const errors: string[] = [];
  const parseOptionalInt = (value: string, label: string): number | null => {
    const trimmed = value.trim();
    if (!trimmed) return null;
    const parsed = Number(trimmed);
    if (!Number.isFinite(parsed) || !Number.isInteger(parsed)) {
      errors.push(`${label} must be a whole number.`);
      return null;
    }
    return parsed;
  };

  const duty = parseOptionalInt(draft.dutyCyclePct, 'Duty cycle');
  if (duty !== null && (duty < 0 || duty > 100)) errors.push('Duty cycle must be between 0 and 100 %.');
  const power = parseOptionalInt(draft.maxPowerDbm, 'Max TX power');
  if (power !== null && (power < -10 || power > 30)) errors.push('Max TX power must be between -10 and 30 dBm.');
  const frequency = parseOptionalInt(draft.frequencyKhz, 'Frequency');
  if (frequency !== null && (frequency < 150_000 || frequency > 960_000)) {
    errors.push('Frequency must be between 150 000 and 960 000 kHz (150–960 MHz).');
  }
  const reason = draft.reason.trim();
  if (reason.length > OVERRIDE_REASON_MAX_CHARS) {
    errors.push(`Reason must be ${OVERRIDE_REASON_MAX_CHARS} characters or fewer.`);
  }
  if (errors.length) return { errors, override: null };
  return {
    errors: [],
    override: {
      enabled: options.enabled ?? true,
      acknowledged: options.acknowledged ?? false,
      duty_cycle_pct: duty ?? legal.duty_cycle_pct,
      max_power_dbm: power ?? legal.max_power_dbm,
      frequency_khz: frequency,
      reason: reason || null,
    },
  };
}

export interface RegulatoryPatchBody {
  regulatory_override: RegulatoryOverridePatch;
  acknowledged?: true;
}

/** PATCH /api/config body (snake_case; acknowledgement only when explicitly granted). */
export function buildRegulatoryOverridePatch(
  override: RegulatoryOverride,
  acknowledged = false,
): RegulatoryPatchBody {
  const patch: RegulatoryOverridePatch = {
    enabled: override.enabled,
    acknowledged: override.acknowledged,
    duty_cycle_pct: override.duty_cycle_pct,
    max_power_dbm: override.max_power_dbm,
    frequency_khz: override.frequency_khz,
    reason: override.reason,
  };
  return acknowledged ? { regulatory_override: patch, acknowledged: true } : { regulatory_override: patch };
}

/** Overlay a PATCH/API response onto the current config, preserving channels/backbone and normalising v0.5 fields. */
export function mergeRegulatoryConfig(current: ConfigData | null, response: unknown): ConfigData | null {
  if (!response || typeof response !== 'object' || Array.isArray(response)) return current;
  const raw = response as Record<string, unknown>;
  const radioRaw = raw.radio && typeof raw.radio === 'object' ? (raw.radio as Record<string, unknown>) : {};
  const nodeRaw = raw.node && typeof raw.node === 'object' ? (raw.node as Record<string, unknown>) : {};
  const featuresRaw = raw.features && typeof raw.features === 'object' ? (raw.features as Record<string, unknown>) : {};
  const region =
    (typeof radioRaw.region === 'string' && radioRaw.region) ||
    (typeof raw.region === 'string' && raw.region) ||
    current?.radio.region ||
    DEFAULT_REGION_FOR_LEGAL;
  const legal = normalizeLegalLimits(raw.legal ?? current?.legal, region);
  const override = normalizeRegulatoryOverride(raw.regulatory_override ?? current?.regulatory_override);
  const active = Boolean(raw.override_active ?? current?.override_active ?? (override.enabled && override.acknowledged));
  const warning = active
    ? (typeof raw.override_warning === 'string' && raw.override_warning) || current?.override_warning || OVERRIDE_WARNING
    : null;
  if (!current) {
    if (!raw.node || !raw.radio || !raw.features) return null;
    return {
      node: {
        name: typeof nodeRaw.name === 'string' ? nodeRaw.name : 'this node',
        class: typeof nodeRaw.class === 'string' ? nodeRaw.class : 'client',
        announce_interval_s: finiteNumber(nodeRaw.announce_interval_s) ?? 1800,
      },
      radio: {
        profile: typeof radioRaw.profile === 'string' ? radioRaw.profile : 'MEDIUM_FAST',
        region,
        tx_power_dbm: finiteNumber(radioRaw.tx_power_dbm) ?? legal.max_power_dbm,
        frequency_mhz: finiteNumber(radioRaw.frequency_mhz),
        interface: typeof radioRaw.interface === 'string' ? radioRaw.interface : null,
        max_power_dbm: finiteNumber(radioRaw.max_power_dbm) ?? legal.max_power_dbm,
        duty_cycle_pct: finiteNumber(radioRaw.duty_cycle_pct) ?? legal.duty_cycle_pct,
        max_bw_khz: finiteNumber(radioRaw.max_bw_khz) ?? legal.max_bw_khz,
      },
      features: {
        auto_announce: featuresRaw.auto_announce !== false,
        store_forward: featuresRaw.store_forward !== false,
        position_sharing: Boolean(featuresRaw.position_sharing),
        ui_mode: featuresRaw.ui_mode === 'advanced' || featuresRaw.ui_mode === 'beginner' ? featuresRaw.ui_mode : undefined,
      },
      channels: Array.isArray(raw.channels) ? (raw.channels as ConfigData['channels']) : [],
      legal,
      regulatory_override: override,
      override_active: active,
      override_warning: warning,
    };
  }
  const next: ConfigData = {
    ...current,
    node: { ...current.node, ...(typeof raw.node === 'object' ? (raw.node as ConfigData['node']) : {}) },
    radio: {
      ...current.radio,
      ...(typeof raw.radio === 'object' ? (raw.radio as ConfigData['radio']) : {}),
      region,
    },
    features: { ...current.features, ...(typeof raw.features === 'object' ? (raw.features as ConfigData['features']) : {}) },
    channels: Array.isArray(raw.channels) ? (raw.channels as ConfigData['channels']) : current.channels,
    legal,
    regulatory_override: override,
    override_active: active,
    override_warning: warning,
  };
  if (raw.backbone && typeof raw.backbone === 'object') next.backbone = raw.backbone as ConfigData['backbone'];
  return next;
}

/** Effective limits shown when an override is active (power clamped like the bridge/firmware). */
export function effectiveRegulatoryValues(
  legal: LegalLimits,
  override: RegulatoryOverride,
  hardwareMaxDbm = 22,
): { dutyCyclePct: number | null; powerDbm: number; frequencyKhz: number } {
  const duty = override.enabled ? override.duty_cycle_pct ?? legal.duty_cycle_pct : legal.duty_cycle_pct;
  const powerRaw = override.enabled ? override.max_power_dbm ?? legal.max_power_dbm : legal.max_power_dbm;
  const power = Math.min(powerRaw, hardwareMaxDbm);
  const frequency = override.enabled
    ? override.frequency_khz ?? regionCentreKhz(legal.region)
    : regionCentreKhz(legal.region);
  return { dutyCyclePct: duty, powerDbm: power, frequencyKhz: frequency };
}

export function formatFrequencyKhz(khz: number): string {
  return `${(khz / 1000).toFixed(3)} MHz`;
}

/** Demo-only config seed that keeps the override off (screenshots toggle it in the panel). */
export function demoRegulatoryConfigFields(region = 'EU868'): Pick<ConfigData, 'legal' | 'regulatory_override' | 'override_active' | 'override_warning'> {
  const legal = legalLimitsForRegion(region);
  return {
    legal,
    regulatory_override: defaultRegulatoryOverride(),
    override_active: false,
    override_warning: null,
  };
}

export type { StatusData };
