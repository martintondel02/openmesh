/**
 * Pure image-compression planning math.
 *
 * The browser-side compressor (src/lib/attachments.ts) uses these functions to
 * decide dimensions and quality before touching a canvas, so the interesting
 * logic is unit-testable. Heuristics are calibrated against typical camera JPEGs
 * and deliberately conservative: the canvas re-encode loop still verifies the
 * real byte size and shrinks further if needed.
 */

export interface Size {
  width: number;
  height: number;
}

export interface DimensionPlan extends Size {
  scale: number;
  /** True when the source was already smaller than the box (no upscaling). */
  upscaled: boolean;
}

/** Fit `src` inside `maxW × maxH`, preserving aspect ratio, never upscaling. */
export function planDimensions(srcW: number, srcH: number, maxW: number, maxH: number): DimensionPlan {
  if (srcW <= 0 || srcH <= 0 || maxW <= 0 || maxH <= 0) {
    return { width: 0, height: 0, scale: 0, upscaled: false };
  }
  const scale = Math.min(maxW / srcW, maxH / srcH, 1);
  return {
    width: Math.max(1, Math.round(srcW * scale)),
    height: Math.max(1, Math.round(srcH * scale)),
    scale,
    upscaled: scale === 1 && (srcW > maxW || srcH > maxH),
  };
}

/**
 * Heuristic JPEG/WebP byte estimate. Coefficients fitted to typical photographic
 * content: ~0.05 B/px at q=0.3 up to ~0.30 B/px near q=0.9.
 */
export function estimateEncodedBytes(width: number, height: number, quality: number): number {
  const q = Math.min(1, Math.max(0.05, quality));
  const bytesPerPixel = 0.05 + 0.32 * q * q;
  return Math.round(width * height * bytesPerPixel);
}

export interface CompressionPlan extends Size {
  quality: number;
  estimatedBytes: number;
  scale: number;
  iterations: number;
  /** True when even the smallest plan may exceed the target. */
  atFloor: boolean;
}

export interface CompressionOptions {
  /** Longest edge before quality is considered. */
  maxDimension?: number;
  /** Lowest quality we are willing to emit. */
  minQuality?: number;
  /** Highest quality we will emit. */
  maxQuality?: number;
  /** Never shrink a dimension below this (unless source is smaller). */
  minDimension?: number;
}

/**
 * Plan dimensions + quality so the encoded image lands under `targetBytes`.
 * Strategy: keep the source aspect, walk quality down to a floor, then shrink the
 * longest edge by 15 % and retry. Never returns zero-size outputs.
 */
export function planCompression(
  srcWidth: number,
  srcHeight: number,
  targetBytes: number,
  options: CompressionOptions = {},
): CompressionPlan {
  const maxDimension = options.maxDimension ?? 1600;
  const minQuality = options.minQuality ?? 0.32;
  const maxQuality = options.maxQuality ?? 0.82;
  const minDimension = options.minDimension ?? 96;
  const target = Math.max(1024, Math.floor(targetBytes || 0));

  const start = planDimensions(srcWidth, srcHeight, maxDimension, maxDimension);
  let width = start.width;
  let height = start.height;
  let iterations = 0;
  const qualitySteps = [maxQuality, 0.7, 0.6, 0.5, 0.42, minQuality];

  for (let shrink = 0; shrink < 12; shrink += 1) {
    for (const quality of qualitySteps) {
      iterations += 1;
      const estimatedBytes = estimateEncodedBytes(width, height, quality);
      if (estimatedBytes <= target) {
        return {
          width,
          height,
          quality,
          estimatedBytes,
          scale: start.width > 0 ? width / start.width : 0,
          iterations,
          atFloor: false,
        };
      }
    }
    const longest = Math.max(width, height);
    if (longest <= minDimension) break;
    const shrinkFactor = 0.85;
    const nextW = Math.max(1, Math.round(width * shrinkFactor));
    const nextH = Math.max(1, Math.round(height * shrinkFactor));
    if (nextW === width && nextH === height) break;
    width = nextW;
    height = nextH;
  }

  const estimatedBytes = estimateEncodedBytes(width, height, minQuality);
  return {
    width,
    height,
    quality: minQuality,
    estimatedBytes,
    scale: start.width > 0 ? width / start.width : 0,
    iterations,
    atFloor: true,
  };
}

/** Largest attachment target for a profile given a safety fraction (default 85 %). */
export function compressionTargetBytes(capBytes: number, fraction = 0.85): number {
  if (!Number.isFinite(capBytes) || capBytes <= 0) return 2048;
  return Math.max(1024, Math.floor(capBytes * Math.min(1, Math.max(0.1, fraction))));
}

/** Slider position (0..100) → target bytes between 1 KiB and the profile cap. */
export function sliderToTargetBytes(slider: number, capBytes: number): number {
  const pct = Math.min(100, Math.max(0, slider)) / 100;
  const min = 1024;
  return Math.round(min + (capBytes - min) * pct);
}

/** Target bytes → slider position (0..100), inverse of `sliderToTargetBytes`. */
export function targetBytesToSlider(bytes: number, capBytes: number): number {
  if (capBytes <= 1024) return 0;
  return Math.min(100, Math.max(0, Math.round(((bytes - 1024) / (capBytes - 1024)) * 100)));
}

/** Voice note size estimate for Opus at the nominal bitrate (PROTOCOL §9.4: 6–10 kbps). */
export function estimateVoiceBytes(durationMs: number, bitrateKbps = 7): number {
  const seconds = Math.max(0, durationMs) / 1000;
  return Math.round((seconds * bitrateKbps * 1000) / 8);
}

/** Human summary for the compression preview chip. */
export function compressionSummary(
  originalBytes: number,
  plan: Pick<CompressionPlan, 'estimatedBytes' | 'width' | 'height' | 'quality'>,
): string {
  if (originalBytes <= 0) return `${plan.width}×${plan.height}`;
  const ratio = plan.estimatedBytes / originalBytes;
  return `${plan.width}×${plan.height} · q${Math.round(plan.quality * 100)} · ~${Math.round(ratio * 100)}% of original`;
}
