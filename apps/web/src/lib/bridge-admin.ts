/**
 * Authenticated bridge admin operations (task-40 contract).
 *
 * `POST /api/admin/reset` is the only data-wipe entry point:
 *   request  { "confirm": "WIPE", "new_identity": true }
 *   200      { reset, identity_changed, old_node_id, node_id, counts, public_channel }
 *   400      confirmation_required / bad JSON, 401 unauthorized, 500 internal
 *
 * Resetting is synchronous in the bridge. The bridge keeps its API token and radio config but
 * clears all mesh data and (by default) regenerates its node identity, so every contact and QR
 * code referencing the old node id becomes invalid. The web client never calls this against the
 * live user bridge without explicit approval; tests use an isolated bridge.
 */
import { buildHttpBaseUrl } from './ws';

export interface BridgeConnectionSettings {
  bridgeUrl: string;
  token: string;
}

export interface BridgeResetCounts {
  messages: number;
  contacts: number;
  channels: number;
  nodes: number;
  attachments: number;
  activity: number;
}

export interface BridgeResetPublicChannel {
  channel_id: string;
  name: string;
  psk_set?: boolean;
  public?: boolean;
}

export interface BridgeResetResult {
  reset: boolean;
  identity_changed: boolean;
  old_node_id?: string;
  node_id?: string;
  counts: BridgeResetCounts;
  public_channel?: BridgeResetPublicChannel;
}

export interface BridgeResetOptions {
  /** Defaults to true: regenerate the bridge node identity (new node id/QR). */
  newIdentity?: boolean;
  signal?: AbortSignal;
}

export class BridgeResetError extends Error {
  readonly status: number;
  readonly code: string | null;
  constructor(message: string, status: number, code: string | null = null) {
    super(message);
    this.name = 'BridgeResetError';
    this.status = status;
    this.code = code;
  }
}

interface BridgeErrorBody {
  error?: string;
  code?: string;
  message?: string;
}

/** POST the confirmed reset request; throws BridgeResetError on any non-success response. */
export async function requestBridgeReset(
  settings: BridgeConnectionSettings,
  options: BridgeResetOptions = {},
): Promise<BridgeResetResult> {
  const controller = options.signal ? null : new AbortController();
  const timer = controller ? setTimeout(() => controller.abort(), 20_000) : null;
  try {
    const response = await fetch(`${buildHttpBaseUrl(settings.bridgeUrl)}/api/admin/reset`, {
      method: 'POST',
      signal: options.signal ?? controller?.signal,
      headers: {
        'Content-Type': 'application/json',
        Accept: 'application/json',
        'X-OpenMesh-Client': 'web',
        ...(settings.token ? { Authorization: `Bearer ${settings.token}` } : {}),
      },
      body: JSON.stringify({ confirm: 'WIPE', new_identity: options.newIdentity ?? true }),
    });

    if (!response.ok) {
      let body: BridgeErrorBody | null = null;
      try {
        body = (await response.json()) as BridgeErrorBody;
      } catch {
        body = null;
      }
      const code = body?.error ?? body?.code ?? null;
      const message =
        body?.message ??
        (response.status === 401
          ? 'The bridge rejected the token. Check the bearer token in Settings.'
          : response.status === 400
            ? 'The bridge requires the exact confirmation WIPE.'
            : `The bridge could not reset (HTTP ${response.status}).`);
      throw new BridgeResetError(message, response.status, code);
    }

    const body = (await response.json().catch(() => null)) as BridgeResetResult | null;
    if (!body || body.reset !== true || !body.counts) {
      throw new BridgeResetError('The bridge returned an unexpected reset response.', response.status, 'invalid_response');
    }
    return body;
  } catch (cause) {
    if (cause instanceof BridgeResetError) throw cause;
    if (cause instanceof DOMException && cause.name === 'AbortError') {
      throw new BridgeResetError('The bridge reset request timed out.', 0, 'timeout');
    }
    throw new BridgeResetError(
      `Could not reach the bridge: ${cause instanceof Error ? cause.message : String(cause)}`,
      0,
      'network',
    );
  } finally {
    if (timer) clearTimeout(timer);
  }
}

/** Compact human-readable summary of the cleared counts. */
export function formatBridgeResetCounts(counts: BridgeResetCounts): string {
  const parts = [
    `${counts.messages} ${counts.messages === 1 ? 'message' : 'messages'}`,
    `${counts.contacts} ${counts.contacts === 1 ? 'contact' : 'contacts'}`,
    `${counts.channels} ${counts.channels === 1 ? 'channel' : 'channels'}`,
    `${counts.nodes} ${counts.nodes === 1 ? 'node' : 'nodes'}`,
    `${counts.attachments} ${counts.attachments === 1 ? 'attachment' : 'attachments'}`,
  ];
  return parts.join(' · ');
}
