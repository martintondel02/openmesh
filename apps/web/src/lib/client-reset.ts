/**
 * Local "reset this client" state clearing.
 *
 * This is deliberately separate from the bridge wipe: the web client holds no node identity —
 * the bridge does. Resetting the browser only clears its cached connection settings, token,
 * drafts, demo flags, read markers and other origin state, then reloads into first-run mode.
 * The bridge keeps its data and node identity unless the authenticated bridge reset is used.
 */

export interface ClearableStorage {
  clear: () => void;
}

export interface ClientResetStorages {
  local: ClearableStorage;
  session: ClearableStorage;
}

/** Clear every key from both web storages; a failing storage provider is not fatal. */
export function clearClientLocalState(storages: ClientResetStorages): void {
  try {
    storages.local.clear();
  } catch {
    // Storage may be unavailable (private mode / Tauri sandbox); the reload still starts fresh.
  }
  try {
    storages.session.clear();
  } catch {
    // ignore
  }
}

/** Root URL to reload after clearing, preserving the deployment base path (web or Tauri). */
export function clientResetUrl(baseUrl: string, currentUrl: string): string {
  try {
    return new URL(baseUrl || '/', currentUrl).toString();
  } catch {
    return '/';
  }
}
