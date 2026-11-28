import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import {
  DEFAULT_IMAGE_TIER,
  DEFAULT_VOICE_TIER,
  IMAGE_ENCODE_MIMES,
  IMAGE_TIERS,
  IMAGE_TIER_ORDER,
  MEDIA_PRESETS,
  MEDIA_PRESETS_VERSION,
  VOICE_TIERS,
  VOICE_TIER_ORDER,
  codec2FrameBytes,
  codec2PayloadDurationMs,
  codec2TierIndex,
  encodedImageFilename,
  estimateImageTierBytes,
  estimateVoiceTierBytes,
  imageEncodeOrder,
  imageEncodeOrderForTier,
  imageExtensionForMime,
  imageTier,
  imageTierPolicy,
  selectPreferredImageEncoding,
  isCodec2Mime,
  isImageTierId,
  isVoiceTierId,
  originalAllowedForProfile,
  suggestImageTier,
  tierAirtimeSummary,
  voiceAttachmentLabel,
  voiceTier,
  voiceTierBytesPerSecond,
  voiceTierCapMs,
  voiceTierForMime,
} from '../media-tiers';
import { maxAttachmentBytes } from '../airtime';
import { createDemoDataset } from '../mock/data';
import { codec2WasmUrl, loadCodec2, resetCodec2CacheForTests } from '../../voice/codec2';

const repoPresets = JSON.parse(
  readFileSync(resolve(process.cwd(), '../../content/presets/media.json'), 'utf8'),
) as unknown;

