import { describe, expect, it } from 'vitest';
import {
  OVERRIDE_ACK_WORD,
  OVERRIDE_BANNER_TEXT,
  OVERRIDE_REASON_MAX_CHARS,
  OVERRIDE_WARNING,
  REGION_LEGAL_SEEDS,
  buildRegulatoryOverridePatch,
  defaultOverrideDraft,
  defaultRegulatoryOverride,
  deriveRegulatoryState,
  effectiveRegulatoryValues,
  exceedsLegal,
  formatFrequencyKhz,
  formatLegalLimit,
  legalLimitsForRegion,
  legalMaximaLine,
  mergeRegulatoryConfig,
  normalizeLegalLimits,
  normalizeRegulatoryOverride,
  regionCentreKhz,
  requiresTypedConfirmation,
  validateOverrideDraft,
} from '../regulatory';
import { ConfigApiError, patchBridgeConfig } from '../config-api';
import { createDemoDataset } from '../mock/data';
import { normalizeRecommended, normalizeSetupState } from '../setup';
import { demoRegulatoryConfigFields } from '../regulatory';
import type { ConfigData } from '../../types/api';
import type { RegulatoryOverride } from '../../types/regulatory';

const EU = legalLimitsForRegion('EU868');

function override(overrides: Partial<RegulatoryOverride> = {}): RegulatoryOverride {
  return { ...defaultRegulatoryOverride(), enabled: true, acknowledged: true, ...overrides };
}

function config(overrides: Partial<ConfigData> = {}): ConfigData {
  return {
    node: { name: 'oslo', class: 'client', announce_interval_s: 1800 },
    radio: { profile: 'MEDIUM_FAST', region: 'EU868', tx_power_dbm: 27, frequency_mhz: 869.525 },
    features: { auto_announce: true, store_forward: true, position_sharing: false },
    channels: [],
    ...overrides,
  };
}

describe('legal maxima', () => {
  it('mirrors region table v2 and formats beginner vs advanced copy', () => {
    expect(REGION_LEGAL_SEEDS.EU868?.legal).toMatchObject({
      max_power_dbm: 27,
      duty_cycle_pct: 10,
      max_bw_khz: 250,
    });
    expect(legalLimitsForRegion('EU868')).toEqual(REGION_LEGAL_SEEDS.EU868!.legal);
    expect(legalLimitsForRegion('US915').duty_cycle_pct).toBeNull();
    expect(legalLimitsForRegion('AS923')).toMatchObject({ max_power_dbm: 20, duty_cycle_pct: 10 });
    expect(legalLimitsForRegion('unknown')).toEqual(REGION_LEGAL_SEEDS.EU868!.legal);
    expect(formatLegalLimit(EU, 'beginner')).toBe('Legal limit for EU868: 27 dBm · 10 % duty');
    expect(formatLegalLimit(EU, 'advanced')).toBe('Legal limit: 27 dBm · 10 % duty · max 250 kHz');
    expect(legalMaximaLine(EU)).toContain('max 250 kHz');
  });

  it('normalises the bridge legal block and older-bridge fallbacks', () => {
    const parsed = normalizeLegalLimits(
      { region: 'US915', max_power_dbm: 30, duty_cycle_pct: null, max_bw_khz: 500, source: 'FCC' },
      'EU868',
    );
    expect(parsed).toEqual({ region: 'US915', max_power_dbm: 30, duty_cycle_pct: null, max_bw_khz: 500, source: 'FCC' });
    expect(normalizeLegalLimits(undefined, 'EU868')).toEqual(legalLimitsForRegion('EU868'));
    expect(normalizeLegalLimits({ max_power_dbm: 'nope', duty_cycle_pct: 150 }, 'EU868')).toMatchObject({
      region: 'EU868',
      max_power_dbm: 27,
      duty_cycle_pct: 100,
    });
    expect(regionCentreKhz('EU868')).toBe(869_525);
    expect(formatFrequencyKhz(869_525)).toBe('869.525 MHz');
  });
});

