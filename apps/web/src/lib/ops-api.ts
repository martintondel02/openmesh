/**
 * REST client for v0.7 remote operations.
 *
 * Normative: docs/design/remote-operations.md §2 (gateways) and §3 (backbone links).
 * Remote MCP has no REST configuration endpoint in v0.7 — the UI renders guidance plus
 * an optional status object and never invents a route.
 */
import { buildHttpBaseUrl } from './ws';
import {
  normalizeBackboneLinksResponse,
  normalizeGatewayConnector,
  normalizeGatewaysResponse,
  normalizeGatewayTestResult,
} from './ops';
import type {
  BackboneLink,
  BackboneLinkInput,
  GatewayConfigInput,
  GatewayConnector,
  GatewayTestResult,
} from '../types/ops';

export class OpsApiError extends Error {
  readonly status: number;
  readonly code: string | null;

  constructor(message: string, status = 0, code: string | null = null) {
    super(message);
    this.name = 'OpsApiError';
    this.status = status;
    this.code = code;
  }
}

export interface OpsRestOptions {
  baseUrl: string;
  token?: string | null;
  fetchImpl?: typeof fetch;
  timeoutMs?: number;
}

export function buildOpsApiUrl(baseUrl: string, path: string): string {
  const base = buildHttpBaseUrl(baseUrl).replace(/\/+$/, '');
  return `${base}${path.startsWith('/') ? path : `/${path}`}`;
}

export class OpsRestClient {
  private readonly baseUrl: string;
  private readonly token: string | null;
  private readonly fetchImpl: typeof fetch;
  private readonly timeoutMs: number;

  constructor(options: OpsRestOptions) {
    this.baseUrl = options.baseUrl;
    this.token = options.token ?? null;
    this.fetchImpl = options.fetchImpl ?? globalThis.fetch.bind(globalThis);
    this.timeoutMs = options.timeoutMs ?? 8000;
  }

  private async request(path: string, init: RequestInit = {}): Promise<Response> {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeoutMs);
    try {
      const response = await this.fetchImpl(buildOpsApiUrl(this.baseUrl, path), {
        ...init,
        headers: {
          Accept: 'application/json',
          'X-OpenMesh-Client': 'web',
          ...(this.token ? { Authorization: `Bearer ${this.token}` } : {}),
          ...(init.body ? { 'Content-Type': 'application/json' } : {}),
          ...(init.headers ?? {}),
        },
        signal: controller.signal,
      });
      if (!response.ok) {
        let code: string | null = null;
        let message = `Remote operations API error (${response.status})`;
        try {
          const payload = (await response.json()) as { error?: string; message?: string; code?: string };
          code = payload.code ?? payload.error ?? null;
          message = payload.message ?? payload.error ?? message;
        } catch {
          // Non-JSON error body (older bridge): keep the status-based message.
        }
        throw new OpsApiError(message, response.status, code);
      }
      return response;
    } catch (error) {
      if (error instanceof OpsApiError) throw error;
      if (error instanceof DOMException && error.name === 'AbortError') {
        throw new OpsApiError('The bridge did not answer in time. Check that it is running, then retry.', 0, 'timeout');
      }
      throw new OpsApiError(error instanceof Error ? error.message : 'Could not reach the bridge.', 0, 'network');
    } finally {
      clearTimeout(timer);
    }
  }

  /** GET /api/gateways → normalized connectors (redacted). */
  async listGateways(): Promise<GatewayConnector[]> {
    const response = await this.request('/api/gateways');
    return normalizeGatewaysResponse(await response.json().catch(() => ({})));
  }

  /** PUT /api/gateways/{id} — upsert without secret values. */
  async putGateway(connector: GatewayConfigInput): Promise<GatewayConnector> {
    const response = await this.request(`/api/gateways/${encodeURIComponent(connector.id)}`, {
      method: 'PUT',
      body: JSON.stringify({
        kind: connector.kind,
        enabled: connector.enabled,
        outbound: connector.outbound,
        inbound: connector.inbound,
      }),
    });
    const payload = (await response.json().catch(() => null)) as unknown;
    const record = payload && typeof payload === 'object' && !Array.isArray(payload) ? (payload as Record<string, unknown>) : null;
    const normalized = normalizeGatewayConnector(record?.connector ?? payload ?? connector);
    return normalized ?? {
      ...connector,
      status: {
        last_success_ms: null,
        last_failure_ms: null,
        last_error: null,
        retries: 0,
        out_secret_present: Boolean(connector.outbound.secret_env),
        in_secret_present: Boolean(connector.inbound.secret_env),
      },
    };
  }

  /** DELETE /api/gateways/{id}. */
  async deleteGateway(id: string): Promise<void> {
    await this.request(`/api/gateways/${encodeURIComponent(id)}`, { method: 'DELETE' });
  }

  /** POST /api/gateways/{id}/test — sends a signed test payload. */
  async testGateway(id: string): Promise<GatewayTestResult> {
    const response = await this.request(`/api/gateways/${encodeURIComponent(id)}/test`, {
      method: 'POST',
      body: JSON.stringify({}),
    });
    return normalizeGatewayTestResult(await response.json().catch(() => ({})));
  }

  /** GET /api/backbone/links → normalized links (`[]` on 404 handled by caller). */
  async listBackboneLinks(): Promise<BackboneLink[]> {
    const response = await this.request('/api/backbone/links');
    return normalizeBackboneLinksResponse(await response.json().catch(() => ({})));
  }

  /** PUT /api/backbone/links/{name}. */
  async putBackboneLink(input: BackboneLinkInput): Promise<BackboneLink> {
    const response = await this.request(`/api/backbone/links/${encodeURIComponent(input.name)}`, {
      method: 'PUT',
      body: JSON.stringify({ url: input.url, enabled: input.enabled }),
    });
    const payload = (await response.json().catch(() => null)) as unknown;
    const record = payload && typeof payload === 'object' && !Array.isArray(payload) ? (payload as Record<string, unknown>) : null;
    const normalized = normalizeBackboneLinksResponse(record?.link ? [record.link] : record?.links ?? payload);
    return normalized[0] ?? {
      name: input.name,
      url: input.url,
      mode: 'client',
      enabled: input.enabled,
      connected: false,
      last_error: null,
      stats: null,
    };
  }

  /** DELETE /api/backbone/links/{name}. */
  async deleteBackboneLink(name: string): Promise<void> {
    await this.request(`/api/backbone/links/${encodeURIComponent(name)}`, { method: 'DELETE' });
  }
}
