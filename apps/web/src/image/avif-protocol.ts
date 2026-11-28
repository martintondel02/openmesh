/**
 * Shared protocol between the main thread and the AVIF encoder Worker.
 *
 * The Worker is created lazily on the first image encode (never at startup): the bundled
 * rav1e WASM is ~1.3 MB raw / ~406 KiB gzip-9, so it must not compete with app boot.
 * All values crossing the Worker boundary are plain data; the RGBA buffer is transferred.
 */

export const AVIF_MIME = 'image/avif';

/** Worker request: encode one RGBA8 image at a preset quality fraction (e.g. 0.4). */
export interface AvifEncodeRequest {
  type: 'encode';
  id: number;
  /** Absolute URL of openmesh_image.wasm (resolved against the app base). */
  wasmUrl: string;
  rgba: ArrayBuffer;
  width: number;
  height: number;
  /** Preset quality fraction from content/presets/media.json (0.28 / 0.40 / 0.50). */
  qualityFraction: number;
}

export type AvifWorkerErrorReason =
  | 'unsupported'
  | 'fetch'
  | 'instantiate'
  | 'invalid'
  | 'memory'
  | 'encode'
  | 'timeout';

export interface AvifEncodedResponse {
  type: 'encoded';
  id: number;
  bytes: ArrayBuffer;
  /** Worker-side encode duration in milliseconds (excludes download/instantiation). */
  ms: number;
  /** Calibrated ravif quality used for the AVIF candidate. */
  quality: number;
  /** Preset WebP quality the AVIF quality was calibrated from. */
  webpQuality: number;
  speed: number;
}

export interface AvifErrorResponse {
  type: 'error';
  id: number;
  reason: AvifWorkerErrorReason;
  message: string;
}

export type AvifWorkerResponse = AvifEncodedResponse | AvifErrorResponse;

/**
 * Quality-matched AVIF/WebP policy mirroring `openmesh-image` (used by tests and as a fallback
 * if the WASM helper exports are unavailable). The Worker prefers the WASM exports
 * (`om_image_quality_from_fraction`, `om_image_calibrated_quality`,
 * `om_image_speed_for_quality`) so the Rust policy remains the single source of truth.
 *
 * ravif quality is perceptually lower than libwebp at the same number, so the AVIF candidate is
 * encoded at 1.25× the preset WebP quality (q28→35, q40→50, q50→62) and the WebP candidate keeps
 * the preset quality. Speed is capped from the preset WebP quality (q28→10, q40/50→8).
 */
export interface AvifEncodeSettings {
  /** Preset WebP quality (`quality_from_fraction`, e.g. 28/40/50). */
  webpQuality: number;
  /** Calibrated ravif quality for the AVIF candidate (e.g. 35/50/62). */
  avifQuality: number;
  /** ravif speed: 10 for tiny, 8 otherwise. */
  speed: number;
}

/** Integer floor of 1.25×, clamped to 1..100 (mirrors `calibrated_avif_quality`). */
export function calibratedAvifQuality(webpQuality: number): number {
  const clamped = Math.min(100, Math.max(1, Math.round(webpQuality)));
  return Math.min(100, Math.max(1, Math.floor((clamped * 125) / 100)));
}

export function avifEncodeSettings(qualityFraction: number): AvifEncodeSettings {
  const finite = Number.isFinite(qualityFraction) ? qualityFraction : 0.5;
  const webpQuality = Math.min(100, Math.max(1, Math.round(finite * 100)));
  return {
    webpQuality,
    avifQuality: calibratedAvifQuality(webpQuality),
    speed: webpQuality <= 30 ? 10 : 8,
  };
}

const AVIF_BRANDS = new Set(['avif', 'avis']);

/**
 * True only for a complete AVIF file header (`....ftyp` + an `avif`/`avis` major brand).
 * Used as a defensive type check so a failed/fallback encode can never be labelled AVIF.
 */
export function isAvifFileHeader(bytes: Uint8Array): boolean {
  if (bytes.byteLength < 12) return false;
  const ftyp =
    bytes[4] === 0x66 && // f
    bytes[5] === 0x74 && // t
    bytes[6] === 0x79 && // y
    bytes[7] === 0x70; // p
  if (!ftyp) return false;
  const brand = String.fromCharCode(bytes[8]!, bytes[9]!, bytes[10]!, bytes[11]!);
  return AVIF_BRANDS.has(brand);
}
