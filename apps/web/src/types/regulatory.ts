/** Legal maxima and emergency regulatory override (docs/design/regulatory-and-regions.md §2). */

export interface LegalLimits {
  region: string;
  max_power_dbm: number;
  /** `null` means no duty-cycle cap (fair use), e.g. US915. */
  duty_cycle_pct: number | null;
  max_bw_khz: number;
  source?: string | null;
}

export interface RegulatoryOverride {
  enabled: boolean;
  acknowledged: boolean;
  duty_cycle_pct: number | null;
  max_power_dbm: number | null;
  frequency_khz: number | null;
  reason: string | null;
}

/** Partial override accepted by PATCH /api/config (snake_case). */
export type RegulatoryOverridePatch = Partial<Omit<RegulatoryOverride, 'reason'>> & {
  reason?: string | null;
};

/** Fields added to GET /api/config, /api/status and the setup payloads (defensive optional). */
export interface RegulatoryConfigFields {
  legal?: LegalLimits | null;
  regulatory_override?: RegulatoryOverride | null;
  override_active?: boolean;
  override_warning?: string | null;
}

export interface RegulatoryState {
  legal: LegalLimits;
  override: RegulatoryOverride;
  active: boolean;
  warning: string | null;
}