describe('regulatory override normalisation + state', () => {
  it('normalises partial/old payloads to a safe off state', () => {
    expect(normalizeRegulatoryOverride(undefined)).toEqual(defaultRegulatoryOverride());
    expect(normalizeRegulatoryOverride({ enabled: true, acknowledged: true, duty_cycle_pct: 120, max_power_dbm: 31, frequency_khz: 869_525, reason: 'x'.repeat(300) })).toMatchObject({
      enabled: true,
      acknowledged: true,
      duty_cycle_pct: 100,
      max_power_dbm: 31,
      frequency_khz: 869_525,
    });
    expect(normalizeRegulatoryOverride({ enabled: true, acknowledged: true, reason: 'x'.repeat(300) }).reason).toHaveLength(OVERRIDE_REASON_MAX_CHARS);
  });

  it('derives active state and the contract warning, with fallbacks for older bridges', () => {
    const inactive = deriveRegulatoryState(config());
    expect(inactive.active).toBe(false);
    expect(inactive.warning).toBeNull();
    expect(inactive.legal.region).toBe('EU868');
    const active = deriveRegulatoryState(
      config({ regulatory_override: override({ duty_cycle_pct: 50 }), override_active: true }),
      { regulatory_override_active: true },
    );
    expect(active.active).toBe(true);
    expect(active.warning).toBe(OVERRIDE_WARNING);
    const fromStatus = deriveRegulatoryState(config(), { regulatory_override_active: true, regulatory_override_warning: 'status warning' });
    expect(fromStatus.active).toBe(true);
    expect(fromStatus.warning).toBe('status warning');
    const enabledNoFlag = deriveRegulatoryState(config({ regulatory_override: override() }));
    expect(enabledNoFlag.active).toBe(true);
  });

  it('detects only values that exceed the legal limit', () => {
    const legal = EU;
    expect(exceedsLegal(override({ duty_cycle_pct: 11 }), legal)).toBe(true);
    expect(exceedsLegal(override({ max_power_dbm: 28 }), legal)).toBe(true);
    expect(exceedsLegal(override({ frequency_khz: 868_100 }), legal)).toBe(true);
    expect(exceedsLegal(override({ duty_cycle_pct: 5, max_power_dbm: 20, frequency_khz: 869_525 }), legal)).toBe(false);
    expect(exceedsLegal(override({ enabled: false, duty_cycle_pct: 100 }), legal)).toBe(false);
    // Fair-use regions have no duty cap, so a duty value alone does not exceed.
    expect(exceedsLegal(override({ duty_cycle_pct: 80 }), legalLimitsForRegion('US915'))).toBe(false);
  });

  it('requires typed confirmation on first enable and on exceeding edits, not on safe reductions', () => {
    expect(requiresTypedConfirmation({ intent: 'enable', override: override({ duty_cycle_pct: 10 }), legal: EU })).toBe(true);
    expect(requiresTypedConfirmation({ intent: 'edit', override: override({ duty_cycle_pct: 12 }), legal: EU })).toBe(true);
    expect(requiresTypedConfirmation({ intent: 'edit', override: override({ duty_cycle_pct: 5, max_power_dbm: 20 }), legal: EU })).toBe(false);
    expect(requiresTypedConfirmation({ intent: 'disable', override: override({ duty_cycle_pct: 90 }), legal: EU })).toBe(false);
    expect(requiresTypedConfirmation({ intent: 'edit', override: override({ enabled: false, duty_cycle_pct: 90 }), legal: EU })).toBe(false);
  });
});

