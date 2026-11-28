/**
 * Shared media tiers (content/presets/media.json is the single source).
 *
 * Normative: docs/design/media-tiers.md §1/§5. The JSON is imported directly so the
 * web client cannot drift from Android/tests; `mediaTierParity.test.ts` checks it
 * against the repository copy as an extra guard.
 */
import mediaPresets from '../../../../content/presets/media.json';
import { estimateAttachment, hourlyAirtimeBudgetMs, maxAttachmentBytes } from './airtime';
import { planDimensions } from './compression';
import type { RadioProfile } from './airtime';

export type ImageTierId = 'tiny' | 'medium' | 'high' | 'original';
export type VoiceTierId = 'tiny' | 'medium' | 'high';

export interface ImageTier {
  id: ImageTierId;
  label: string;
  maxDim: number | null;
  quality: number | null;
  /** Declared fallback MIME from `content/presets/media.json` (currently image/webp). */
  mime: string;
  /**
   * Browser encode preference for this tier, best first: AVIF when the browser
   * can actually encode it, then WebP, then JPEG. All formats share the same
   * `maxDim`/`quality` targets, so the tier geometry does not change with MIME.
   */
  encodeMimes: ImageEncodeMime[];
  description: string;
  targetLabel: string;
}

export interface VoiceTier {
  id: VoiceTierId;
  label: string;
  codec: 'codec2' | 'opus';
  mode: string;
  mime: string;
  sampleRate: number;
  channels: number;
  frameMs: number | null;
  bitsPerSecond: number;
  maxDurationS: number;
  description: string;
}

const IMAGE_LABELS: Record<ImageTierId, { label: string; description: string }> = {
  tiny: { label: 'Tiny', description: 'Fastest to send; good for a quick glance.' },
  medium: { label: 'Medium', description: 'Balanced detail and airtime.' },
  high: { label: 'High', description: 'Most detail the radio can carry in one go.' },
  original: { label: 'Original', description: 'No resize or re-encode; must be explicitly confirmed.' },
};

const VOICE_LABELS: Record<VoiceTierId, { label: string; description: string }> = {
  tiny: { label: 'Tiny', description: 'Codec2 1.3k — longest recording, smallest bytes, robotic voice.' },
  medium: { label: 'Medium', description: 'Codec2 3.2k — clearer voice, shorter cap.' },
  high: { label: 'High', description: 'Opus — natural voice, shortest cap and largest bytes.' },
};

export const IMAGE_TIER_ORDER: ImageTierId[] = ['tiny', 'medium', 'high', 'original'];
export const VOICE_TIER_ORDER: VoiceTierId[] = ['tiny', 'medium', 'high'];

/* ------------------------------------------------------------------ */
/* Browser image encoders (AVIF where supported, WebP/JPEG fallbacks)  */
/* ------------------------------------------------------------------ */

/** Browser canvas encoders in preference order (best compression first). */
export const IMAGE_ENCODE_MIMES = ['image/avif', 'image/webp', 'image/jpeg'] as const;
export type ImageEncodeMime = (typeof IMAGE_ENCODE_MIMES)[number];

/** Result of a canvas `toBlob` capability probe (verified by `blob.type`). */
export interface ImageEncoderSupport {
  avif: boolean;
  webp: boolean;
  jpeg: boolean;
}

export interface ImageEncodingCandidate {
  mime: ImageEncodeMime;
  bytes: number;
}

/** Supported encoders in preference order (AVIF only when the probe verified it). */
export function imageEncodeOrder(support: ImageEncoderSupport): ImageEncodeMime[] {
  return IMAGE_ENCODE_MIMES.filter((mime) =>
    mime === 'image/avif' ? support.avif : mime === 'image/webp' ? support.webp : support.jpeg,
  );
}

/** Preferred encoders for one tier, filtered by what this browser can encode. */
export function imageEncodeOrderForTier(tierInput: ImageTierId, support: ImageEncoderSupport): ImageEncodeMime[] {
  const tier = imageTier(tierInput);
  if (tier.id === 'original') return [];
  return tier.encodeMimes.filter((mime) =>
    mime === 'image/avif' ? support.avif : mime === 'image/webp' ? support.webp : support.jpeg,
  );
}

/**
 * Choose the validated encoding with the fewest bytes. A byte-size tie prefers WebP, then JPEG,
 * then AVIF: the quality-matched AVIF candidate only wins when it is strictly smaller, matching
 * `docs/design/media-tiers.md` §1.2. Callers pass only candidates whose `blob.type` matched the
 * requested format, so a silent browser fallback cannot be selected.
 */
