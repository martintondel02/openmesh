/**
 * Radio link presentation helpers — pure functions only.
 *
 * The settings card, the Shell connection panel, the system banner and the
 * composer warning all derive their wording from this module so the link state
 * is described the same way everywhere and can be unit-tested without React.
 *
 * Data comes from the existing bridge API only (no new endpoints):
 *  - `hello.radios`        — transports the bridge is configured with
 *  - `bridgeStatus.radio`  — aggregate link state, last receive, attempts, signal
 *
 * Link states (bridge radio-link-state contract):
 *  - `connected`    — the serial radio is attached and the companion handshake finished
 *  - `connecting`   — the port is open, the handshake is still in progress
 *  - `reconnecting` — the link was lost and the bridge reopens it with backoff (250 ms → 5 s)
 *  - `disconnected` — the bridge cannot reach the radio
 *  - `virtual`      — no local serial radio (demo/simulator, UDP/TCP LAN link, backbone)
 *
 * The bridge retries a dropped serial link on its own, so `reconnecting` is
 * described as "retrying", never as broken. `virtual` is not an error.
 */

import type {
  ConnectionState,
  HelloData,
  HelloRadio,
  RadioLinkStateValue,
  StatusData,
  TransportMode,
} from '../types/api';
import { profileLabel } from './airtime';

export type RadioLinkState = RadioLinkStateValue | 'unknown';

/** Colour is decoration only; the accompanying text always names the state. */
export type RadioTone = 'emerald' | 'amber' | 'rose' | 'slate';

export const RADIO_LINK_STATES: readonly RadioLinkStateValue[] = [
  'connected',
  'connecting',
  'reconnecting',
  'disconnected',
  'virtual',
] as const;

export function isRadioLinkStateValue(value: unknown): value is RadioLinkStateValue {
  return typeof value === 'string' && (RADIO_LINK_STATES as readonly string[]).includes(value);
}

export interface RadioLink {
  state: RadioLinkState;
  connectedRadios: HelloRadio[];
  disconnectedRadios: HelloRadio[];
  /** The serial radio when there is one, otherwise the first reported radio. */
  primary: HelloRadio | null;
  /** Transports the bridge derives as `connected` (includes `virtual`). */
  connectedCount: number;
  totalCount: number;
  /** Epoch ms of the last frame heard by a serial radio, or null. */
  lastRxMs: number | null;
  /** Reconnect attempts since the last successful attach. */
  reconnectAttempts: number;
}

export interface RadioLinkInput {
  mode: TransportMode;
  connection: ConnectionState;
  hello: HelloData | null | undefined;
  bridgeStatus: StatusData | null | undefined;
}

export interface RadioLinkText {
  /** Short status phrase for a header row. */
  statusLabel: string;
  /** Bridge-reported transport line, e.g. `Serial /dev/ttyACM0 @115200`. */
  deviceLabel: string | null;
  /** Profile and region, e.g. `Medium Fast · EU868`. */
  settingsLabel: string | null;
  /** `N of M radios connected` when the bridge reports more than one transport. */
  radioCountLabel: string | null;
}

export interface RadioOfflineBannerCopy {
  title: string;
  body: string;
  /** Existing help-corpus slug only (`content/help/index.json`). */
  helpTopic: 'troubleshooting';
}

/**
 * Normalise one reported transport into a link state. Older bridges omit
 * `state`; fall back to their `connected` flag and `kind`.
 */
export function transportState(radio: HelloRadio): RadioLinkStateValue {
  if (isRadioLinkStateValue(radio.state)) return radio.state;
  if (!radio.connected) return 'disconnected';
  return radio.kind === 'serial' ? 'connected' : 'virtual';
}

/**
 * Aggregate the bridge-reported status down to one link state.
 * `bridgeStatus.radio.state` is authoritative; before the first status frame
 * the hello transport list is summarised the same way the bridge does it.
 */
