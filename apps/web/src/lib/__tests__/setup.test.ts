import { describe, expect, it } from 'vitest';
import {
  COUNTRIES,
  SETUP_ROLES,
  buildDemoQuickSetupResult,
  checklistForMode,
  checklistProgress,
  countryOption,
  createOnboardingState,
  deriveSetupChecks,
  inviteShareText,
  isSetupComplete,
  isSetupStateComplete,
  nextStepsForResult,
  normalizeQuickSetupChecks,
  normalizeQuickSetupRequest,
  normalizeQuickSetupResult,
  normalizeRecommended,
  normalizeSetupState,
  onboardingReducer,
  recommendedFor,
  regionForCountry,
  setupRole,
  suggestNodeName,
} from '../setup';

describe('setup presets and regions', () => {
  it('maps countries to legal regions', () => {
    expect(regionForCountry('NO')).toMatchObject({ region: 'EU868', known: true });
    expect(regionForCountry('us').region).toBe('US915');
    expect(regionForCountry('AU').region).toBe('AU915');
    expect(regionForCountry('IN').region).toBe('IN865');
    expect(regionForCountry('KR').region).toBe('KR920');
    expect(regionForCountry('SG').region).toBe('AS923');
    const unknown = regionForCountry('ZZ');
    expect(unknown.region).toBe('EU868');
    expect(unknown.known).toBe(false);
    expect(unknown.warning).toBeTruthy();
    expect(countryOption('NO')?.name).toBe('Norway');
    expect(countryOption(null)).toBeNull();
  });

  it('mirrors the server-owned quick-setup presets', () => {
    expect(SETUP_ROLES.map((role) => role.id)).toEqual(['client', 'repeater', 'gateway', 'tracker']);
    const client = recommendedFor('NO', 'client');
    expect(client).toMatchObject({ region: 'EU868', profile: 'MEDIUM_FAST', store_forward: false, position: 'private', announce_interval_s: 1800 });
    const repeater = recommendedFor('US', 'repeater');
    expect(repeater).toMatchObject({ region: 'US915', profile: 'LONG_FAST', store_forward: true, duty_cycle_pct: 1 });
    expect(setupRole('tracker').batteryFriendly).toBe(true);
    expect(setupRole('bogus').id).toBe('client');
  });

  it('suggests friendly node names', () => {
    expect(suggestNodeName('client')).toBe('my-node');
    expect(suggestNodeName('repeater')).toBe('my-repeater');
    expect(suggestNodeName('client', 'Anne Ø')).toBe('my-node-anne');
    expect(suggestNodeName('gateway', '')).toBe('my-gateway');
  });
});

describe('setup API payload normalisation', () => {
  it('normalises the live {ok, detail} check objects and completion flags', () => {
    const state = normalizeSetupState({
      node_id: 'a1b2c3d4e5f60718',
      node_name: 'oslo-node',
      first_run: false,
      setup_completed: true,
      checks: {
        identity: { ok: true, detail: 'named' },
        radio: { ok: true },
        api_token: { ok: false, detail: 'missing' },
        channel: { ok: true },
        backbone: { ok: false },
        agent: { ok: false },
      },
      recommended: { region: 'EU868', profile: 'MEDIUM_FAST', role: 'client' },
      counts: { nodes: 9, messages: 40, contacts: 3, channels: 2, radios: 1, backbone_peers: 3 },
    });
    expect(state.setup_completed).toBe(true);
    expect(state.first_run).toBe(false);
    expect(state.checks).toEqual({ identity: true, radio: true, api_token: false, channel: true, backbone: false, agent: false });
    expect(state.counts).toMatchObject({ nodes: 9, radios: 1, backbone_peers: 3 });
    expect(state.node_name).toBe('oslo-node');
  });

  it('treats missing setup_completed as first-run when requested', () => {
    const state = normalizeSetupState({ first_run: true, checks: { identity: true } });
    expect(state.first_run).toBe(true);
    expect(state.setup_completed).toBe(false);
  });

  it('normalises recommendations, including position_sharing and warnings', () => {
    const recommendation = normalizeRecommended({
      country: 'NO',
      role: 'client',
      region: 'EU868',
      profile: 'MEDIUM_FAST',
      store_forward: false,
      position_sharing: false,
      announce_interval_s: 1800,
      duty_cycle_pct: null,
      frequency_mhz: 869.525,
      warning: 'Country not recognised; using EU868.',
      notes: ['start with the default'],
    });
    expect(recommendation.position).toBe('private');
    expect(recommendation.position_sharing).toBe(false);
    expect(recommendation.frequency_mhz).toBe(869.525);
    expect(recommendation.warning).toContain('EU868');
    expect(recommendation.notes).toEqual(['start with the default']);
  });

  it('normalises quick-setup requests and responses', () => {
    expect(normalizeQuickSetupRequest({ role: 'repeater', country: 'no', privacy: 'shared', node_name: '  roof box  ' })).toEqual({
      role: 'repeater',
      country: 'NO',
      privacy: 'shared',
      node_name: 'roof box',
      create_channel: true,
    });
    expect(normalizeQuickSetupRequest({ role: 'invalid' }).role).toBe('client');
    const result = normalizeQuickSetupResult({
      applied: { node_name: 'my-node', channel_created: true },
      channel: { channel_id: 'f0e1d2c3b4a59687', name: 'Familien', invite_url: 'openmesh://channel?psk=demo', psk_once: 'secret-once' },
      checklist: [{ id: 'identity', label: 'Identity created', ok: true }, null, { label: 'No id', ok: false }],
      next_steps: [{ title: 'Send a message', detail: 'say hello', help_topic: 'messaging' }],
      recommended: { region: 'EU868', profile: 'MEDIUM_FAST', role: 'client' },
      first_run: false,
      setup_completed: true,
    });
    expect(result.channel?.psk_once).toBe('secret-once');
    expect(result.checklist).toHaveLength(2);
    expect(result.checklist[0]?.key).toBe('identity');
    expect(result.checklist[1]?.key).toBe('no-id');
    expect(result.next_steps[0]).toMatchObject({ title: 'Send a message', help_topic: 'messaging' });
    expect(result.recommended?.region).toBe('EU868');
    expect(result.setup_completed).toBe(true);
    expect(normalizeQuickSetupChecks('nope')).toEqual([]);
  });
});