describe('media presets: single source + tiers', () => {
  it('imports the repository preset JSON verbatim (parity)', () => {
    expect(MEDIA_PRESETS).toEqual(repoPresets);
    expect(MEDIA_PRESETS_VERSION).toBe(1);
    expect(DEFAULT_IMAGE_TIER).toBe('medium');
    expect(DEFAULT_VOICE_TIER).toBe('tiny');
  });

  it('exposes the four image tiers with contract dimensions and quality', () => {
    expect(IMAGE_TIER_ORDER).toEqual(['tiny', 'medium', 'high', 'original']);
    expect(imageTier('tiny')).toMatchObject({ maxDim: 256, quality: 0.28, mime: 'image/avif' });
    expect(imageTier('medium')).toMatchObject({ maxDim: 512, quality: 0.4, mime: 'image/avif' });
    expect(imageTier('high')).toMatchObject({ maxDim: 1024, quality: 0.5, mime: 'image/avif' });
    expect(imageTier('original')).toMatchObject({ maxDim: null, quality: null, mime: 'original' });
    expect(IMAGE_TIERS).toHaveLength(4);
    expect(isImageTierId('tiny')).toBe(true);
    expect(isImageTierId('huge')).toBe(false);
  });

  it('declares AVIF-first encode order per image tier with unchanged geometry', () => {
    expect(IMAGE_ENCODE_MIMES).toEqual(['image/avif', 'image/webp', 'image/jpeg']);
    for (const id of ['tiny', 'medium', 'high'] as const) {
      const tier = imageTier(id);
      expect(tier.encodeMimes).toEqual(['image/avif', 'image/webp', 'image/jpeg']);
      // The shared preset declares AVIF as the preferred MIME; fallbacks stay available.
      expect(tier.mime).toBe('image/avif');
      expect(tier.maxDim).toBe(MEDIA_PRESETS.images[id].max_dim);
      expect(tier.quality).toBe(MEDIA_PRESETS.images[id].quality);
    }
    expect(imageTier('original').encodeMimes).toEqual([]);
  });

  it('filters the encode order by real browser capability', () => {
    expect(imageEncodeOrder({ avif: true, webp: true, jpeg: true })).toEqual(['image/avif', 'image/webp', 'image/jpeg']);
    // Chromium 159 cannot encode AVIF from canvas → WebP first, exactly the pre-AVIF behaviour.
    expect(imageEncodeOrder({ avif: false, webp: true, jpeg: true })).toEqual(['image/webp', 'image/jpeg']);
    expect(imageEncodeOrder({ avif: false, webp: false, jpeg: true })).toEqual(['image/jpeg']);
    expect(imageEncodeOrder({ avif: false, webp: false, jpeg: false })).toEqual([]);
    expect(imageEncodeOrderForTier('tiny', { avif: true, webp: false, jpeg: true })).toEqual(['image/avif', 'image/jpeg']);
    expect(imageEncodeOrderForTier('original', { avif: true, webp: true, jpeg: true })).toEqual([]);
  });

  it('sends the strictly smaller of AVIF and WebP, with WebP winning a tie', () => {
    const avif = { mime: 'image/avif' as const, bytes: 900 };
    const webp = { mime: 'image/webp' as const, bytes: 1_200 };
    expect(selectPreferredImageEncoding([avif, webp])).toBe('image/avif');
    expect(selectPreferredImageEncoding([{ mime: 'image/avif', bytes: 1_200 }, { mime: 'image/webp', bytes: 900 }])).toBe(
      'image/webp',
    );
    expect(selectPreferredImageEncoding([{ mime: 'image/avif', bytes: 1_000 }, { mime: 'image/webp', bytes: 1_000 }])).toBe(
      'image/webp',
    );
    // JPEG is only part of the comparison when WebP could not be produced.
    const jpeg = { mime: 'image/jpeg' as const, bytes: 700 };
    expect(selectPreferredImageEncoding([avif, jpeg])).toBe('image/jpeg');
    expect(selectPreferredImageEncoding([{ mime: 'image/webp', bytes: 500 }, jpeg])).toBe('image/webp');
    expect(selectPreferredImageEncoding([webp, jpeg])).toBe('image/jpeg');
    expect(selectPreferredImageEncoding([jpeg])).toBe('image/jpeg');
    expect(selectPreferredImageEncoding([{ mime: 'image/webp', bytes: 1_000 }, { mime: 'image/jpeg', bytes: 1_000 }])).toBe(
      'image/webp',
    );
    expect(selectPreferredImageEncoding([])).toBeNull();
  });

  it('chooses the MIME from the winning bytes and derives the matching upload filename', () => {
    const avifWins = selectPreferredImageEncoding([
      { mime: 'image/avif', bytes: 900 },
      { mime: 'image/webp', bytes: 1_000 },
    ]);
    const webpWins = selectPreferredImageEncoding([
      { mime: 'image/avif', bytes: 1_200 },
      { mime: 'image/webp', bytes: 900 },
    ]);
    const tie = selectPreferredImageEncoding([
      { mime: 'image/avif', bytes: 900 },
      { mime: 'image/webp', bytes: 900 },
    ]);
    expect(avifWins).toBe('image/avif');
    expect(encodedImageFilename('holiday.png', avifWins!)).toBe('holiday.avif');
    expect(webpWins).toBe('image/webp');
    expect(encodedImageFilename('holiday.png', webpWins!)).toBe('holiday.webp');
    expect(tie).toBe('image/webp');
  });

  it('derives extensions and upload filenames from the bytes actually encoded', () => {
    expect(imageExtensionForMime('image/avif')).toBe('avif');
    expect(imageExtensionForMime('image/webp')).toBe('webp');
    expect(imageExtensionForMime('image/jpeg')).toBe('jpg');
    expect(imageExtensionForMime('image/jpg')).toBe('jpg');
    expect(imageExtensionForMime('image/png')).toBe('png');
    expect(imageExtensionForMime('image/svg+xml')).toBe('svg');
    expect(imageExtensionForMime(null)).toBe('img');
    expect(encodedImageFilename('photo.jpg', 'image/avif')).toBe('photo.avif');
    expect(encodedImageFilename('IMG_1234.JPG', 'image/webp')).toBe('IMG_1234.webp');
    expect(encodedImageFilename('no-extension', 'image/avif')).toBe('no-extension.avif');
    expect(encodedImageFilename(null, 'image/avif')).toMatch(/^image-[a-z0-9]+\.avif$/);
  });

  it('exposes the three voice tiers with codec, mime and caps from the JSON', () => {
    expect(VOICE_TIER_ORDER).toEqual(['tiny', 'medium', 'high']);
    expect(voiceTier('tiny')).toMatchObject({ codec: 'codec2', mode: '1300', mime: 'audio/c2-1300', sampleRate: 8000, frameMs: 40, maxDurationS: 30 });
    expect(voiceTier('medium')).toMatchObject({ codec: 'codec2', mode: '3200', mime: 'audio/c2-3200', frameMs: 20, maxDurationS: 20 });
    expect(voiceTier('high')).toMatchObject({ codec: 'opus', bitsPerSecond: 12000, maxDurationS: 15 });
    expect(VOICE_TIERS).toHaveLength(3);
    expect(isVoiceTierId('medium')).toBe(true);
    expect(isVoiceTierId('original')).toBe(false);
  });
});

