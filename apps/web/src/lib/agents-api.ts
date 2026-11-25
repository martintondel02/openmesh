/** REST client for agent/activity endpoints (AI-NATIVE.md §2). */
import type { ActivityEntry } from '../types/agents';
import { normalizeActivityResponse } from './agents';

export class AgentsApiError extends Error {
  readonly status: number;
  readonly code: string | null;

  constructor(message: string, status = 0, code: string | null = null) {
    super(message);
    this.name = 'AgentsApiError';
    this.status = status;
    this.code = code;
  }
}

export interface AgentsClientOptions {
  baseUrl: string;
  token?: string | null;
  fetchImpl?: typeof fetch;
  timeoutMs?: number;
}

function joinUrl(baseUrl: string, path: string): string {
  return `${baseUrl.replace(/\/$/, '')}/${path.replace(/^\//, '')}`;
}

export class AgentsRestClient {
  private readonly baseUrl: string;
  private readonly token: string | null;
  private readonly fetchImpl: typeof fetch;
  private readonly timeoutMs: number;

  constructor(options: AgentsClientOptions) {
    this.baseUrl = options.baseUrl;
    this.token = options.token ?? null;
    this.fetchImpl = options.fetchImpl ?? fetch;
    this.timeoutMs = options.timeoutMs ?? 8000;
  }

  private async request(path: string, init: RequestInit = {}): Promise<unknown> {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeoutMs);
    try {
      const response = await this.fetchImpl(joinUrl(this.baseUrl, path), {
        ...init,
        headers: {
          Accept: 'application/json',
          'X-OpenMesh-Client': 'web',
          ...(init.body ? { 'Content-Type': 'application/json' } : {}),
          ...(this.token ? { Authorization: `Bearer ${this.token}` } : {}),
        },
        signal: controller.signal,
      });
      const payload = (await response.json().catch(() => null)) as unknown;
      if (!response.ok) {
        const record = payload && typeof payload === 'object' ? (payload as Record<string, unknown>) : {};
        const message =
          (typeof record.message === 'string' && record.message) ||
          (typeof record.error === 'string' && record.error) ||
          `Agent request failed (${response.status})`;
        throw new AgentsApiError(message, response.status, typeof record.error === 'string' ? record.error : null);
      }
      return payload;
    } catch (cause) {
      if (cause instanceof AgentsApiError) throw cause;
      if (cause instanceof DOMException && cause.name === 'AbortError') {
        throw new AgentsApiError('The bridge did not answer in time. Check that it is running, then retry.', 0, 'timeout');
      }
      throw new AgentsApiError(cause instanceof Error ? cause.message : 'Could not reach the bridge.', 0, 'network');
    } finally {
      clearTimeout(timer);
    }
  }

  async getActivity(limit = 100): Promise<ActivityEntry[]> {
    return normalizeActivityResponse(await this.request(`/api/activity?limit=${Math.max(1, Math.min(100, Math.floor(limit)))}`));
  }

  /** Full payload with capacity / agent_connected / sources metadata. */
  async getActivityPayload(limit = 100): Promise<unknown> {
    return this.request(`/api/activity?limit=${Math.max(1, Math.min(100, Math.floor(limit)))}`);
  }

}