export function describeRadioLink(
  hello: HelloData | null | undefined,
  bridgeStatus: StatusData | null | undefined,
): RadioLink {
  const radios = hello?.radios ?? [];
  const connectedRadios = radios.filter((radio) => radio.connected);
  const disconnectedRadios = radios.filter((radio) => !radio.connected);
  const reported = bridgeStatus?.radio;
  const serialRadios = radios.filter((radio) => radio.kind === 'serial');

  let state: RadioLinkState;
  if (reported) {
    state = isRadioLinkStateValue(reported.state)
      ? reported.state
      : reported.connected
        ? 'connected'
        : 'disconnected';
  } else if (hello) {
    state = summarizeHello(radios);
  } else {
    state = 'unknown';
  }

  const rawLastRx = reported?.last_rx_ms;
  const rawAttempts = reported?.reconnect_attempts;

  return {
    state,
    connectedRadios,
    disconnectedRadios,
    primary:
      serialRadios.find((radio) => radio.connected) ??
      connectedRadios[0] ??
      serialRadios[0] ??
      radios[0] ??
      null,
    connectedCount: connectedRadios.length,
    totalCount: radios.length,
    lastRxMs: typeof rawLastRx === 'number' && Number.isFinite(rawLastRx) ? rawLastRx : null,
    reconnectAttempts:
      typeof rawAttempts === 'number' && Number.isFinite(rawAttempts) && rawAttempts > 0 ? Math.floor(rawAttempts) : 0,
  };
}

/** Same precedence as the bridge's `summarize_radio_link` (serial wins, else virtual). */
function summarizeHello(radios: HelloRadio[]): RadioLinkState {
  if (radios.length === 0) return 'disconnected';
  const serialStates = radios.filter((radio) => radio.kind === 'serial').map(transportState);
  if (serialStates.length === 0) return 'virtual';
  if (serialStates.includes('connected')) return 'connected';
  if (serialStates.includes('reconnecting')) return 'reconnecting';
  if (serialStates.includes('connecting')) return 'connecting';
  return 'disconnected';
}

function virtualLabel(link: RadioLink): string {
  switch (link.primary?.kind) {
    case 'udp':
    case 'tcp':
      return 'Virtual radio (LAN link)';
    case 'backbone':
      return 'Virtual radio (backbone)';
    default:
      return 'Virtual radio (simulator)';
  }
}

/** Status text plus the radio/profile/region lines shown next to it. */
export function radioLinkText(link: RadioLink, options: { beginner?: boolean } = {}): RadioLinkText {
  const primary = link.primary;
  const profile = primary
    ? options.beginner
      ? profileLabel(primary.profile)
      : primary.profile.replace(/_/g, ' ')
    : '';

  return {
    statusLabel: statusLabel(link),
    deviceLabel: primary?.label?.trim() || null,
    settingsLabel: primary ? [profile, primary.region].filter(Boolean).join(' · ') : null,
    radioCountLabel: link.totalCount > 1 ? `${link.connectedCount} of ${link.totalCount} radios connected` : null,
  };
}

export function statusLabel(link: RadioLink): string {
  switch (link.state) {
    case 'connected':
      return 'Radio connected';
    case 'connecting':
      return 'Radio connecting…';
    case 'reconnecting':
      return link.reconnectAttempts > 1 ? `Radio reconnecting (attempt ${link.reconnectAttempts})` : 'Radio reconnecting';
    case 'disconnected':
      return 'Radio disconnected';
    case 'virtual':
      return virtualLabel(link);
    default:
      return 'Radio status unknown';
  }
}

/** Colour for dots/icons; the text always states the state too. */
export function radioStateTone(state: RadioLinkState): RadioTone {
  switch (state) {
    case 'connected':
      return 'emerald';
    case 'connecting':
    case 'reconnecting':
      return 'amber';
    case 'disconnected':
      return 'rose';
    default:
      return 'slate';
  }
}

export function radioLinkTone(link: RadioLink): RadioTone {
  return radioStateTone(link.state);
}

/** Label for one transport in the multi-radio list / settings radio list. */
export function transportStateLabel(radio: HelloRadio): string {
  switch (transportState(radio)) {
    case 'connected':
      return 'Connected';
    case 'connecting':
      return 'Connecting';
    case 'reconnecting':
      return 'Reconnecting';
    case 'virtual':
      return 'Virtual';
    default:
      return 'Not connected';
  }
}

/** True when the state means messages cannot currently leave over a radio. */
export function isRadioLinkUnavailable(state: RadioLinkState): boolean {
  return state === 'connecting' || state === 'reconnecting' || state === 'disconnected';
}

/**
 * Human age of the last frame heard by the radio: `just now`, `12 s ago`,
 * `3 min ago`, `2 h ago`. `null` when the bridge has not reported a frame yet.
 */
export function formatLastFrameAge(lastRxMs: number | null | undefined, nowMs: number): string | null {
  if (typeof lastRxMs !== 'number' || !Number.isFinite(lastRxMs)) return null;
  const delta = Math.max(0, nowMs - lastRxMs);
  if (delta < 1_000) return 'just now';
  if (delta < 60_000) return `${Math.round(delta / 1_000)} s ago`;
  if (delta < 3_600_000) return `${Math.round(delta / 60_000)} min ago`;
  return `${Math.round(delta / 3_600_000)} h ago`;
}