export function selectPreferredImageEncoding(
  candidates: readonly ImageEncodingCandidate[],
): ImageEncodeMime | null {
  if (candidates.length === 0) return null;
  const tieRank: Record<ImageEncodeMime, number> = {
    'image/webp': 0,
    'image/jpeg': 1,
    'image/avif': 2,
  };
  return [...candidates].sort(
    (a, b) => a.bytes - b.bytes || tieRank[a.mime] - tieRank[b.mime],
  )[0]!.mime;
}
/** Short display label for the image MIME that was actually produced. */
export function imageFormatLabel(mime: string | null | undefined): string {
  if (!mime) return 'Image';
  if (mime === 'image/avif') return 'AVIF';
  if (mime === 'image/webp') return 'WebP';
  if (mime === 'image/jpeg' || mime === 'image/jpg') return 'JPEG';
  if (mime === 'image/png') return 'PNG';
  if (mime === 'image/gif') return 'GIF';
  if (mime === 'image/svg+xml') return 'SVG';
  const subtype = mime.split('/')[1] ?? 'image';
  return subtype.toUpperCase();
}

/** File extension for the actual encoded MIME (never the requested-but-unused one). */
export function imageExtensionForMime(mime: string | null | undefined): string {
  const value = (mime ?? '').split(';')[0]?.trim().toLowerCase() ?? '';
  switch (value) {
    case 'image/avif':
      return 'avif';
    case 'image/webp':
      return 'webp';
    case 'image/jpeg':
    case 'image/jpg':
      return 'jpg';
    case 'image/png':
      return 'png';
    case 'image/gif':
      return 'gif';
    case 'image/svg+xml':
      return 'svg';
    case 'image/heic':
      return 'heic';
    case 'image/heif':
      return 'heif';
    default: {
      const subtype = value.startsWith('image/') ? value.slice('image/'.length).replace(/[^a-z0-9]+/g, '') : '';
      return subtype || 'img';
    }
  }
}

/**
 * Filename for the bytes actually uploaded: keep the source base name but use the
 * real encoded extension (`photo.jpg` picked as AVIF becomes `photo.avif`).
 */
export function encodedImageFilename(
  sourceName: string | null | undefined,
  mime: string,
  fallbackPrefix = 'image',
): string {
  const extension = imageExtensionForMime(mime);
  const name = (sourceName ?? '').trim();
  if (!name) return `${fallbackPrefix}-${Date.now().toString(36)}.${extension}`;
  const dot = name.lastIndexOf('.');
  const base = dot > 0 ? name.slice(0, dot) : name;
  return `${base || fallbackPrefix}.${extension}`;
}

export const IMAGE_TIERS: ImageTier[] = IMAGE_TIER_ORDER.map((id) => {
  const preset = mediaPresets.images[id];
  return {
    id,
    label: IMAGE_LABELS[id].label,
    maxDim: preset.max_dim,
    quality: preset.quality,
    mime: preset.mime,
    encodeMimes: id === 'original' ? [] : [...IMAGE_ENCODE_MIMES],
    description: IMAGE_LABELS[id].description,
    targetLabel:
      id === 'original'
        ? 'no resize · original bytes'
        : `≤ ${preset.max_dim} px · q${Math.round((preset.quality ?? 0) * 100)}`,
  };
});

export const VOICE_TIERS: VoiceTier[] = VOICE_TIER_ORDER.map((id) => {
  const preset = mediaPresets.voice[id];
  return {
    id,
    label: VOICE_LABELS[id].label,
    codec: preset.codec as 'codec2' | 'opus',
    mode: preset.mode,
    mime: preset.mime,
    sampleRate: preset.sample_rate,
    channels: preset.channels,
    frameMs: 'frame_ms' in preset ? preset.frame_ms : null,
    bitsPerSecond: preset.bits_per_second,
    maxDurationS: preset.max_duration_s,
    description: VOICE_LABELS[id].description,
  };
});

export const DEFAULT_IMAGE_TIER: ImageTierId = mediaPresets.defaults.image as ImageTierId;
export const DEFAULT_VOICE_TIER: VoiceTierId = mediaPresets.defaults.voice as VoiceTierId;
export const MEDIA_PRESETS_VERSION = mediaPresets.version;
export const MEDIA_PRESETS = mediaPresets;

export function imageTier(id: ImageTierId): ImageTier {
  return IMAGE_TIERS.find((tier) => tier.id === id) ?? IMAGE_TIERS[0]!;
}

export function voiceTier(id: VoiceTierId): VoiceTier {
  return VOICE_TIERS.find((tier) => tier.id === id) ?? VOICE_TIERS[0]!;
}

export function isImageTierId(value: unknown): value is ImageTierId {
  return typeof value === 'string' && (IMAGE_TIER_ORDER as string[]).includes(value);
}