describe('image tier policy and auto-suggest', () => {
  const source = { sourceBytes: 900_000, sourceWidth: 3000, sourceHeight: 2000 };

  it('blocks Original on long-range profiles only', () => {
    expect(originalAllowedForProfile('LONG_FAST')).toBe(false);
    expect(originalAllowedForProfile('LONG_SLOW')).toBe(false);
    expect(originalAllowedForProfile('MEDIUM_FAST')).toBe(true);
    expect(originalAllowedForProfile('SHORT_TURBO')).toBe(true);
    expect(imageTierPolicy('LONG_FAST').originalAllowed).toBe(false);
    expect(imageTierPolicy('LONG_FAST').reason).toContain('long-range');
    expect(imageTierPolicy('MEDIUM_FAST').originalAllowed).toBe(true);
  });

  it('estimates tier bytes from the contract dimensions', () => {
    const original = estimateImageTierBytes('original', source);
    expect(original).toBe(source.sourceBytes);
    const tiny = estimateImageTierBytes('tiny', source);
    const high = estimateImageTierBytes('high', source);
    expect(tiny).toBeGreaterThan(0);
    expect(high).toBeGreaterThan(tiny);
    // Tiny is capped at 320 px → at most 320×320 pixels regardless of source.
    expect(tiny).toBeLessThan(320 * 320);
  });

  it('suggests the largest allowed tier that fits the profile cap', () => {
    const medium = suggestImageTier({ profileId: 'MEDIUM_FAST', ...source });
    const suggested = medium.estimates.find((entry) => entry.tier === medium.suggested)!;
    expect(suggested.allowed).toBe(true);
    expect(suggested.fitsCap).toBe(true);
    for (const entry of medium.estimates) {
      if (entry.bytes > suggested.bytes && entry.allowed) expect(entry.fitsCap).toBe(false);
    }
    expect(medium.capBytes).toBe(maxAttachmentBytes('MEDIUM_FAST'));

    const long = suggestImageTier({ profileId: 'LONG_FAST', ...source });
    expect(long.originalAllowed).toBe(false);
    expect(long.estimates.find((entry) => entry.tier === 'original')?.allowed).toBe(false);
    expect(long.suggested).not.toBe('original');

    const tiny = suggestImageTier({ profileId: 'LONG_SLOW', sourceBytes: 1_500_000, sourceWidth: 5000, sourceHeight: 5000 });
    // Falls back to Tiny when even the smallest tier cannot be estimated under the cap.
    expect(['tiny', 'medium', 'high']).toContain(tiny.suggested);
    expect(tiny.suggested).not.toBe('original');
  });

  it('can suggest Original when it is allowed and fits the profile cap', () => {
    const small = suggestImageTier({ profileId: 'MEDIUM_FAST', sourceBytes: 2_048, sourceWidth: 800, sourceHeight: 600 });
    expect(small.originalAllowed).toBe(true);
    expect(small.suggested).toBe('original');
    expect(small.estimates.find((entry) => entry.tier === 'original')).toMatchObject({ allowed: true, fitsCap: true, bytes: 2_048 });
  });

  it('formats the pre-send airtime + hourly-duty label', () => {
    const summary = tierAirtimeSummary(12_000, 'MEDIUM_FAST', 'EU868');
    expect(summary.airtimeMs).toBeGreaterThan(0);
    expect(summary.budgetPct).toBeGreaterThan(0);
    expect(summary.label).toContain('MEDIUM FAST');
    expect(summary.label).toContain('hourly duty budget');
    expect(summary.label).toMatch(/^≈ [\d.]+ s airtime/);
    expect(summary.chunks).toBeGreaterThan(0);
    const fairUse = tierAirtimeSummary(12_000, 'MEDIUM_FAST', 'US915');
    expect(fairUse.budgetPct).toBeNull();
    expect(fairUse.label).toContain('no duty cycle');
  });
});