/** `Reconnect attempt N` while the bridge retries more than once; otherwise null. */
export function reconnectAttemptLabel(link: RadioLink): string | null {
  if (link.state !== 'reconnecting' || link.reconnectAttempts <= 1) return null;
  return `Reconnect attempt ${link.reconnectAttempts}`;
}

/**
 * Beginner guidance shown under the status. `null` while connected.
 * The bridge retries by itself, so the user is not asked to restart anything
 * unless the link is fully disconnected.
 */
export function radioGuidance(link: RadioLink): string | null {
  switch (link.state) {
    case 'connected':
      return null;
    case 'virtual':
      return 'No hardware radio attached — the bridge is using a virtual radio. This is normal for demo, simulator, local network and backbone links.';
    case 'connecting':
      return 'The bridge is opening the radio connection. Messages wait here and go out as soon as the link is ready.';
    case 'reconnecting':
      return 'The bridge keeps trying to reconnect on its own. Check the USB cable is plugged in at both ends, the radio has power and the antenna is attached. Messages wait here and go out when the radio is back.';
    case 'disconnected':
      return 'The bridge cannot open the radio. Check the USB cable is plugged in at both ends, the radio has power and the antenna is attached, then restart the bridge if the radio stays offline.';
    default:
      return 'The bridge has not reported a radio yet. Check that the radio is plugged into the computer that runs the bridge.';
  }
}

/** True only for a live panel whose bridge is connected but the radio is not usable. */
export function isLiveRadioOffline(input: RadioLinkInput): boolean {
  if (input.mode !== 'live' || input.connection !== 'connected') return false;
  return isRadioLinkUnavailable(describeRadioLink(input.hello, input.bridgeStatus).state);
}

export function shouldShowRadioOfflineBanner(input: RadioLinkInput): boolean {
  return isLiveRadioOffline(input);
}

export function shouldShowComposerRadioWarning(input: RadioLinkInput): boolean {
  return isLiveRadioOffline(input);
}

/**
 * Banner copy for the current unavailable state. `null` for connected,
 * virtual and unknown (no claim is made before a state is known).
 */
export function radioOfflineBanner(link: RadioLink): RadioOfflineBannerCopy | null {
  switch (link.state) {
    case 'reconnecting':
      return {
        title:
          link.reconnectAttempts > 1
            ? `Radio reconnecting — the bridge keeps retrying (attempt ${link.reconnectAttempts})`
            : 'Radio reconnecting — the bridge keeps retrying',
        body: 'Messages wait here and go out automatically when the radio is back. Check the USB cable, the radio’s power and the antenna.',
        helpTopic: 'troubleshooting',
      };
    case 'connecting':
      return {
        title: 'Radio connecting…',
        body: 'The bridge is opening the radio link. Messages wait here and go out as soon as it is ready.',
        helpTopic: 'troubleshooting',
      };
    case 'disconnected':
      return {
        title: 'Radio disconnected',
        body: 'The bridge cannot reach the radio right now. Messages wait here. Check the USB cable, the radio’s power and the antenna.',
        helpTopic: 'troubleshooting',
      };
    default:
      return null;
  }
}

/** Inline composer warning for the current unavailable state; sending is never blocked. */
export function composerRadioWarning(link: RadioLink): string | null {
  switch (link.state) {
    case 'connecting':
      return COMPOSER_RADIO_CONNECTING_WARNING;
    case 'reconnecting':
      return COMPOSER_RADIO_RECONNECTING_WARNING;
    case 'disconnected':
      return COMPOSER_RADIO_OFFLINE_WARNING;
    default:
      return null;
  }
}

/** One call for the composer: live + bridge connected + radio unavailable. */
export function composerRadioNotice(input: RadioLinkInput): string | null {
  if (!isLiveRadioOffline(input)) return null;
  return composerRadioWarning(describeRadioLink(input.hello, input.bridgeStatus));
}

export const COMPOSER_RADIO_OFFLINE_WARNING = 'Radio offline — your message will queue until it reconnects.';
export const COMPOSER_RADIO_RECONNECTING_WARNING = 'Radio reconnecting — your message will queue until it reconnects.';
export const COMPOSER_RADIO_CONNECTING_WARNING = 'Radio connecting — your message will queue until the link is ready.';
