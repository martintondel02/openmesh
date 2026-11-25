/**
 * Derived view models for the Backbone control panel.
 * Pure functions over the store slices, so the panel and tests share one source.
 */

import type { AppState } from './app';
import type { BackboneConfig, BackbonePeer, BackboneSetup, BackboneStatus } from '../types/backbone';
import {
  backbonePill,
  buildBackboneConnectionString,
  buildFallbackSetup,
  formatEndpoint,
  formatListenAddress,
  isBackboneRelevant,
  sortPeers,
  sumPeerTraffic,
  withConnectionStringSetup,
} from '../lib/backbone';
import type { BackbonePill } from '../lib/backbone';

export interface BackboneStats {
  peers: number;
  maxPeers: number;
  framesIn: number;
  framesOut: number;
  bytesIn: number;
  bytesOut: number;
  throughputInBps: number;
  throughputOutBps: number;
  storeForwardEnvelopes: number;
  authFailures: number;
  uptimeS: number;
  running: boolean;
  lastError: string | null;
}

type BackboneSlice = Pick<
  AppState,
  | 'mode'
  | 'hello'
  | 'ownNodeId'
  | 'backboneConfig'
  | 'backboneStatus'
  | 'backbonePeers'
  | 'backboneSetup'
  | 'backboneSessionToken'
>;

/** True when the Backbone nav item should be visible (demo always shows it). */
export function selectBackboneAvailable(state: Pick<AppState, 'mode' | 'hello' | 'backboneConfig'>): boolean {
  return isBackboneRelevant({
    features: state.hello?.features ?? null,
    config: state.backboneConfig,
    demo: state.mode === 'demo',
  });
}

/** Stats card model, falling back to config totals / peer sums before the first event. */
export function selectBackboneStats(state: BackboneSlice): BackboneStats {
  const { backboneConfig, backboneStatus, backbonePeers } = state;
  const peerSums = sumPeerTraffic(backbonePeers);
  const totals = backboneConfig?.totals;
  const status = backboneStatus;
  return {
    peers: status?.peers ?? backbonePeers.length,
    maxPeers: backboneConfig?.max_peers ?? 32,
    framesIn: status?.frames_in ?? totals?.frames_in ?? peerSums.frames_in,
    framesOut: status?.frames_out ?? totals?.frames_out ?? peerSums.frames_out,
    bytesIn: status?.bytes_in ?? totals?.bytes_in ?? peerSums.bytes_in,
    bytesOut: status?.bytes_out ?? totals?.bytes_out ?? peerSums.bytes_out,
    throughputInBps: status?.throughput_in_bps ?? 0,
    throughputOutBps: status?.throughput_out_bps ?? 0,
    storeForwardEnvelopes: status?.store_forward_envelopes ?? 0,
    authFailures: status?.auth_failures ?? totals?.auth_failures ?? 0,
    uptimeS: status?.uptime_s ?? totals?.uptime_s ?? 0,
    running: status?.running ?? backboneConfig?.running ?? false,
    lastError: status?.last_error ?? null,
  };
}

/** Status pill + endpoint for the panel header. */
export function selectBackboneHeader(state: BackboneSlice): {
  pill: BackbonePill;
  endpoint: string;
  listen: string;
  uptimeS: number;
  maxPeers: number;
} {
  const stats = selectBackboneStats(state);
  return {
    pill: backbonePill(state.backboneConfig, state.backboneStatus),
    endpoint: formatEndpoint(state.backboneConfig),
    listen: formatListenAddress(state.backboneConfig),
    uptimeS: stats.uptimeS,
    maxPeers: stats.maxPeers,
  };
}

export function selectBackbonePeers(state: Pick<AppState, 'backbonePeers'>): BackbonePeer[] {
  return sortPeers(state.backbonePeers);
}

/**
 * Connection string for copy/QR. Uses the token known from rotation/setup when
 * available; returns null when the bridge is in token mode and the token is not
 * in client memory (the setup endpoint is the fallback).
 */
export function selectBackboneConnectionString(state: BackboneSlice): string | null {
  const config = state.backboneConfig;
  if (!config) return state.backboneSetup?.connection_string ?? null;
  if (config.auth === 'token' && !state.backboneSessionToken) {
    return state.backboneSetup?.connection_string ?? null;
  }
  return buildBackboneConnectionString({
    host: config.public_host || config.bind,
    port: config.port,
    token: config.auth === 'token' ? state.backboneSessionToken : null,
    nodeId: state.ownNodeId,
    name: config.name,
  });
}

/**
 * Setup payload for the wizard: the bridge-provided one when present, otherwise
 * an honest local template, with the connection string kept in sync with the
 * current config/token.
 */
export function selectBackboneSetup(state: BackboneSlice): BackboneSetup {
  const connectionString = selectBackboneConnectionString(state) ?? '';
  const base =
    state.backboneSetup ??
    buildFallbackSetup({
      config: state.backboneConfig,
      connectionString,
    });
  return withConnectionStringSetup(base, connectionString);
}

/** Whether the panel is showing seeded demo data. */
export function selectBackboneDemo(state: Pick<AppState, 'mode'>): boolean {
  return state.mode === 'demo';
}

/** Dirty check helper for the controls form (kept here for symmetry with tests). */
export function selectBackboneConfigOrNull(state: Pick<AppState, 'backboneConfig'>): BackboneConfig | null {
  return state.backboneConfig;
}

/** Convenience for components that need raw status. */
export function selectBackboneStatusOrNull(state: Pick<AppState, 'backboneStatus'>): BackboneStatus | null {
  return state.backboneStatus;
}