export function isVoiceTierId(value: unknown): value is VoiceTierId {
  return typeof value === 'string' && (VOICE_TIER_ORDER as string[]).includes(value);
}

/** Original is blocked for long-range profiles (contract §1 + §5). */
export function originalAllowedForProfile(profileId: string | null | undefined): boolean {
  const id = (profileId ?? '').toUpperCase();
  return Boolean(id) && !id.startsWith('LONG');
}

export function imageTierPolicy(profileId: string | null | undefined): {
  originalAllowed: boolean;
  reason: string | null;
} {
  const allowed = originalAllowedForProfile(profileId);
  return {
    originalAllowed: allowed,
    reason: allowed
      ? null
      : 'Original images are disabled on long-range profiles: they cannot fit the attachment budget and would block the mesh.',
  };
}

/** Estimated bytes for a tier before encoding (original uses the real source size). */
export function estimateImageTierBytes(
  tierInput: ImageTierId,
  input: { sourceBytes: number; sourceWidth: number; sourceHeight: number },
): number {
  const tier = imageTier(tierInput);
  if (tier.id === 'original') return Math.max(0, Math.ceil(input.sourceBytes));
  const dims = planDimensions(
    Math.max(1, input.sourceWidth),
    Math.max(1, input.sourceHeight),
    tier.maxDim ?? 1600,
    tier.maxDim ?? 1600,
  );
  // Match compression.ts' empirical WebP/JPEG estimate (AVIF only makes it smaller).
  const bytesPerPixel = 0.05 + 0.32 * (tier.quality ?? 0.5) ** 2;
  return Math.max(256, Math.round(dims.width * dims.height * bytesPerPixel));
}

export interface ImageTierPolicy {
  profileId: string;
  capBytes: number;
  originalAllowed: boolean;
  reason: string | null;
}

export interface ImageTierSuggestion extends ImageTierPolicy {
  suggested: ImageTierId;
  estimates: Array<{ tier: ImageTierId; bytes: number; fitsCap: boolean; allowed: boolean }>;
}

/** Largest allowed tier whose estimated bytes fit under the profile attachment cap. */
export function suggestImageTier(input: {
  profileId: string;
  sourceBytes: number;
  sourceWidth: number;
  sourceHeight: number;
}): ImageTierSuggestion {
  const policy = imageTierPolicy(input.profileId);
  const capBytes = maxAttachmentBytes(input.profileId);
  const budget = Math.max(1024, Math.floor(capBytes * 0.9));
  const estimates = IMAGE_TIER_ORDER.map((tier) => {
    const bytes = estimateImageTierBytes(tier, input);
    const allowed = tier !== 'original' || policy.originalAllowed;
    return { tier, bytes, fitsCap: bytes <= budget, allowed };
  });
  const suggested =
    [...estimates].reverse().find((entry) => entry.allowed && entry.fitsCap)?.tier ?? 'tiny';
  return {
    suggested,
    estimates,
    originalAllowed: policy.originalAllowed,
    reason: policy.reason,
    profileId: input.profileId,
    capBytes,
  };
}

export interface TierAirtimeSummary {
  bytes: number;
  airtimeMs: number;
  budgetPct: number | null;
  chunks: number;
  withinCap: boolean;
  label: string;
}

/** “≈ 3.4 s airtime at MEDIUM_FAST · 2.1 % of the hourly duty budget”. */
export function tierAirtimeSummary(
  bytes: number,
  profileId: string,
  regionId: string,
): TierAirtimeSummary {
  const estimate = estimateAttachment(bytes, profileId, regionId);
  const budgetMs = hourlyAirtimeBudgetMs(regionId);
  const budgetPct = budgetMs ? (estimate.airtimeMs / budgetMs) * 100 : null;
  const seconds = estimate.airtimeMs / 1000;
  const airtimeText = seconds >= 10 ? `${Math.round(seconds)} s` : `${seconds.toFixed(1)} s`;
  const dutyText = budgetPct === null ? 'no duty cycle' : `~${budgetPct.toFixed(1)} % of the hourly duty budget`;
  return {
    bytes,
    airtimeMs: estimate.airtimeMs,
    budgetPct,
    chunks: estimate.chunks,
    withinCap: estimate.withinCap,
    label: `≈ ${airtimeText} airtime at ${(profileId || '').replace(/_/g, ' ')} · ${dutyText}`,
  };
}

export function voiceTierBytesPerSecond(tierInput: VoiceTierId): number {
  return voiceTier(tierInput).bitsPerSecond / 8;
}

/** Raw Codec2 frame size (7 bytes @1300, 8 bytes @3200) or null for Opus. */
export function codec2FrameBytes(tierInput: VoiceTierId): number | null {
  const tier = voiceTier(tierInput);
  if (tier.codec !== 'codec2') return null;
  return tier.mode === '1300' ? 7 : 8;
}