describe('setup checklist', () => {
  it('derives checks from live client state when the server cannot answer', () => {
    const checks = deriveSetupChecks({
      ownNodeId: 'a1b2c3d4e5f60718',
      radioConnected: true,
      tokenSet: true,
      channelCount: 2,
      backboneEnabled: true,
      agentConfigured: true,
    });
    expect(checks).toEqual({ identity: true, radio: true, api_token: true, channel: true, backbone: true, agent: true });
    expect(isSetupComplete(checks)).toBe(true);
    expect(isSetupComplete({ ...checks, channel: false })).toBe(false);
    expect(isSetupStateComplete({ setup_completed: true, first_run: false, checks, recommended: recommendedFor('NO', 'client'), counts: {} })).toBe(true);
    expect(deriveSetupChecks({}).channel).toBe(false);
  });

  it('builds a beginner and an advanced checklist with progress', () => {
    const checks = { identity: true, radio: true, api_token: false, channel: false, backbone: true, agent: false };
    const beginner = checklistForMode('beginner', checks);
    expect(beginner.map((item) => item.key)).toEqual(['identity', 'radio', 'api_token', 'channel']);
    expect(checklistProgress(beginner)).toEqual({ done: 2, total: 4, pct: 50 });
    const advanced = checklistForMode('advanced', checks);
    expect(advanced.map((item) => item.key)).toEqual(['backbone', 'agent', 'airtime', 'backup']);
    expect(advanced[0]?.ok).toBe(true);
    expect(advanced[2]?.ok).toBe(false);
  });

  it('builds a safe demo quick-setup result and share text', () => {
    const result = buildDemoQuickSetupResult({
      role: 'client',
      country: 'NO',
      nodeName: 'my-phone',
      privacy: 'private',
      channelId: 'f0e1d2c3b4a59687',
      channelName: 'Familien',
      inviteUrl: 'openmesh://channel?psk=demo',
    });
    expect(result.channel?.psk_once).toContain('demo');
    expect(result.checklist.every((item) => item.ok)).toBe(true);
    expect(result.next_steps.map((step) => step.action)).toEqual(['message', 'contact', 'backbone', 'agent', 'invite']);
    expect(nextStepsForResult(result)).toHaveLength(5);
    expect(nextStepsForResult(null)).toEqual([]);
    expect(inviteShareText({ name: 'Familien', invite_url: 'openmesh://channel?psk=demo' })).toContain('like a password');
  });
});

describe('onboarding state machine', () => {
  it('opens, moves through the three steps and goes back', () => {
    let state = createOnboardingState();
    expect(state).toMatchObject({ open: false, step: 'role', draft: { role: 'client', country: 'NO', privacy: 'private' } });
    state = onboardingReducer(state, { type: 'open' });
    state = onboardingReducer(state, { type: 'next' });
    expect(state.step).toBe('place');
    state = onboardingReducer(state, { type: 'next' });
    expect(state.step).toBe('secure');
    state = onboardingReducer(state, { type: 'next' });
    expect(state.step).toBe('success');
    state = onboardingReducer(state, { type: 'back' });
    expect(state.step).toBe('secure');
    state = onboardingReducer(state, { type: 'close' });
    expect(state.open).toBe(false);
  });

  it('updates the draft and suggests a name when the role changes', () => {
    let state = createOnboardingState();
    state = onboardingReducer(state, { type: 'draft', patch: { role: 'repeater' } });
    expect(state.draft.role).toBe('repeater');
    expect(state.draft.nodeName).toBe('my-repeater');
    state = onboardingReducer(state, { type: 'draft', patch: { country: 'US', nodeName: 'roof box' } });
    expect(state.draft.country).toBe('US');
    expect(state.draft.nodeName).toBe('roof box');
  });

  it('tracks submit/error/success and resets', () => {
    let state = createOnboardingState();
    state = onboardingReducer(state, { type: 'submit-start' });
    expect(state.submitting).toBe(true);
    state = onboardingReducer(state, { type: 'submit-error', message: 'bridge offline' });
    expect(state.submitting).toBe(false);
    expect(state.error).toBe('bridge offline');
    state = onboardingReducer(state, { type: 'submit-start' });
    const result = normalizeQuickSetupResult({ checklist: [], next_steps: [] });
    state = onboardingReducer(state, { type: 'submit-success', result });
    expect(state.step).toBe('success');
    expect(state.result).toBe(result);
    expect(onboardingReducer(state, { type: 'reset' }).step).toBe('role');
  });
});

describe('country list', () => {
  it('covers the documented region mapping and is unique', () => {
    const codes = COUNTRIES.map((country) => country.code);
    expect(new Set(codes).size).toBe(codes.length);
    expect(COUNTRIES.filter((country) => country.region === 'EU868').length).toBeGreaterThan(10);
    expect(COUNTRIES).toContainEqual({ code: 'NO', name: 'Norway', region: 'EU868' });
  });
});
