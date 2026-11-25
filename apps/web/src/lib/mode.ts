/** Beginner / advanced UI mode (docs/design/ux-help-system.md §2). */
import type { Screen } from '../types/api';

export type UiMode = 'beginner' | 'advanced';

export const UI_MODE_STORAGE_KEY = 'openmesh.mode';
export const DEFAULT_UI_MODE: UiMode = 'beginner';

export const UI_MODES: Array<{ id: UiMode; label: string; summary: string }> = [
  {
    id: 'beginner',
    label: 'Beginner',
    summary: 'Plain language, safe defaults and guided setup. Advanced options stay one tap away.',
  },
  {
    id: 'advanced',
    label: 'Advanced',
    summary: 'Every radio, identity, backbone and agent control, with exact values and costs.',
  },
];

export function isUiMode(value: unknown): value is UiMode {
  return value === 'beginner' || value === 'advanced';
}

export function normalizeUiMode(value: unknown, fallback: UiMode = DEFAULT_UI_MODE): UiMode {
  return isUiMode(value) ? value : fallback;
}

export function loadUiMode(storage: Pick<Storage, 'getItem'> | null | undefined): UiMode {
  try {
    return normalizeUiMode(storage?.getItem(UI_MODE_STORAGE_KEY));
  } catch {
    return DEFAULT_UI_MODE;
  }
}

export function persistUiMode(mode: UiMode, storage: Pick<Storage, 'setItem'> | null | undefined): void {
  try {
    storage?.setItem(UI_MODE_STORAGE_KEY, mode);
  } catch {
    // Storage can be unavailable (private mode, Tauri); mode still works for the session.
  }
}

/** `?mode=advanced` wins for screenshots without overwriting the saved preference. */
export function resolveUiModeOverride(search: string): UiMode | null {
  const value = new URLSearchParams(search).get('mode');
  return isUiMode(value) ? value : null;
}

export function isAdvanced(mode: UiMode): boolean {
  return mode === 'advanced';
}

export function uiModeLabel(mode: UiMode): string {
  return mode === 'advanced' ? 'Advanced' : 'Beginner';
}

export interface NavVisibility {
  mode: UiMode;
  backboneAvailable: boolean;
  agentsAvailable: boolean;
}

/** Screens that are always available in both modes (Help is always reachable). */
export const CORE_NAV_SCREENS: Screen[] = ['chats', 'map', 'nodes', 'contacts'];
export const BOTTOM_NAV_SCREENS: Screen[] = ['help', 'settings'];

/** Beginner: Backbone/Agents only when configured. Advanced: both are available. */
export function canSeeBackbone(visibility: NavVisibility): boolean {
  return isAdvanced(visibility.mode) || visibility.backboneAvailable;
}

export function canSeeAgents(visibility: NavVisibility): boolean {
  return isAdvanced(visibility.mode) || visibility.agentsAvailable;
}

export function visibleNavScreens(visibility: NavVisibility): Screen[] {
  const screens: Screen[] = [...CORE_NAV_SCREENS];
  if (canSeeBackbone(visibility)) screens.push('backbone');
  if (canSeeAgents(visibility)) screens.push('agents');
  return [...screens, ...BOTTOM_NAV_SCREENS];
}

export function shouldAutoShowOnboarding(input: {
  mode: UiMode;
  demo: boolean;
  demoForced: boolean;
  firstRun: boolean;
  setupComplete: boolean;
  dismissed: boolean;
}): boolean {
  if (input.demoForced) return true;
  if (input.mode !== 'beginner') return false;
  if (input.dismissed || input.setupComplete) return false;
  return input.demo ? false : input.firstRun;
}

/** Copy rules: beginner copy must not leak unexplained protocol jargon (UX spec §6). */
export const BEGINNER_BANNED_TERMS = [
  'SF',
  'spreading factor',
  'bandwidth',
  'PSK',
  'TTL',
  'MTU',
  'OBB',
  'HMAC',
  'SNR',
  'RSSI',
] as const;

export function bannedTermsInCopy(text: string): string[] {
  const haystack = ` ${text.toLowerCase()} `;
  return BEGINNER_BANNED_TERMS.filter((term) => {
    const needle = term.toLowerCase();
    if (needle.length <= 3) {
      // Short acronyms must match as whole words to avoid false positives.
      return new RegExp(`(^|[^a-z0-9])${needle}([^a-z0-9]|$)`, 'i').test(text);
    }
    return haystack.includes(needle);
  });
}
