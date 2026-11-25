/**
 * Small, dependency-free formatting and validation helpers shared by the UI.
 * Everything here is pure so it can be unit-tested without a DOM.
 */

/** Join class names, skipping falsy values. */
export function cn(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(' ');
}

export function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

export function pluralize(count: number, singular: string, plural = `${singular}s`): string {
  return count === 1 ? singular : plural;
}

/* ------------------------------------------------------------------ */
/* Bytes, durations, clocks                                           */
/* ------------------------------------------------------------------ */

const KIB = 1024;
const MIB = KIB * 1024;

function trimTrailingZeros(value: string): string {
  if (!value.includes('.')) return value;
  return value.replace(/0+$/, '').replace(/\.$/, '');
}

/** `1536` → `"1.5 KiB"`, `512` → `"512 B"`, `1572864` → `"1.5 MiB"`. */
export function formatBytes(bytes: number, fractionDigits?: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '—';
  if (bytes < KIB) return `${Math.round(bytes)} B`;
  const digits = fractionDigits ?? (bytes < 10 * KIB ? 1 : 1);
  if (bytes < MIB) {
    const kib = bytes / KIB;
    return `${trimTrailingZeros(kib.toFixed(digits))} KiB`;
  }
  const mib = bytes / MIB;
  return `${trimTrailingZeros(mib.toFixed(digits + 1))} MiB`;
}

/** Compact size for tight spots: `12.4K`, `980B`, `2.1M`. */
export function formatBytesShort(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '—';
  if (bytes < KIB) return `${Math.round(bytes)}B`;
  if (bytes < MIB) return `${(bytes / KIB).toFixed(1).replace(/\.0$/, '')}K`;
  return `${(bytes / MIB).toFixed(1).replace(/\.0$/, '')}M`;
}

/** `7400` → `"0:07"`; `3723000` → `"1:02:03"`. */
export function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) ms = 0;
  const total = Math.round(ms / 1000);
  const s = total % 60;
  const m = Math.floor(total / 60) % 60;
  const h = Math.floor(total / 3600);
  const ss = String(s).padStart(2, '0');
  if (h > 0) return `${h}:${String(m).padStart(2, '0')}:${ss}`;
  return `${m}:${ss}`;
}

/** `7400` → `"7.4 s"`. */
export function formatSeconds(ms: number, digits = 1): string {
  if (!Number.isFinite(ms) || ms < 0) ms = 0;
  if (ms < 1000) return `${Math.round(ms)} ms`;
  const s = ms / 1000;
  return `${s.toFixed(digits).replace(/\.0$/, '')} s`;
}

/** `"14:32"` in the viewer's local timezone (24h, deterministic across locale). */
export function formatClockTime(timestampMs: number): string {
  const d = new Date(timestampMs);
  if (Number.isNaN(d.getTime())) return '--:--';
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
const WEEKDAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];