/** Size on the wire: Codec2 is whole frames; Opus is bitrate × seconds. */
export function estimateVoiceTierBytes(durationMs: number, tierInput: VoiceTierId): number {
  const tier = voiceTier(tierInput);
  const ms = Math.max(0, durationMs);
  const frameBytes = codec2FrameBytes(tier.id);
  if (frameBytes !== null && tier.frameMs) {
    return Math.max(0, Math.ceil(ms / tier.frameMs) * frameBytes);
  }
  return Math.max(0, Math.ceil((ms / 1000) * voiceTierBytesPerSecond(tier.id)));
}

/** Effective cap = min(tier seconds, bytes cap converted at the tier bitrate). */
export function voiceTierCapMs(tierInput: VoiceTierId, profileId: string): {
  capMs: number;
  tierCapMs: number;
  byteCapMs: number;
  capBytes: number;
} {
  const tier = voiceTier(tierInput);
  const capBytes = maxAttachmentBytes(profileId);
  const tierCapMs = tier.maxDurationS * 1000;
  let byteCapMs: number;
  const frameBytes = codec2FrameBytes(tier.id);
  if (frameBytes !== null && tier.frameMs) {
    const frames = Math.max(0, Math.floor(capBytes / frameBytes));
    byteCapMs = frames * tier.frameMs;
  } else {
    byteCapMs = Math.floor((capBytes / voiceTierBytesPerSecond(tier.id)) * 1000);
  }
  return { capMs: Math.max(0, Math.min(tierCapMs, byteCapMs)), tierCapMs, byteCapMs, capBytes };
}

export function voiceTierForMime(mime: string | null | undefined): VoiceTierId | null {
  const value = (mime ?? '').toLowerCase();
  if (value.includes('audio/c2-1300')) return 'tiny';
  if (value.includes('audio/c2-3200')) return 'medium';
  if (value.includes('ogg') || value.includes('opus') || value.includes('mp4') || value.includes('aac') || value.includes('mpeg')) {
    return 'high';
  }
  return null;
}

export function isCodec2Mime(mime: string | null | undefined): boolean {
  return (mime ?? '').toLowerCase().startsWith('audio/c2-');
}

/**
 * Duration of a raw Codec2 payload derived from its byte length (whole frames × frame duration).
 *
 * Incoming and fixture messages may omit `duration_ms`; Codec2 is fixed-rate (7 B/40 ms at 1300,
 * 8 B/20 ms at 3200), so the stored size is enough to show an honest clip length and waveform.
 * Returns `null` for non-Codec2 MIME types or an empty payload.
 */
export function codec2PayloadDurationMs(
  byteLength: number | null | undefined,
  mime: string | null | undefined,
): number | null {
  if (!byteLength || byteLength <= 0 || !isCodec2Mime(mime)) return null;
  const tierId = voiceTierForMime(mime);
  if (!tierId || tierId === 'high') return null;
  const tier = voiceTier(tierId);
  const frameBytes = codec2FrameBytes(tierId);
  if (!frameBytes || !tier.frameMs) return null;
  return Math.floor(byteLength / frameBytes) * tier.frameMs;
}

export function codec2TierIndex(tierInput: VoiceTierId): 0 | 1 | 2 {
  if (tierInput === 'high') return 2;
  return voiceTier(tierInput).mode === '1300' ? 0 : 1;
}

/** Human label for an incoming/outgoing voice attachment. */
export function voiceAttachmentLabel(mime: string | null | undefined, durationMs?: number | null, bytes?: number | null): string {
  const tier = voiceTierForMime(mime);
  const value = (mime ?? '').toLowerCase();
  const highCodec =
    value.includes('opus') || value.includes('ogg')
      ? 'Opus'
      : value.includes('aac') || value.includes('mp4')
        ? 'AAC'
        : value.includes('mpeg')
          ? 'MP3'
          : 'Audio';
  const codec = tier === 'tiny' ? 'Codec2 1.3k' : tier === 'medium' ? 'Codec2 3.2k' : tier === 'high' ? highCodec : 'Audio';
  const duration = durationMs && durationMs > 0 ? ` · ${(durationMs / 1000).toFixed(durationMs >= 10_000 ? 0 : 1)} s` : '';
  const size = bytes && bytes > 0 ? ` · ${(bytes / 1024).toFixed(bytes >= 10 * 1024 ? 0 : 1)} KiB` : '';
  return `${codec}${duration}${size}`;
}

export function tierProfileLabel(profile: string | RadioProfile): string {
  return typeof profile === 'string' ? profile.replace(/_/g, ' ') : profile.label;
}
