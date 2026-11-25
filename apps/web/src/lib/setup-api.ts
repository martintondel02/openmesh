/** REST client for the v0.3 setup endpoints (UX spec §5). */
import type { QuickSetupRequest, QuickSetupResult, SetupRecommendation, SetupState } from '../types/setup';
import { normalizeQuickSetupResult, normalizeRecommended, normalizeSetupState } from './setup';

export class SetupApiError extends Error {
  readonly status: number;
  readonly code: string | null;

  constructor(message: string, status = 0, code: string | null = null) {
    super(message);
    this.name = 'SetupApiError';
    this.status = status;
    this.code = code;
  }
}

export interface SetupClientOptions {
  baseUrl: string;
  token?: string | null;
  fetchImpl?: typeof fetch;
  timeoutMs?: number;
}

function joinUrl(baseUrl: string, path: string): string {
  return `${baseUrl.replace(/\/$/, '')}/${path.replace(/^\//, '')}`;
}

export class SetupRestClient {
  private readonly baseUrl: string;
  private readonly token: string | null;
  private readonly fetchImpl: typeof fetch;
  private readonly timeoutMs: number;

  constructor(options: SetupClientOptions) {
    this.baseUrl = options.baseUrl;
    this.token = options.token ?? null;
    this.fetchImpl = options.fetchImpl ?? fetch;
    this.timeoutMs = options.timeoutMs ?? 8000;
  }

  private async request(path: string, init: RequestInit = {}): Promise<unknown> {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeoutMs);
    try {
      const headers: Record<string, string> = {
        Accept: 'application/json',
        'X-OpenMesh-Client': 'web',
        ...(init.body ? { 'Content-Type': 'application/json' } : {}),
        ...(this.token ? { Authorization: `Bearer ${this.token}` } : {}),
      };
      const response = await this.fetchImpl(joinUrl(this.baseUrl, path), {
        ...init,
        headers,
        signal: controller.signal,
      });
      const payload = (await response.json().catch(() => null)) as unknown;
      if (!response.ok) {
        const record = payload && typeof payload === 'object' ? (payload as Record<string, unknown>) : {};
        const message =
          (typeof record.message === 'string' && record.message) ||
          (typeof record.error === 'string' && record.error) ||
          `Setup request failed (${response.status})`;
        throw new SetupApiError(message, response.status, typeof record.error === 'string' ? record.error : null);
      }
      return payload;
    } catch (cause) {
      if (cause instanceof SetupApiError) throw cause;
      if (cause instanceof DOMException && cause.name === 'AbortError') {
        throw new SetupApiError('The bridge did not answer in time. Check that it is running, then try again.', 0, 'timeout');
      }
      throw new SetupApiError(cause instanceof Error ? cause.message : 'Could not reach the bridge.', 0, 'network');
    } finally {
      clearTimeout(timer);
    }
  }

  async getState(): Promise<SetupState> {
    return normalizeSetupState(await this.request('/api/setup/state'));
  }

  async quickSetup(request: QuickSetupRequest): Promise<QuickSetupResult> {
    return normalizeQuickSetupResult(
      await this.request('/api/setup/quick', { method: 'POST', body: JSON.stringify(request) }),
    );
  }

  async getRecommendation(country: string, role: string): Promise<SetupRecommendation> {
    const query = new URLSearchParams({ country, role }).toString();
    const payload = await this.request(`/api/setup/recommend?${query}`);
    return normalizeRecommended(payload) as SetupRecommendation;
  }
}
