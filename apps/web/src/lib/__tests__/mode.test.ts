import { describe, expect, it } from 'vitest';
import {
  BEGINNER_BANNED_TERMS,
  DEFAULT_UI_MODE,
  bannedTermsInCopy,
  canSeeAgents,
  canSeeBackbone,
  loadUiMode,
  normalizeUiMode,
  persistUiMode,
  resolveUiModeOverride,
  shouldAutoShowOnboarding,
  uiModeLabel,
  visibleNavScreens,
  UI_MODE_STORAGE_KEY,
} from '../mode';

function fakeStorage(initial: Record<string, string> = {}) {
  const values = { ...initial };
  return {
    getItem: (key: string) => values[key] ?? null,
    setItem: (key: string, value: string) => {
      values[key] = value;
    },
    dump: () => values,
  };
}

describe('beginner/advanced mode', () => {
  it('defaults to beginner and normalises invalid values', () => {
    expect(DEFAULT_UI_MODE).toBe('beginner');
    expect(loadUiMode(fakeStorage())).toBe('beginner');
    expect(loadUiMode(fakeStorage({ [UI_MODE_STORAGE_KEY]: 'nonsense' }))).toBe('beginner');
    expect(normalizeUiMode('advanced')).toBe('advanced');
    expect(normalizeUiMode(undefined, 'advanced')).toBe('advanced');
  });

  it('persists and reloads the chosen mode', () => {
    const storage = fakeStorage();
    persistUiMode('advanced', storage);
    expect(storage.dump()[UI_MODE_STORAGE_KEY]).toBe('advanced');
    expect(loadUiMode(storage)).toBe('advanced');
  });

  it('accepts ?mode= for screenshots without corrupting storage', () => {
    expect(resolveUiModeOverride('?demo=1&mode=advanced')).toBe('advanced');
    expect(resolveUiModeOverride('?mode=beginner')).toBe('beginner');
    expect(resolveUiModeOverride('?mode=expert')).toBeNull();
    expect(resolveUiModeOverride('')).toBeNull();
  });

  it('derives nav visibility for both modes', () => {
    expect(visibleNavScreens({ mode: 'beginner', backboneAvailable: false, agentsAvailable: false })).toEqual([
      'chats', 'map', 'nodes', 'contacts', 'help', 'settings',
    ]);
    expect(visibleNavScreens({ mode: 'beginner', backboneAvailable: true, agentsAvailable: true })).toEqual([
      'chats', 'map', 'nodes', 'contacts', 'backbone', 'agents', 'help', 'settings',
    ]);
    expect(visibleNavScreens({ mode: 'advanced', backboneAvailable: false, agentsAvailable: false })).toEqual([
      'chats', 'map', 'nodes', 'contacts', 'backbone', 'agents', 'help', 'settings',
    ]);
    expect(canSeeBackbone({ mode: 'beginner', backboneAvailable: false, agentsAvailable: false })).toBe(false);
    expect(canSeeBackbone({ mode: 'advanced', backboneAvailable: false, agentsAvailable: false })).toBe(true);
    expect(canSeeAgents({ mode: 'beginner', backboneAvailable: false, agentsAvailable: true })).toBe(true);
  });

  it('auto-shows onboarding only for a fresh beginner install, or a forced demo link', () => {
    expect(
      shouldAutoShowOnboarding({ mode: 'beginner', demo: false, demoForced: false, firstRun: true, setupComplete: false, dismissed: false }),
    ).toBe(true);
    expect(
      shouldAutoShowOnboarding({ mode: 'advanced', demo: false, demoForced: false, firstRun: true, setupComplete: false, dismissed: false }),
    ).toBe(false);
    expect(
      shouldAutoShowOnboarding({ mode: 'beginner', demo: false, demoForced: false, firstRun: true, setupComplete: true, dismissed: false }),
    ).toBe(false);
    expect(
      shouldAutoShowOnboarding({ mode: 'beginner', demo: true, demoForced: false, firstRun: true, setupComplete: false, dismissed: false }),
    ).toBe(false);
    expect(
      shouldAutoShowOnboarding({ mode: 'beginner', demo: true, demoForced: true, firstRun: false, setupComplete: true, dismissed: true }),
    ).toBe(true);
  });

  it('labels modes and flags unexplained jargon in beginner copy', () => {
    expect(uiModeLabel('beginner')).toBe('Beginner');
    expect(uiModeLabel('advanced')).toBe('Advanced');
    expect(BEGINNER_BANNED_TERMS.length).toBeGreaterThan(5);
    expect(bannedTermsInCopy('The radio profile and region decide coverage.')).toEqual([]);
    expect(bannedTermsInCopy('Set the SF and TTL by hand.')).toEqual(['SF', 'TTL']);
    expect(bannedTermsInCopy('The PSK is secret')).toEqual(['PSK']);
    // Whole-word matching avoids false positives such as "transfer".
    expect(bannedTermsInCopy('A smooth transfer is fine')).toEqual([]);
  });
});