function startOfDay(d: Date): number {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

function dayDelta(timestampMs: number, nowMs: number): number {
  const a = startOfDay(new Date(timestampMs));
  const b = startOfDay(new Date(nowMs));
  return Math.round((b - a) / 86_400_000);
}

/** Relative age for list rows: `now`, `3m`, `2h`, `Tue`, `12 Mar`, `2025-11`. */
export function formatRelativeTime(timestampMs: number, nowMs: number = Date.now()): string {
  const delta = nowMs - timestampMs;
  if (!Number.isFinite(timestampMs)) return '';
  if (delta < 45_000) return 'now';
  if (delta < 3_600_000) return `${Math.floor(delta / 60_000)}m`;
  const days = dayDelta(timestampMs, nowMs);
  if (days === 0) return `${Math.floor(delta / 3_600_000)}h`;
  if (days === 1) return 'Yesterday';
  if (days < 7) return WEEKDAYS[new Date(timestampMs).getDay()]!;
  const d = new Date(timestampMs);
  const sameYear = d.getFullYear() === new Date(nowMs).getFullYear();
  if (sameYear) return `${d.getDate()} ${MONTHS[d.getMonth()]}`;
  return `${d.getDate()} ${MONTHS[d.getMonth()]} ${d.getFullYear()}`;
}

/** Sticky separator label: `Today`, `Yesterday`, `Tuesday`, `12 March 2026`. */
export function formatDayLabel(timestampMs: number, nowMs: number = Date.now()): string {
  const days = dayDelta(timestampMs, nowMs);
  if (days === 0) return 'Today';
  if (days === 1) return 'Yesterday';
  if (days > 1 && days < 7) {
    const weekday = WEEKDAYS[new Date(timestampMs).getDay()]!;
    return weekday;
  }
  const d = new Date(timestampMs);
  const month = MONTHS[d.getMonth()]!;
  return `${d.getDate()} ${month} ${d.getFullYear()}`;
}

/** Local `YYYY-MM-DD` key used to group a thread into day sections. */
export function dayKey(timestampMs: number): string {
  const d = new Date(timestampMs);
  const m = String(d.getMonth() + 1).padStart(2, '0');
  const day = String(d.getDate()).padStart(2, '0');
  return `${d.getFullYear()}-${m}-${day}`;
}

/** `"3 min ago"`, `"2 h ago"`, `"just now"`, `"4 d ago"`, `"never"`. */
export function formatLastSeen(timestampMs: number | null | undefined, nowMs: number = Date.now()): string {
  if (!timestampMs) return 'never';
  const delta = nowMs - timestampMs;
  if (delta < 60_000) return 'just now';
  if (delta < 3_600_000) {
    const m = Math.floor(delta / 60_000);
    return `${m} ${pluralize(m, 'min', 'min')} ago`;
  }
  if (delta < 86_400_000) {
    const h = Math.floor(delta / 3_600_000);
    return `${h} ${pluralize(h, 'h', 'h')} ago`;
  }
  const d = Math.floor(delta / 86_400_000);
  if (d < 30) return `${d} ${pluralize(d, 'day')} ago`;
  return formatRelativeTime(timestampMs, nowMs);
}

/** Uptime: `4d 3h`, `2h 12m`, `48s`. */
export function formatUptime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '—';
  if (seconds < 60) return `${Math.floor(seconds)}s`;
  const m = Math.floor(seconds / 60) % 60;
  const h = Math.floor(seconds / 3600) % 24;
  const d = Math.floor(seconds / 86400);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

/* ------------------------------------------------------------------ */
/* Node ids, identities, hex                                          */
/* ------------------------------------------------------------------ */

const HEX_RE = /^[0-9a-fA-F]+$/;

export function isHex(value: string, byteLength?: number): boolean {
  if (!HEX_RE.test(value)) return false;
  if (value.length % 2 !== 0) return false;
  return byteLength === undefined || value.length === byteLength * 2;
}

export function hexToBytes(hex: string): Uint8Array {
  const clean = hex.replace(/^0x/i, '');
  const out = new Uint8Array(clean.length / 2);
  for (let i = 0; i < out.length; i += 1) out[i] = Number.parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  return out;
}

export function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

/** Node ids are 8 self-certifying bytes, rendered as 16 lowercase hex chars. */
export function isValidNodeId(value: string): boolean {
  return isHex(value, 8);
}

/** Channel ids use the same 8-byte shape as node ids. */
export function isValidChannelId(value: string): boolean {
  return isHex(value, 8);
}

export function normalizeHex(value: string): string {
  return value.trim().replace(/^0x/i, '').replace(/[\s·:\-_]/g, '').toLowerCase();
}

/** `"ab12…7890"` — never shows the whole id in dense UI. */
export function shortenNodeId(nodeId: string, head = 4, tail = 4): string {
  const clean = nodeId.trim();
  if (clean.length <= head + tail + 1) return clean;
  return `${clean.slice(0, head)}…${clean.slice(-tail)}`;
}

/** `"ab12·cd34·ef56·7890"` for detail panels. */
export function formatNodeId(nodeId: string): string {
  return nodeId.toLowerCase().match(/.{1,4}/g)?.join('·') ?? nodeId;
}

/** `"ab12 cd34 ef56 7890"` for monospace raw display. */
export function spacedHex(value: string): string {
  return value.toLowerCase().match(/.{1,2}/g)?.join(' ') ?? value;
}

export function bytesToBase64(bytes: Uint8Array): string {
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

export function base64ToBytes(base64: string): Uint8Array {
  const normalized = base64.replace(/-/g, '+').replace(/_/g, '/');
  const padded = normalized + '='.repeat((4 - (normalized.length % 4)) % 4);
  const binary = atob(padded);
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) out[i] = binary.charCodeAt(i);
  return out;
}

/** Returns decoded bytes when `value` is base64 for exactly `byteLength` bytes. */
export function decodeBase64OfLength(value: string, byteLength: number): Uint8Array | null {
  const trimmed = value.trim();
  if (!trimmed || !/^[A-Za-z0-9+/_=-]+$/.test(trimmed)) return null;
  try {
    const bytes = base64ToBytes(trimmed);
    return bytes.length === byteLength ? bytes : null;
  } catch {
    return null;
  }
}

/**
 * FNV-1a over bytes, rendered as 4 hex chars. Only for friendly PSK/identity
 * fingerprints in the UI — never used for anything security-relevant.
 */
export function fingerprintHex(bytes: Uint8Array): string {
  let hash = 0x811c9dc5;
  for (const byte of bytes) {
    hash ^= byte;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash.toString(16).padStart(8, '0').slice(0, 4);
}

/* ------------------------------------------------------------------ */
/* Text                                                               */
/* ------------------------------------------------------------------ */

type SegmenterCtor = new (
  locales?: string,
  options?: { granularity: 'grapheme' },
) => { segment(input: string): Iterable<{ segment: string }> };

/** Grapheme-safe truncation (handles emoji sequences and combining marks). */
export function truncateGraphemes(text: string, max: number, ellipsis = '…'): string {
  if (max <= 0 || text.length === 0) return '';
  let graphemes: string[];
  const Segmenter = (Intl as unknown as { Segmenter?: SegmenterCtor }).Segmenter;
  if (typeof Segmenter === 'function') {
    try {
      graphemes = Array.from(new Segmenter('en', { granularity: 'grapheme' }).segment(text), (s) => s.segment);
    } catch {
      graphemes = Array.from(text);
    }
  } else {
    graphemes = Array.from(text);
  }
  if (graphemes.length <= max) return text;
  return graphemes.slice(0, max).join('') + ellipsis;
}

/** Single-line preview for conversation rows. */
export function messagePreview(text: string | null | undefined, fallback = 'No messages yet', max = 80): string {
  if (!text) return fallback;
  const flattened = text.replace(/\s+/g, ' ').trim();
  return truncateGraphemes(flattened, max);
}

export function initials(name: string): string {
  const parts = name
    .trim()
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2);
  if (parts.length === 0) return '?';
  return parts.map((p) => Array.from(p)[0]?.toUpperCase() ?? '').join('');
}

/** Count of user-perceived characters, used for the composer counter. */
export function graphemeLength(text: string): number {
  const Segmenter = (Intl as unknown as { Segmenter?: SegmenterCtor }).Segmenter;
  if (typeof Segmenter === 'function') {
    try {
      return Array.from(new Segmenter('en', { granularity: 'grapheme' }).segment(text)).length;
    } catch {
      return Array.from(text).length;
    }
  }
  return Array.from(text).length;
}

/* ------------------------------------------------------------------ */
/* Radio / telemetry presentation helpers                             */
/* ------------------------------------------------------------------ */

/** Map RSSI in dBm to 0–4 bars. LoRa is usable far below Wi-Fi thresholds. */
export function signalBars(rssi: number | null | undefined): 0 | 1 | 2 | 3 | 4 {
  if (rssi === null || rssi === undefined || !Number.isFinite(rssi)) return 0;
  if (rssi >= -85) return 4;
  if (rssi >= -95) return 3;
  if (rssi >= -105) return 2;
  if (rssi >= -115) return 1;
  return 0;
}

export function signalLabel(rssi: number | null | undefined): string {
  if (rssi === null || rssi === undefined || !Number.isFinite(rssi)) return 'No signal';
  const bars = signalBars(rssi);
  const labels = ['no signal', 'marginal', 'weak', 'good', 'strong'];
  return `${labels[bars]} (${Math.round(rssi)} dBm)`;
}

/** LoRa SNR is usable down to ~-20 dB; ≥5 dB is comfortable. */
export function snrLabel(snr: number | null | undefined): string {
  if (snr === null || snr === undefined || !Number.isFinite(snr)) return '—';
  return `${snr > 0 ? '+' : ''}${snr.toFixed(1)} dB`;
}

export function batteryTone(pct: number | null | undefined): 'good' | 'fair' | 'low' | 'unknown' {
  if (pct === null || pct === undefined || !Number.isFinite(pct)) return 'unknown';
  if (pct <= 15) return 'low';
  if (pct <= 35) return 'fair';
  return 'good';
}

export function formatPct(value: number | null | undefined, digits = 1): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return '—';
  return `${value.toFixed(digits).replace(/\.0$/, '')}%`;
}

