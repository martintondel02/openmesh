/**
 * Browser-side image handling: decode → plan → canvas resize → encode, looping
 * until the blob fits the airtime budget. Pure planning math lives in
 * `compression.ts`; this file only touches DOM APIs.
 */

import { encodeAvifRgba } from '../image/avif-encoder';
import { AVIF_MIME } from '../image/avif-protocol';
import { planCompression, planDimensions, type CompressionPlan } from './compression';
import {
  encodedImageFilename,
  imageEncodeOrderForTier,
  imageExtensionForMime,
  imageTier,
  selectPreferredImageEncoding,
  type ImageEncodeMime,
  type ImageEncoderSupport,
  type ImageTierId,
} from './media-tiers';

export interface ImageEncodingOption {
  mime: string;
  bytes: number;
}

export interface CompressedImage {
  blob: Blob;
  objectUrl: string;
  mime: string;
  /** Every validated candidate that was encoded (AVIF/WebP/JPEG) with its byte size. */
  alternatives?: ImageEncodingOption[];
  width: number;
  height: number;
  quality: number;
  originalBytes: number;
  originalWidth: number;
  originalHeight: number;
  plan: CompressionPlan;
  attempts: number;
}

const MAX_ATTEMPTS = 6;

function createCanvas(width: number, height: number): HTMLCanvasElement {
  const canvas = document.createElement('canvas');
  canvas.width = Math.max(1, width);
  canvas.height = Math.max(1, height);
  return canvas;
}

let webpSupport: boolean | null = null;

export function supportsWebp(): boolean {
  if (webpSupport !== null) return webpSupport;
  try {
    const canvas = createCanvas(2, 2);
    webpSupport = canvas.toDataURL('image/webp').startsWith('data:image/webp');
  } catch {
    webpSupport = false;
  }
  return webpSupport;
}

async function canvasToBlob(canvas: HTMLCanvasElement, mime: string, quality: number): Promise<Blob> {
  const blob = await new Promise<Blob | null>((resolve) => {
    canvas.toBlob((result) => resolve(result), mime, quality);
  });
  if (!blob) throw new Error('The browser could not encode this image');
  return blob;
}

let encoderSupportProbe: Promise<ImageEncoderSupport> | null = null;

/** Encode a 2x2 canvas and confirm the browser really produced the requested format. */
async function probeEncoder(mime: ImageEncodeMime): Promise<boolean> {
  try {
    const canvas = createCanvas(2, 2);
    const context = canvas.getContext('2d');
    if (!context) return false;
    context.fillStyle = '#808080';
    context.fillRect(0, 0, 2, 2);
    const blob = await new Promise<Blob | null>((resolve) => {
      canvas.toBlob((result) => resolve(result), mime, 0.8);
    });
    // Chromium falls back to image/png for unsupported types; that must never pass.
    if (!blob) return false;
    return blob.type === mime;
  } catch {
    return false;
  }
}

/**
 * Capability probe for browser canvas encoders. Cached for the session.
 * Chromium (checked: Chrome for Testing 159) cannot encode AVIF from canvas and
 * silently returns image/png — the `blob.type` check keeps that path off.
 */
export function detectImageEncoderSupport(): Promise<ImageEncoderSupport> {
  if (!encoderSupportProbe) {
    encoderSupportProbe = (async () => {
      const [avif, webp, jpeg] = await Promise.all([
        probeEncoder('image/avif'),
        probeEncoder('image/webp'),
        probeEncoder('image/jpeg'),
      ]);
      return { avif, webp, jpeg };
    })();
  }
  return encoderSupportProbe;
}

interface EncodedCanvasImage {
  blob: Blob;
  /** MIME actually produced by the encoder (verified via `blob.type` / AVIF header). */
  mime: ImageEncodeMime | string;
  /** All validated candidates that were compared for this canvas. */
  alternatives: ImageEncodingOption[];
}

interface AvifCanvasCandidate {
  blob: Blob;
  mime: 'image/avif';
}

/**
 * Encode the canvas with the bundled Rust AVIF encoder (lazy WASM in a Worker). Returns null
 * when the module is unavailable or fails, so the caller falls back to canvas WebP/JPEG and
 * labels the result with the MIME the browser actually produced.
 */
async function encodeWasmAvif(canvas: HTMLCanvasElement, quality: number): Promise<AvifCanvasCandidate | null> {
  try {
    const context = canvas.getContext('2d');
    if (!context) return null;
    const imageData = context.getImageData(0, 0, canvas.width, canvas.height);
    const encoded = await encodeAvifRgba(imageData.data, canvas.width, canvas.height, quality);
    const blob = new Blob([encoded.bytes.buffer as ArrayBuffer], { type: AVIF_MIME });
    if (blob.type !== AVIF_MIME || blob.size === 0) return null;
    return { blob, mime: AVIF_MIME };
  } catch {
    return null;
  }
}