describe('voice tier math', () => {
  it('uses whole Codec2 frames and bitrate math for Opus', () => {
    expect(codec2FrameBytes('tiny')).toBe(7);
    expect(codec2FrameBytes('medium')).toBe(8);
    expect(codec2FrameBytes('high')).toBeNull();
    expect(codec2TierIndex('tiny')).toBe(0);
    expect(codec2TierIndex('medium')).toBe(1);
    expect(codec2TierIndex('high')).toBe(2);
    expect(voiceTierBytesPerSecond('tiny')).toBe(162.5);
    expect(voiceTierBytesPerSecond('high')).toBe(1500);
    expect(estimateVoiceTierBytes(1000, 'tiny')).toBe(175);
    expect(estimateVoiceTierBytes(1000, 'medium')).toBe(400);
    expect(estimateVoiceTierBytes(1500, 'high')).toBe(2250);
    expect(estimateVoiceTierBytes(0, 'tiny')).toBe(0);
  });

  it('caps duration by the tier limit and the profile byte cap', () => {
    const tiny = voiceTierCapMs('tiny', 'MEDIUM_FAST');
    expect(tiny.tierCapMs).toBe(30_000);
    expect(tiny.capBytes).toBe(maxAttachmentBytes('MEDIUM_FAST'));
    expect(tiny.capMs).toBeLessThanOrEqual(tiny.tierCapMs);
    expect(tiny.capMs).toBeLessThanOrEqual(tiny.byteCapMs);
    const high = voiceTierCapMs('high', 'MEDIUM_FAST');
    expect(high.tierCapMs).toBe(15_000);
    expect(high.capMs).toBeLessThanOrEqual(15_000);
    // A byte cap below one frame yields an empty cap, not a negative duration.
    expect(voiceTierCapMs('tiny', 'LONG_SLOW').capMs).toBeGreaterThan(0);
    const slow = voiceTierCapMs('high', 'LONG_SLOW');
    expect(slow.capMs).toBe(Math.min(slow.tierCapMs, slow.byteCapMs));
  });

  it('derives Codec2 clip duration from the payload size when duration_ms is missing', () => {
    // 308 B at 1300 bps = 44 whole frames × 40 ms = 1.76 s (the live bridge fixture).
    expect(codec2PayloadDurationMs(308, 'audio/c2-1300')).toBe(1760);
    expect(codec2PayloadDurationMs(7, 'audio/c2-1300')).toBe(40);
    expect(codec2PayloadDurationMs(309, 'audio/c2-1300')).toBe(1760);
    expect(codec2PayloadDurationMs(400, 'audio/c2-3200')).toBe(1000);
    expect(codec2PayloadDurationMs(308, 'audio/c2-3200')).toBe(760);
    expect(codec2PayloadDurationMs(0, 'audio/c2-1300')).toBeNull();
    expect(codec2PayloadDurationMs(null, 'audio/c2-1300')).toBeNull();
    expect(codec2PayloadDurationMs(undefined, 'audio/c2-1300')).toBeNull();
    expect(codec2PayloadDurationMs(1000, 'audio/ogg; codecs=opus')).toBeNull();
    expect(codec2PayloadDurationMs(1000, 'image/webp')).toBeNull();
    expect(codec2PayloadDurationMs(1000, null)).toBeNull();
  });

  it('maps MIME types to tiers and formats incoming labels', () => {
    expect(voiceTierForMime('audio/c2-1300')).toBe('tiny');
    expect(voiceTierForMime('audio/c2-3200')).toBe('medium');
    expect(voiceTierForMime('audio/ogg; codecs=opus')).toBe('high');
    expect(voiceTierForMime('audio/mp4')).toBe('high');
    expect(voiceTierForMime('image/webp')).toBeNull();
    expect(isCodec2Mime('audio/c2-1300')).toBe(true);
    expect(isCodec2Mime('audio/ogg')).toBe(false);
    expect(voiceAttachmentLabel('audio/c2-1300', 7400, 1295)).toBe('Codec2 1.3k · 7.4 s · 1.3 KiB');
    expect(voiceAttachmentLabel('audio/ogg; codecs=opus', 5_000, 20_000)).toContain('Opus');
    expect(voiceAttachmentLabel('audio/ogg', 1_000, 2_000)).toContain('Opus');
    expect(voiceAttachmentLabel('audio/mp4', 3_000, 12_000)).toContain('AAC');
    expect(voiceAttachmentLabel('audio/aac', 3_000, 12_000)).toContain('AAC');
    expect(voiceAttachmentLabel('audio/mpeg', 3_000, 12_000)).toContain('MP3');
  });

  it('demo seeds a tiny Codec2 voice note plus the existing Opus note', () => {
    const dataset = createDemoDataset(new Date(2026, 2, 15, 12, 0, 0).getTime());
    const voiceMessages = dataset.messages.filter((message) => message.attachment?.mime.startsWith('audio/'));
    const tiny = voiceMessages.find((message) => message.attachment?.mime === 'audio/c2-1300');
    expect(tiny).toBeTruthy();
    expect(tiny?.attachment?.size).toBe(1_295);
    expect(voiceTierForMime(tiny?.attachment?.mime)).toBe('tiny');
    expect(voiceMessages.some((message) => message.attachment?.mime.includes('opus'))).toBe(true);
  });
});