describe('override draft validation and patch body', () => {
  it('defaults fields to the legal maxima and validates ranges', () => {
    const draft = defaultOverrideDraft(EU);
    expect(draft).toEqual({ dutyCyclePct: '10', maxPowerDbm: '27', frequencyKhz: '', reason: '' });
    const valid = validateOverrideDraft({ ...draft, maxPowerDbm: '22' }, EU);
    expect(valid.errors).toEqual([]);
    expect(valid.override).toMatchObject({ enabled: true, duty_cycle_pct: 10, max_power_dbm: 22, frequency_khz: null, reason: null });
    expect(validateOverrideDraft({ ...draft, dutyCyclePct: '101' }, EU).errors.join(' ')).toContain('between 0 and 100');
    expect(validateOverrideDraft({ ...draft, frequencyKhz: '100000' }, EU).errors.join(' ')).toContain('150 000');
    expect(validateOverrideDraft({ ...draft, maxPowerDbm: 'abc' }, EU).errors.join(' ')).toContain('whole number');
    expect(validateOverrideDraft({ ...draft, reason: 'x'.repeat(OVERRIDE_REASON_MAX_CHARS + 1) }, EU).errors.join(' ')).toContain('characters or fewer');
    expect(validateOverrideDraft({ ...draft, dutyCyclePct: '', maxPowerDbm: '' }, EU).override).toMatchObject({
      duty_cycle_pct: 10,
      max_power_dbm: 27,
    });
  });

  it('builds the PATCH body with snake_case fields and optional acknowledgement', () => {
    const active = override({ duty_cycle_pct: 50, max_power_dbm: 27, frequency_khz: 869_525, reason: 'emergency' });
    expect(buildRegulatoryOverridePatch(active, true)).toEqual({
      regulatory_override: {
        enabled: true,
        acknowledged: true,
        duty_cycle_pct: 50,
        max_power_dbm: 27,
        frequency_khz: 869_525,
        reason: 'emergency',
      },
      acknowledged: true,
    });
    const safe = buildRegulatoryOverridePatch(override({ duty_cycle_pct: 5 }), false);
    expect(safe.acknowledged).toBeUndefined();
    expect(safe.regulatory_override).toMatchObject({ enabled: true, duty_cycle_pct: 5 });
    const disabled = buildRegulatoryOverridePatch(override({ enabled: false, acknowledged: false }), false);
    expect(disabled.regulatory_override.enabled).toBe(false);
    expect(disabled.acknowledged).toBeUndefined();
  });

  it('computes effective values with the hardware clamp', () => {
    expect(effectiveRegulatoryValues(EU, override({ duty_cycle_pct: 50, max_power_dbm: 30 }), 22)).toEqual({
      dutyCyclePct: 50,
      powerDbm: 22,
      frequencyKhz: 869_525,
    });
    expect(effectiveRegulatoryValues(EU, defaultRegulatoryOverride(), 22)).toEqual({
      dutyCyclePct: 10,
      powerDbm: 22,
      frequencyKhz: 869_525,
    });
  });
});