/**
 * Encode one canvas as AVIF (bundled WASM) and WebP (canvas), then keep the smaller.
 *
 * - Every candidate is validated by `blob.type` (browser) or the AVIF file header (WASM),
 *   so a PNG fallback can never be sent under an AVIF/WebP label.
 * - AVIF is attempted first and the lazy Worker is loaded here (first image encode only).
 * - WebP is always encoded when available; JPEG is only attempted if WebP fails.
 * - A byte-size tie chooses AVIF, then WebP, then JPEG.
 */
async function encodeCanvasImage(
  canvas: HTMLCanvasElement,
  quality: number,
  support: ImageEncoderSupport,
  tierId: ImageTierId,
): Promise<EncodedCanvasImage> {
  const candidates: Array<{ blob: Blob; mime: ImageEncodeMime; bytes: number }> = [];

  if (tierId !== 'original') {
    const avif = await encodeWasmAvif(canvas, quality);
    if (avif) candidates.push({ blob: avif.blob, mime: avif.mime, bytes: avif.blob.size });
  }

  // Canvas AVIF is skipped when the WASM encoder already produced a real AVIF candidate;
  // the bundled encoder is the client's AVIF path, canvas stays the fallback for AVIF-less
  // browsers (which normally cannot produce AVIF anyway — the type check catches that).
  const hasAvif = candidates.some((candidate) => candidate.mime === 'image/avif');
  const order = imageEncodeOrderForTier(tierId, support).filter(
    (mime) => !(mime === 'image/avif' && hasAvif),
  );
  for (const mime of order) {
    try {
      const blob = await canvasToBlob(canvas, mime, quality);
      if (blob.type !== mime) continue; // browser silently fell back; never mislabel
      candidates.push({ blob, mime, bytes: blob.size });
      if (mime === 'image/webp') break; // JPEG is only needed when WebP failed
    } catch {
      continue;
    }
  }

  if (candidates.length === 0) {
    throw new Error('The browser could not encode this image in a supported format');
  }
  const preferredMime = selectPreferredImageEncoding(candidates) ?? candidates[0]!.mime;
  const chosen = candidates.find((candidate) => candidate.mime === preferredMime) ?? candidates[0]!;
  if (candidates.length > 1) {
    console.info(
      `[openmesh-image-select] ${JSON.stringify({
        tier: tierId,
        quality: Number(quality.toFixed(2)),
        candidates: candidates.map(({ mime, bytes }) => ({ mime, bytes })),
        chosen: chosen.mime,
      })}`,
    );
  }
  return {
    blob: chosen.blob,
    mime: chosen.mime,
    alternatives: candidates.map(({ mime, bytes }) => ({ mime, bytes })),
  };
}
interface DecodedImage {
  source: ImageBitmap | HTMLImageElement;
  width: number;
  height: number;
  close: () => void;
}

async function decodeImage(blob: Blob): Promise<DecodedImage> {
  if (typeof createImageBitmap === 'function') {
    try {
      const bitmap = await createImageBitmap(blob);
      return {
        source: bitmap,
        width: bitmap.width,
        height: bitmap.height,
        close: () => bitmap.close(),
      };
    } catch {
      // Fall through to the <img> decoder (older Safari, some SVG inputs).
    }
  }
  const url = URL.createObjectURL(blob);
  try {
    const image = await new Promise<HTMLImageElement>((resolve, reject) => {
      const element = new Image();
      element.onload = () => resolve(element);
      element.onerror = () => reject(new Error('The browser could not decode this image'));
      element.src = url;
    });
    return {
      source: image,
      width: image.naturalWidth || image.width,
      height: image.naturalHeight || image.height,
      close: () => URL.revokeObjectURL(url),
    };
  } catch (error) {
    URL.revokeObjectURL(url);
    throw error;
  }
}

/** Default compression target used by the composer (85 % of the profile cap). */
export function defaultImageTarget(profileCapBytes: number): number {
  return Math.max(1024, Math.floor(profileCapBytes * 0.85));
}

export interface CompressOptions {
  targetBytes: number;
  maxDimension?: number;
  mime?: 'image/webp' | 'image/jpeg' | 'auto';
  onProgress?: (stage: 'decode' | 'encode', attempt: number) => void;
}