describe('Codec2 wasm loader', () => {
  it('builds a base-safe URL and degrades gracefully when the artifact is absent', async () => {
    expect(codec2WasmUrl()).toContain('voice/openmesh_voice.wasm');
    resetCodec2CacheForTests();
    const fetchImpl = (async () => new Response('not found', { status: 404 })) as typeof fetch;
    const error = await loadCodec2(fetchImpl).catch((cause: unknown) => cause);
    expect(error).toBeInstanceOf(Error);
    expect(String(error)).toMatch(/Codec2 voice is unavailable/);
    resetCodec2CacheForTests();
  });

  it('rejects an invalid wasm artifact with a typed error', async () => {
    resetCodec2CacheForTests();
    const fetchImpl = (async () => new Response(new Uint8Array([0, 1, 2, 3]), { status: 200 })) as typeof fetch;
    const error = await loadCodec2(fetchImpl).catch((cause: unknown) => cause);
    expect(String(error)).toMatch(/WASM artifact is invalid/);
    resetCodec2CacheForTests();
  });

  const wasmPath = resolve(process.cwd(), 'public/voice/openmesh_voice.wasm');
  it.skipIf(!existsSync(wasmPath))('round-trips 1 s of 8 kHz PCM through the frozen ABI', async () => {
    resetCodec2CacheForTests();
    const wasm = readFileSync(wasmPath);
    const fetchImpl = (async () =>
      new Response(new Uint8Array(wasm), { status: 200, headers: { 'Content-Type': 'application/wasm' } })) as typeof fetch;
    const codec2 = await loadCodec2(fetchImpl);
    expect(codec2.frameBytes('tiny')).toBe(7);
    expect(codec2.frameBytes('medium')).toBe(8);
    const rate = 8000;
    const pcm = new Int16Array(rate);
    for (let i = 0; i < pcm.length; i += 1) pcm[i] = Math.round(Math.sin((2 * Math.PI * 440 * i) / rate) * 0.5 * 32767);
    const tinyPayload = codec2.encode(pcm, 'tiny');
    expect(tinyPayload.length).toBe(175);
    const decoded = codec2.decode(tinyPayload, 'tiny');
    expect(decoded.length).toBe(rate);
    const mediumPayload = codec2.encode(pcm, 'medium');
    expect(mediumPayload.length).toBe(400);
    expect(codec2.decode(mediumPayload, 'medium').length).toBe(rate);
    expect(() => codec2.encode(pcm, 'high')).toThrow();
    resetCodec2CacheForTests();
  });
});