describe('config merge + PATCH client', () => {
  it('overlays a regulatory API response while preserving channels/backbone/node state', () => {
    const base = config({ channels: [{ channel_id: 'c1', name: 'Familjen', psk_set: true }], backbone: undefined });
    const merged = mergeRegulatoryConfig(base, {
      legal: { region: 'EU868', max_power_dbm: 27, duty_cycle_pct: 10, max_bw_khz: 250 },
      regulatory_override: override({ duty_cycle_pct: 80 }),
      override_active: true,
      override_warning: OVERRIDE_WARNING,
    });
    expect(merged?.channels).toEqual(base.channels);
    expect(merged?.node).toEqual(base.node);
    expect(merged?.regulatory_override?.duty_cycle_pct).toBe(80);
    expect(merged?.override_active).toBe(true);
    expect(merged?.override_warning).toBe(OVERRIDE_WARNING);
    // Older bridge response without v0.5 fields keeps working with a region fallback.
    const old = mergeRegulatoryConfig(base, { radio: { profile: 'LONG_FAST' } });
    expect(old?.radio.profile).toBe('LONG_FAST');
    expect(old?.legal?.region).toBe('EU868');
    expect(old?.override_active).toBe(false);
    expect(mergeRegulatoryConfig(null, {})).toBeNull();
  });

  it('PATCHes /api/config with attribution headers and the override body', async () => {
    const calls: Array<{ url: string; init?: RequestInit }> = [];
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push({ url: String(input), init });
      return new Response(JSON.stringify({ ...config(), legal: EU }), { status: 200, headers: { 'Content-Type': 'application/json' } });
    }) as typeof fetch;
    const payload = buildRegulatoryOverridePatch(override({ duty_cycle_pct: 50 }), true);
    await patchBridgeConfig({ baseUrl: 'http://127.0.0.1:8787', token: 'test-token-not-a-secret', patch: payload, fetchImpl });
    expect(calls[0]!.url).toBe('http://127.0.0.1:8787/api/config');
    expect(calls[0]!.init?.method).toBe('PATCH');
    const headers = calls[0]!.init?.headers as Record<string, string>;
    expect(headers.Authorization).toBe('Bearer test-token-not-a-secret');
    expect(headers['X-OpenMesh-Client']).toBe('web');
    expect(JSON.parse(String(calls[0]!.init?.body))).toMatchObject({ regulatory_override: { enabled: true, duty_cycle_pct: 50 }, acknowledged: true });
  });

  it('surfaces a 422 acknowledgement error with code and warning', async () => {
    const fetchImpl = (async () =>
      new Response(
        JSON.stringify({
          error: 'regulatory_override_requires_acknowledgement',
          message: 'regulatory override requires acknowledgement',
          warning: OVERRIDE_WARNING,
        }),
        { status: 422, headers: { 'Content-Type': 'application/json' } },
      )) as typeof fetch;
    const error = await patchBridgeConfig({
      baseUrl: 'http://127.0.0.1:8787',
      patch: buildRegulatoryOverridePatch(override({ duty_cycle_pct: 90 }), false),
      fetchImpl,
    }).catch((cause: unknown) => cause);
    expect(error).toBeInstanceOf(ConfigApiError);
    expect((error as ConfigApiError).status).toBe(422);
    expect((error as ConfigApiError).code).toBe('regulatory_override_requires_acknowledgement');
    expect((error as ConfigApiError).warning).toBe(OVERRIDE_WARNING);
  });

  it('keeps the banner and acknowledgement copy exact', () => {
    expect(OVERRIDE_ACK_WORD).toBe('OVERRIDE');
    expect(OVERRIDE_BANNER_TEXT).toContain('may violate local law; you are responsible');
    expect(OVERRIDE_BANNER_TEXT).toContain('Disable in Advanced → Regulatory');
    expect(OVERRIDE_WARNING).toContain('life-threatening emergency');
    expect(OVERRIDE_WARNING).toContain("void your radio's certification");
  });
});

describe('setup + demo regulatory data', () => {
  it('normalises legal and override fields from setup payloads', () => {
    const recommendation = normalizeRecommended({
      country: 'NO',
      role: 'client',
      region: 'EU868',
      profile: 'MEDIUM_FAST',
      legal: REGION_LEGAL_SEEDS.EU868!.legal,
    });
    expect(recommendation.legal).toEqual(REGION_LEGAL_SEEDS.EU868!.legal);
    const state = normalizeSetupState({
      first_run: true,
      checks: {},
      recommended: { region: 'EU868' },
      regulatory_override: { enabled: false, acknowledged: false },
      override_warning: null,
    });
    expect(state.regulatory_override?.enabled).toBe(false);
    expect(state.override_warning).toBeNull();
    expect(state.recommended.legal?.max_power_dbm).toBe(27);
  });

  it('seeds an inactive override + legal block in demo mode, with a fake token', () => {
    const dataset = createDemoDataset(new Date(2026, 2, 15, 12, 0, 0).getTime());
    expect(dataset.config.legal).toMatchObject({ region: 'EU868', max_power_dbm: 27, duty_cycle_pct: 10, max_bw_khz: 250 });
    expect(dataset.config.regulatory_override).toMatchObject({ enabled: false, acknowledged: false });
    expect(dataset.config.override_active).toBe(false);
    expect(dataset.config.override_warning).toBeNull();
    expect(dataset.status.regulatory_override_active).toBe(false);
    expect(dataset.config.radio.max_power_dbm).toBe(27);
    expect(dataset.config.radio.duty_cycle_pct).toBe(10);
    expect(dataset.backbone.token).toContain('demo-');
    expect(demoRegulatoryConfigFields('US915').legal).toMatchObject({ region: 'US915', max_power_dbm: 30 });
    const derived = deriveRegulatoryState(dataset.config, dataset.status);
    expect(derived.active).toBe(false);
  });
});