/**
 * Compress an image to roughly `targetBytes` using canvas downscale + re-encode.
 * Returns a blob URL ready for the chat bubble preview and the upload.
 */
export async function compressImage(blob: Blob, options: CompressOptions): Promise<CompressedImage> {
  options.onProgress?.('decode', 0);
  const decoded = await decodeImage(blob);
  try {
    const support = await detectImageEncoderSupport();
    const target = Math.max(1024, Math.floor(options.targetBytes));
    let plan = planCompression(decoded.width, decoded.height, target, {
      maxDimension: options.maxDimension ?? 1600,
    });
    let attempts = 0;
    let best: {
      blob: Blob;
      mime: string;
      alternatives: ImageEncodingOption[];
      width: number;
      height: number;
      quality: number;
    } | null = null;

    for (let round = 0; round < MAX_ATTEMPTS; round += 1) {
      attempts += 1;
      options.onProgress?.('encode', attempts);
      const canvas = createCanvas(plan.width, plan.height);
      const context = canvas.getContext('2d');
      if (!context) throw new Error('Canvas 2D is unavailable in this browser');
      context.imageSmoothingEnabled = true;
      context.imageSmoothingQuality = 'high';
      context.drawImage(decoded.source, 0, 0, plan.width, plan.height);
      const encoded = await encodeCanvasImage(canvas, plan.quality, support, 'high');
      best = {
        blob: encoded.blob,
        mime: encoded.mime,
        alternatives: encoded.alternatives,
        width: plan.width,
        height: plan.height,
        quality: plan.quality,
      };
      if (encoded.blob.size <= target) break;

      // Still too large: shrink and drop quality, then retry from the real size.
      const nextWidth = Math.max(64, Math.round(plan.width * 0.82));
      const nextHeight = Math.max(64, Math.round(plan.height * 0.82));
      if (nextWidth === plan.width && nextHeight === plan.height) break;
      plan = planCompression(nextWidth, nextHeight, target, {
        maxDimension: Math.max(nextWidth, nextHeight),
        minQuality: Math.max(0.26, plan.quality - 0.08),
        maxQuality: Math.max(0.3, plan.quality - 0.04),
      });
    }

    if (!best) throw new Error('Image compression failed');
    const objectUrl = URL.createObjectURL(best.blob);
    return {
      blob: best.blob,
      objectUrl,
      mime: best.mime,
      alternatives: best.alternatives,
      width: best.width,
      height: best.height,
      quality: best.quality,
      originalBytes: blob.size,
      originalWidth: decoded.width,
      originalHeight: decoded.height,
      plan: { ...plan, width: best.width, height: best.height, quality: best.quality, estimatedBytes: best.blob.size },
      attempts,
    };
  } finally {
    decoded.close();
  }
}

/** Read the pixel dimensions of an image without retaining it. */
export async function readImageDimensions(blob: Blob): Promise<{ width: number; height: number }> {
  const decoded = await decodeImage(blob);
  try {
    return { width: decoded.width, height: decoded.height };
  } finally {
    decoded.close();
  }
}

/** Validate a dropped/picked file before we spend CPU on it. */
export function validateImageFile(file: File): string | null {
  if (!file.type.startsWith('image/')) return 'Only images (JPEG, PNG, WebP, AVIF, GIF, SVG) can be sent from the web client.';
  if (file.size > 32 * 1024 * 1024) return 'That image is larger than 32 MiB — pick something smaller.';
  return null;
}

/**
 * Upload filename that matches the bytes actually encoded: the source base name
 * with the real extension (a photo picked as JPEG but sent as AVIF becomes .avif).
 */
export function encodedImageUploadName(file: Pick<File, 'name'>, mime: string): string {
  return encodedImageFilename(file.name, mime);
}

/** Human-readable file name fallback for pasted/dropped images. */
export function fallbackImageName(file: File): string {
  if (file.name) return file.name;
  const extension = file.type.split('/')[1]?.replace('+xml', '') || 'jpg';
  return `image-${Date.now().toString(36)}.${extension}`;
}


/** Tier label carried alongside the compressed result (web-only). */
export interface TierCompressedImage extends CompressedImage {
  tier: ImageTierId;
  /** True for `original`: the user confirmed sending the unmodified file. */
  original: boolean;
}

/**
 * Compress to one of the shared tiers (Tiny/Medium/High) or pass the original through.
 * Contract: downscale to `max_dim`, then encode at `quality` with the best format this
 * browser can actually produce (AVIF when it wins, else WebP, else JPEG); the measured
 * bytes and real MIME are shown later.
 */
