/**
 * REST client for the Backbone control surface.
 *
 * Normative: BACKBONE.md §7.1 and WEBSOCKET-API.md §8.
 *   GET    /api/backbone
 *   GET    /api/backbone/peers
 *   POST   /api/backbone/config
 *   POST   /api/backbone/token/rotate
 *   DELETE /api/backbone/peers/:node_id
 *   GET    /api/backbone/setup
 *   GET    /api/backbone/qr.svg
 *
 * All endpoints require the bridge bearer token. `GET /api/backbone` returns
 * `{enabled:false}` (HTTP 200) when the bridge is not in backbone mode.
 */

import type {
  BackboneConfig,
  BackboneConfigPatch,
  BackboneConfigResponse,
  BackbonePeer,
  BackboneSetup,
} from '../types/backbone';
import { normalizeBackboneConfig } from './backbone';
import { buildHttpBaseUrl } from './ws';

export interface BackboneRestOptions {
  /** Bridge base URL, e.g. `http://127.0.0.1:8787`. */
  baseUrl: string;
  /** Bridge bearer token (from Settings). */
  token?: string | null;
  /** Injected in tests; defaults to global fetch. */
  fetchImpl?: typeof fetch;
  timeoutMs?: number;
}

export class BackboneApiError extends Error {
  readonly status: number;
  readonly code: string | null;

  constructor(message: string, status: number, code: string | null = null) {
    super(message);
    this.name = 'BackboneApiError';
    this.status = status;
    this.code = code;
  }
}

/** Join the bridge base URL and an API path without duplicate slashes. */
export function buildBackboneApiUrl(baseUrl: string, path: string): string {
  const base = buildHttpBaseUrl(baseUrl).replace(/\/+$/, '');
  const suffix = path.startsWith('/') ? path : `/${path}`;
  return `${base}${suffix}`;
}

export function backboneRequestHeaders(token: string | null | undefined, hasBody = false): Record<string, string> {
  const headers: Record<string, string> = { Accept: 'application/json' };
  if (token) headers.Authorization = `Bearer ${token}`;
  if (hasBody) headers['Content-Type'] = 'application/json';
  return headers;
}

export class BackboneRestClient {
  private readonly baseUrl: string;
  private readonly token: string | null;
  private readonly fetchImpl: typeof fetch;
  private readonly timeoutMs: number;

  constructor(options: BackboneRestOptions) {
    this.baseUrl = options.baseUrl;
    this.token = options.token ?? null;
    this.fetchImpl = options.fetchImpl ?? globalThis.fetch.bind(globalThis);
    this.timeoutMs = options.timeoutMs ?? 5000;
  }

  private async request(path: string, init: RequestInit = {}): Promise<Response> {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeoutMs);
    try {
      const response = await this.fetchImpl(buildBackboneApiUrl(this.baseUrl, path), {
        ...init,
        headers: {
          ...backboneRequestHeaders(this.token, Boolean(init.body)),
          ...(init.headers ?? {}),
        },
        signal: controller.signal,
      });
      if (!response.ok) {
        let code: string | null = null;
        let message = `Backbone API error (${response.status})`;
        try {
          const payload = (await response.json()) as { error?: string; message?: string; code?: string };
          code = payload.code ?? null;
          message = payload.error ?? payload.message ?? message;
        } catch {
          // Non-JSON error body: keep the status message.
        }
        throw new BackboneApiError(message, response.status, code);
      }
      return response;
    } finally {
      clearTimeout(timer);
    }
  }

  /** GET /api/backbone — config + live totals; normalizes `{enabled:false}`. */
  async getConfig(): Promise<BackboneConfig> {
    const response = await this.request('/api/backbone');
    return normalizeBackboneConfig(await response.json());
  }

  /** GET /api/backbone/peers — connected peer list ([] when disabled/not running). */
  async getPeers(): Promise<BackbonePeer[]> {
    try {
      const response = await this.request('/api/backbone/peers');
      const payload = (await response.json()) as unknown;
      if (Array.isArray(payload)) return payload as BackbonePeer[];
      // The bridge returns {"enabled":false,"peers":[]} when not configured.
      if (payload && typeof payload === 'object' && Array.isArray((payload as { peers?: unknown }).peers)) {
        return (payload as { peers: BackbonePeer[] }).peers;
      }
      return [];
    } catch (error) {
      if (error instanceof BackboneApiError && (error.status === 404 || error.status === 409)) return [];
      throw error;
    }
  }

  /** POST /api/backbone/config — partial live update. */
  async updateConfig(patch: BackboneConfigPatch): Promise<BackboneConfigResponse> {
    const response = await this.request('/api/backbone/config', {
      method: 'POST',
      body: JSON.stringify(patch),
    });
    const payload = (await response.json()) as unknown;
    const normalized = normalizeBackboneConfig(payload);
    const restarted = Boolean(payload && typeof payload === 'object' && (payload as { restarted?: boolean }).restarted);
    return { ...normalized, restarted };
  }

  /** POST /api/backbone/token/rotate — new token returned exactly once. */
  async rotateToken(): Promise<string> {
    const response = await this.request('/api/backbone/token/rotate', { method: 'POST', body: JSON.stringify({}) });
    const payload = (await response.json()) as { token?: string };
    if (!payload.token) throw new BackboneApiError('The bridge did not return a token', 500);
    return payload.token;
  }

  /** DELETE /api/backbone/peers/:node_id — disconnect one peer. */
  async kickPeer(nodeId: string): Promise<boolean> {
    const response = await this.request(`/api/backbone/peers/${encodeURIComponent(nodeId)}`, { method: 'DELETE' });
    const payload = (await response.json().catch(() => ({}))) as { disconnected?: boolean };
    return payload.disconnected !== false;
  }

  /** GET /api/backbone/setup — onboarding payload (§8). */
  async getSetup(): Promise<BackboneSetup> {
    const response = await this.request('/api/backbone/setup');
    return (await response.json()) as BackboneSetup;
  }

  /** GET /api/backbone/qr.svg — QR markup of the current connection string. */
  async getQrSvg(): Promise<string> {
    const response = await this.request('/api/backbone/qr.svg', {
      headers: { Accept: 'image/svg+xml' },
    });
    return response.text();
  }
}
