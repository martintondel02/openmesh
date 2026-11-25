/** REST PATCH /api/config client (v0.5 regulatory override wiring). */
import type { RegulatoryPatchBody } from './regulatory';

export class ConfigApiError extends Error {
  readonly status: number;
  readonly code: string | null;
  /** The contract's full override warning, when the bridge includes it (422). */
  readonly warning: string | null;

  constructor(message: string, status = 0, code: string | null = null, warning: string | null = null) {
    super(message);
    this.name = 'ConfigApiError';
    this.status = status;
    this.code = code;
    this.warning = warning;
  }
}

export interface PatchConfigOptions {
  baseUrl: string;
  token?: string | null;
  /** Body for PATCH /api/config; regulatory_override is snake_case per the contract. */
  patch: RegulatoryPatchBody | Record<string, unknown>;
  fetchImpl?: typeof fetch;
  timeoutMs?: number;
}

function joinUrl(baseUrl: string, path: string): string {
  return `${baseUrl.replace(/\/$/, '')}/${path.replace(/^\//, '')}`;
}

/** PATCH /api/config with the bridge bearer token and web attribution header. */
export async function patchBridgeConfig(options: PatchConfigOptions): Promise<unknown> {
  const fetchImpl = options.fetchImpl ?? fetch;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), options.timeoutMs ?? 8000);
  try {
    const response = await fetchImpl(joinUrl(options.baseUrl, '/api/config'), {
      method: 'PATCH',
      headers: {
        Accept: 'application/json',
        'Content-Type': 'application/json',
        'X-OpenMesh-Client': 'web',
        ...(options.token ? { Authorization: `Bearer ${options.token}` } : {}),
      },
      body: JSON.stringify(options.patch),
      signal: controller.signal,
    });
    const payload = (await response.json().catch(() => null)) as unknown;
    if (!response.ok) {
      const record = payload && typeof payload === 'object' ? (payload as Record<string, unknown>) : {};
      const message =
        (typeof record.message === 'string' && record.message) ||
        (typeof record.error === 'string' && record.error) ||
        `The bridge rejected the config change (${response.status}).`;
      throw new ConfigApiError(
        message,
        response.status,
        typeof record.error === 'string' ? record.error : null,
        typeof record.warning === 'string' ? record.warning : null,
      );
    }
    return payload;
  } catch (error) {
    if (error instanceof ConfigApiError) throw error;
    if (error instanceof DOMException && error.name === 'AbortError') {
      throw new ConfigApiError('The bridge did not answer in time. Check that it is running, then try again.', 0, 'timeout');
    }
    throw new ConfigApiError(error instanceof Error ? error.message : 'Could not reach the bridge.', 0, 'network');
  } finally {
    clearTimeout(timer);
  }
}