export async function compressImageToTier(blob: Blob, tierId: ImageTierId): Promise<TierCompressedImage> {
  const tier = imageTier(tierId);
  const decoded = await decodeImage(blob);
  try {
    if (tier.id === 'original') {
      return {
        tier: 'original',
        original: true,
        blob,
        objectUrl: URL.createObjectURL(blob),
        mime: blob.type || 'application/octet-stream',
        alternatives: [{ mime: blob.type || 'application/octet-stream', bytes: blob.size }],
        width: decoded.width,
        height: decoded.height,
        quality: 1,
        originalBytes: blob.size,
        originalWidth: decoded.width,
        originalHeight: decoded.height,
        plan: {
          width: decoded.width,
          height: decoded.height,
          quality: 1,
          estimatedBytes: blob.size,
          scale: 1,
          iterations: 0,
          atFloor: false,
        },
        attempts: 0,
      };
    }
    const dims = planDimensions(decoded.width, decoded.height, tier.maxDim ?? 1600, tier.maxDim ?? 1600);
    const canvas = createCanvas(dims.width, dims.height);
    const context = canvas.getContext('2d');
    if (!context) throw new Error('Canvas 2D is unavailable in this browser');
    context.imageSmoothingEnabled = true;
    context.imageSmoothingQuality = 'high';
    context.drawImage(decoded.source, 0, 0, dims.width, dims.height);
    const support = await detectImageEncoderSupport();
    const quality = tier.quality ?? 0.6;
    const encoded = await encodeCanvasImage(canvas, quality, support, tier.id);
    return {
      tier: tier.id,
      original: false,
      blob: encoded.blob,
      objectUrl: URL.createObjectURL(encoded.blob),
      mime: encoded.mime,
      alternatives: encoded.alternatives,
      width: dims.width,
      height: dims.height,
      quality,
      originalBytes: blob.size,
      originalWidth: decoded.width,
      originalHeight: decoded.height,
      plan: {
        width: dims.width,
        height: dims.height,
        quality,
        estimatedBytes: encoded.blob.size,
        scale: dims.width / Math.max(1, decoded.width),
        iterations: 1,
        atFloor: false,
      },
      attempts: 1,
    };
  } finally {
    decoded.close();
  }
}

/**
 * Deterministic synthetic image used by demo-mode screenshots/tests (no file picker needed).
 * A 1600×1067 landscape gradient with simple shapes so tier resizing is visible.
 */
export async function createDemoImageFile(width = 1600, height = 1067): Promise<File> {
  const canvas = createCanvas(width, height);
  const context = canvas.getContext('2d');
  if (!context) throw new Error('Canvas 2D is unavailable in this browser');
  const sky = context.createLinearGradient(0, 0, 0, height);
  sky.addColorStop(0, '#0f2a3f');
  sky.addColorStop(0.52, '#f59e0b');
  sky.addColorStop(1, '#0b1220');
  context.fillStyle = sky;
  context.fillRect(0, 0, width, height);
  context.fillStyle = 'rgba(15, 23, 42, 0.92)';
  context.beginPath();
  context.moveTo(0, height * 0.72);
  context.lineTo(width * 0.24, height * 0.44);
  context.lineTo(width * 0.46, height * 0.7);
  context.lineTo(width * 0.7, height * 0.32);
  context.lineTo(width, height * 0.66);
  context.lineTo(width, height);
  context.lineTo(0, height);
  context.closePath();
  context.fill();
  context.fillStyle = '#34d399';
  context.beginPath();
  context.arc(width * 0.76, height * 0.2, height * 0.055, 0, Math.PI * 2);
  context.fill();
  context.strokeStyle = 'rgba(255,255,255,0.26)';
  context.lineWidth = 3;
  context.beginPath();
  context.arc(width * 0.18, height * 0.26, height * 0.07, 0, Math.PI * 2);
  context.stroke();
  context.fillStyle = 'rgba(226,232,240,0.85)';
  context.font = `${Math.round(height * 0.035)}px sans-serif`;
  context.fillText('Demo ridge · tier preview', width * 0.055, height * 0.91);
  const support = await detectImageEncoderSupport();
  let encoded: EncodedCanvasImage;
  try {
    encoded = await encodeCanvasImage(canvas, 0.86, support, 'high');
  } catch {
    const blob = await canvasToBlob(canvas, 'image/webp', 0.86);
    encoded = { blob, mime: blob.type || 'image/webp', alternatives: [{ mime: blob.type || 'image/webp', bytes: blob.size }] };
  }
  return new File([encoded.blob], `demo-ridge.${imageExtensionForMime(encoded.mime)}`, {
    type: encoded.mime,
  });
}