export function formatFrequency(mhz: number | null | undefined): string {
  if (mhz === null || mhz === undefined || !Number.isFinite(mhz)) return '—';
  return `${mhz.toFixed(3)} MHz`;
}

export function formatRole(role: string | null | undefined): string {
  switch (role) {
    case 'repeater':
      return 'Repeater';
    case 'gateway':
      return 'Gateway';
    case 'tracker':
      return 'Tracker';
    case 'room_server':
      return 'Room server';
    case 'client':
      return 'Client';
    default:
      return role ? role.replace(/_/g, ' ') : 'Unknown';
  }
}

export const ROLE_COLORS: Record<string, string> = {
  client: '#34d399',
  repeater: '#fbbf24',
  tracker: '#38bdf8',
  gateway: '#a78bfa',
  room_server: '#f472b6',
};

export function roleColor(role: string | null | undefined): string {
  return ROLE_COLORS[role ?? ''] ?? '#94a3b8';
}

/* ------------------------------------------------------------------ */
/* Misc                                                               */
/* ------------------------------------------------------------------ */

let idCounter = 0;

/** Monotonic-ish local id for optimistic rows. */
export function localId(prefix = 'local'): string {
  idCounter += 1;
  return `${prefix}_${Date.now().toString(36)}_${idCounter.toString(36)}`;
}

export function randomHex(bytes: number): string {
  const buf = new Uint8Array(bytes);
  globalThis.crypto?.getRandomValues(buf);
  return Array.from(buf, (b) => b.toString(16).padStart(2, '0')).join('');
}

export function randomBase64(bytes: number): string {
  const buf = new Uint8Array(bytes);
  globalThis.crypto?.getRandomValues(buf);
  return bytesToBase64(buf);
}

/** A fresh 256-bit channel PSK, base64 (spec §4.4 `psk`). */
export function generateChannelPsk(): string {
  return randomBase64(32);
}

/** Stable-ish key for React lists. */
export function messageKey(msg: { msg_id: number; tempId?: string }): string {
  return msg.tempId ?? `m${msg.msg_id}`;
}

export function safeJsonParse<T = unknown>(input: string): T | null {
  try {
    return JSON.parse(input) as T;
  } catch {
    return null;
  }
}
