/**
 * LoRa airtime + resource size math.
 *
 * Normative sources:
 *  - `docs/architecture/PROTOCOL.md` §4 (240 B MTU), §9 (resources, chunking,
 *    size policy), §12 (timing constants), Appendix A (worked sizes).
 *  - `docs/architecture/WEBSOCKET-API.md` §6 (attachments).
 *
 * The estimator mirrors `util::lora_airtime_ms` (Semtech AN1200.13 formula) with
 * the conservative assumptions the firmware uses: explicit header, CRC on,
 * 8 preamble symbols, low-data-rate optimisation for SF11/SF12 at 125 kHz.
 * Numbers are deliberately close to the worked examples in the protocol spec
 * (5 KiB ≈ 20 s at MEDIUM_FAST); tests pin that agreement.
 */

/** Frame overhead before the body: version/type, ttl/flags, src, dst, seq. */
export const FRAME_HEADER_BYTES = 22;
/** One LoRa frame including header (PROTOCOL §4: `MTU = 240`). */
export const FRAME_MTU_BYTES = 240;
/** Default resource chunk size (PROTOCOL §9.2). */
export const RESOURCE_CHUNK_BYTES = 180;
/** Resource frames in flight (PROTOCOL §9.3 / §12). */
export const RESOURCE_WINDOW = 8;
/** Receiver acks every N chunks (PROTOCOL §9.3). */
export const RESOURCE_ACK_EVERY = 8;
/** Manifest is a single Data frame; used for frame-count estimates. */
export const MANIFEST_FRAME_BYTES = 121;
/** Resource RTO used for the transfer-time model (PROTOCOL §12). */
export const RESOURCE_RTO_MS = 3000;
/** Announce frame is ~200 B on the wire (PROTOCOL §6 / Appendix A). */
export const ANNOUNCE_FRAME_BYTES = 200;
/** Floor between announces (PROTOCOL §12). */
export const ANNOUNCE_MIN_INTERVAL_MS = 300_000;

/** Estimated CBOR overhead of an AppMessage excluding the text/attachment. */
const MESSAGE_CBOR_OVERHEAD = 72;
/** Sealed body overhead: payload_flags + nonce + Poly1305 tag. */
const SEAL_OVERHEAD_BYTES = 29;
/** Public identity attached to the first message to an unknown peer (PROTOCOL §3). */
export const PUBLIC_IDENTITY_BYTES = 72;

export interface RadioProfile {
  id: string;
  label: string;
  description: string;
  /** Bandwidth in kHz. */
  bwKHz: number;
  spreadingFactor: number;
  /** LoRa coding-rate denominator index: 1 = 4/5, 2 = 4/6, 3 = 4/7, 4 = 4/8. */
  codingRate: 1 | 2 | 3 | 4;
  /** Attachment cap from PROTOCOL §9.4. */
  attachmentCapBytes: number;
  /** Voice note cap from PROTOCOL §9.4. */
  voiceCapMs: number;
  /** Approximate raw bitrate, for the picker subtitle. */
  approxKbps: number;
}

function kib(n: number): number {
  return n * 1024;
}

/**
 * Radio presets exposed by the bridge. Caps marked `derived` are not in the
 * PROTOCOL §9.4 table and are interpolated conservatively (see README).
 */
export const RADIO_PROFILES: Record<string, RadioProfile> = {
  SHORT_TURBO: {
    id: 'SHORT_TURBO',
    label: 'Short Turbo',
    description: 'Fastest, shortest range. Best for dense camp/event meshes.',
    bwKHz: 500,
    spreadingFactor: 7,
    codingRate: 1,
    attachmentCapBytes: kib(24),
    voiceCapMs: 12_000,
    approxKbps: 21.9,
  },
  SHORT_FAST: {
    id: 'SHORT_FAST',
    label: 'Short Fast',
    description: 'Good balance for city distances; most common handheld preset.',
    bwKHz: 500,
    spreadingFactor: 9,
    codingRate: 1,
    attachmentCapBytes: kib(24),
    voiceCapMs: 12_000,
    approxKbps: 10.9,
  },
  SHORT_SLOW: {
    id: 'SHORT_SLOW',
    label: 'Short Slow',
    description: 'Slightly longer range than Short Fast at half the bitrate.',
    bwKHz: 500,
    spreadingFactor: 10,
    codingRate: 1,
    attachmentCapBytes: kib(16), // derived: between SHORT_FAST and MEDIUM_FAST
    voiceCapMs: 10_000,
    approxKbps: 5.5,
  },
  MEDIUM_FAST: {
    id: 'MEDIUM_FAST',
    label: 'Medium Fast',
    description: 'Balanced suburban preset. The bridge simulator default.',
    bwKHz: 250,
    spreadingFactor: 9,
    codingRate: 1,
    attachmentCapBytes: kib(12),
    voiceCapMs: 10_000,
    approxKbps: 5.5,
  },
  MEDIUM_SLOW: {
    id: 'MEDIUM_SLOW',
    label: 'Medium Slow',
    description: 'More range, slower media. Good for valley-to-ridge links.',
    bwKHz: 250,
    spreadingFactor: 10,
    codingRate: 1,
    attachmentCapBytes: kib(8), // derived: between MEDIUM_FAST and LONG_FAST
    voiceCapMs: 8_000,
    approxKbps: 2.7,
  },
  LONG_TURBO: {
    id: 'LONG_TURBO',
    label: 'Long Turbo',
    description: 'Long range with a wide band; needs a less busy spectrum.',
    bwKHz: 500,
    spreadingFactor: 11,
    codingRate: 4,
    attachmentCapBytes: kib(16), // derived
    voiceCapMs: 12_000,
    approxKbps: 0.67,
  },
  LONG_FAST: {
    id: 'LONG_FAST',
    label: 'Long Fast',
    description: 'Long-range hiking preset. Media is a treat, not a habit.',
    bwKHz: 125,
    spreadingFactor: 11,
    codingRate: 1,
    attachmentCapBytes: kib(6),
    voiceCapMs: 8_000,
    approxKbps: 1.07,
  },
  LONG_SLOW: {
    id: 'LONG_SLOW',
    label: 'Long Slow',
    description: 'Maximum range, slowest. Text-only in practice.',
    bwKHz: 125,
    spreadingFactor: 12,
    codingRate: 4,
    attachmentCapBytes: kib(2),
    voiceCapMs: 5_000,
    approxKbps: 0.34,
  },
};

export const DEFAULT_PROFILE_ID = 'MEDIUM_FAST';

export function getProfile(profileId: string | null | undefined): RadioProfile {
  if (profileId && RADIO_PROFILES[profileId]) return RADIO_PROFILES[profileId]!;
  return RADIO_PROFILES[DEFAULT_PROFILE_ID]!;
}

export function profileLabel(profileId: string | null | undefined): string {
  return getProfile(profileId).label;
}

export const PROFILE_IDS = Object.keys(RADIO_PROFILES);

export interface Region {
  id: string;
  label: string;
  /** Regulatory duty cycle in percent, or null when the region uses dwell time instead. */
  dutyCyclePct: number | null;
  /** Max conducted power typically allowed for the default sub-band. */
  maxTxPowerDbm: number;
  frequencyLabel: string;
  notes: string;
}

export const REGIONS: Record<string, Region> = {
  EU868: {
    id: 'EU868',
    label: 'Europe 868 MHz (EU868)',
    dutyCyclePct: 1,
    maxTxPowerDbm: 14,
    frequencyLabel: '869.525 MHz',
    notes: 'ETSI 1 % duty cycle on the default sub-band. Media transfers are budgeted.',
  },
  EU433: {
    id: 'EU433',
    label: 'Europe 433 MHz (EU433)',
    dutyCyclePct: 10,
    maxTxPowerDbm: 10,
    frequencyLabel: '433.175 MHz',
    notes: 'Wider budget, lower permitted power.',
  },
  US915: {
    id: 'US915',
    label: 'North America 915 MHz (US915)',
    dutyCyclePct: null,
    maxTxPowerDbm: 30,
    frequencyLabel: '906.875 MHz',
    notes: 'FCC Part 15: no duty cycle, 400 ms dwell time per channel.',
  },
  AS923: {
    id: 'AS923',
    label: 'Asia 923 MHz (AS923)',
    dutyCyclePct: 1,
    maxTxPowerDbm: 16,
    frequencyLabel: '923.200 MHz',
    notes: '1 % duty cycle in most administrations; check local rules.',
  },
  AU915: {
    id: 'AU915',
    label: 'Australia 915 MHz (AU915)',
    dutyCyclePct: null,
    maxTxPowerDbm: 30,
    frequencyLabel: '916.800 MHz',
    notes: 'No general duty cycle; 400 ms dwell time applies.',
  },
  IN865: {
    id: 'IN865',
    label: 'India 865 MHz (IN865)',
    dutyCyclePct: null,
    maxTxPowerDbm: 30,
    frequencyLabel: '865.0625 MHz',
    notes: 'No general duty cycle; power limits apply.',
  },
  KR920: {
    id: 'KR920',
    label: 'South Korea 920 MHz (KR920)',
    dutyCyclePct: null,
    maxTxPowerDbm: 14,
    frequencyLabel: '921.900 MHz',
    notes: 'LBT (listen-before-talk) required.',
  },
  RU864: {
    id: 'RU864',
    label: 'Russia 864 MHz (RU864)',
    dutyCyclePct: 1,
    maxTxPowerDbm: 14,
    frequencyLabel: '868.900 MHz',
    notes: '1 % duty cycle on the default sub-band.',
  },
};

export const DEFAULT_REGION_ID = 'EU868';
export const REGION_IDS = Object.keys(REGIONS);

export function getRegion(regionId: string | null | undefined): Region {
  if (regionId && REGIONS[regionId]) return REGIONS[regionId]!;
  return { id: 'UNKNOWN', label: regionId ?? 'Unknown region', dutyCyclePct: null, maxTxPowerDbm: 14, frequencyLabel: '—', notes: 'Region not known to this client build; verify local rules.' };
}

/** True when SF11/SF12 are used on a 125 kHz channel — lowers effective bitrate. */
function lowDataRateOptimize(profile: RadioProfile): boolean {
  return profile.spreadingFactor >= 11 && profile.bwKHz <= 125;
}

/**
 * Semtech LoRa time-on-air in milliseconds.
 * `payloadBytes` is the full PHY payload (frame header included).
 */
export function loraAirtimeMs(payloadBytes: number, profileInput: string | RadioProfile): number {
  const profile = typeof profileInput === 'string' ? getProfile(profileInput) : profileInput;
  const payload = Math.max(0, Math.ceil(payloadBytes));
  const bwHz = profile.bwKHz * 1000;
  const tsymMs = ((2 ** profile.spreadingFactor) / bwHz) * 1000;
  const preambleMs = (8 + 4.25) * tsymMs;
  const de = lowDataRateOptimize(profile) ? 1 : 0;
  const numerator = 8 * payload - 4 * profile.spreadingFactor + 28 + 16; // CRC on, explicit header
  const denominator = 4 * (profile.spreadingFactor - 2 * de);
  const symbolCount = Math.ceil(numerator / denominator);
  const payloadSymbols = 8 + Math.max(symbolCount, 0) * (profile.codingRate + 4);
  return preambleMs + payloadSymbols * tsymMs;
}

/** Whole frames needed for `bytes` of resource payload (PROTOCOL §9.2). */
export function chunkCount(bytes: number, chunkBytes = RESOURCE_CHUNK_BYTES): number {
  if (!Number.isFinite(bytes) || bytes <= 0) return 0;
  return Math.ceil(bytes / chunkBytes);
}

/** Airtime of one resource chunk frame, including header + resource fields. */
export function chunkFrameBytes(chunkBytes = RESOURCE_CHUNK_BYTES): number {
  return FRAME_HEADER_BYTES + 1 + 4 + 2 + chunkBytes;
}

export interface AttachmentEstimate {
  bytes: number;
  chunks: number;
  /** Chunk frames + manifest + resource acks (window/completion). */
  totalFrames: number;
  chunkFrames: number;
  ackFrames: number;
  manifestFrames: number;
  /** Raw radio time for every frame, ms. */
  airtimeMs: number;
  /** Wall-clock transfer estimate including ARQ backoff, ms. */
  transferMs: number;
  /** Wall-clock at the region's duty cycle (queueing included), ms. */
  wallClockMs: number;
  windowCount: number;
  /** Profile attachment cap, bytes. */
  capBytes: number;
  withinCap: boolean;
  percentOfCap: number;
  profileId: string;
  regionId: string;
}

function roundUpTo(value: number, step: number): number {
  return Math.ceil(value / step) * step;
}

/**
 * Full estimate for sending an attachment: chunk count, frame count, airtime,
 * transfer time and duty-cycle cost. This is the function behind the "~18 s
 * airtime, 29 chunks" chip in the composer.
 */
export function estimateAttachment(
  bytes: number,
  profileInput: string | RadioProfile = DEFAULT_PROFILE_ID,
  regionInput: string | Region = DEFAULT_REGION_ID,
): AttachmentEstimate {
  const profile = typeof profileInput === 'string' ? getProfile(profileInput) : profileInput;
  const region = typeof regionInput === 'string' ? getRegion(regionInput) : regionInput;
  const safeBytes = Math.max(0, Math.ceil(bytes || 0));
  const chunks = chunkCount(safeBytes);
  const chunkFrames = chunks;
  const ackFrames = chunks === 0 ? 0 : Math.ceil(chunks / RESOURCE_ACK_EVERY) + 1;
  const manifestFrames = chunks === 0 ? 0 : 1;
  const chunkAirtime = chunkFrames * loraAirtimeMs(chunkFrameBytes(), profile);
  const ackAirtime = ackFrames * loraAirtimeMs(72, profile);
  const manifestAirtime = manifestFrames * loraAirtimeMs(MANIFEST_FRAME_BYTES, profile);
  const airtimeMs = Math.round(chunkAirtime + ackAirtime + manifestAirtime);
  // ARQ: 5 % retransmits + one RTO pause per window for selective-repeat pacing.
  const windowCount = Math.ceil(chunks / RESOURCE_WINDOW);
  const retransmitMs = airtimeMs * 0.05;
  const pacingMs = windowCount * 250;
  const transferMs = Math.round(airtimeMs + retransmitMs + pacingMs);
  const wallClockMs =
    region.dutyCyclePct && region.dutyCyclePct > 0 && region.dutyCyclePct < 100
      ? Math.round(airtimeMs / (region.dutyCyclePct / 100))
      : transferMs;
  const capBytes = profile.attachmentCapBytes;
  return {
    bytes: safeBytes,
    chunks,
    totalFrames: chunkFrames + ackFrames + manifestFrames,
    chunkFrames,
    ackFrames,
    manifestFrames,
    airtimeMs,
    transferMs,
    wallClockMs,
    windowCount,
    capBytes,
    withinCap: safeBytes <= capBytes,
    percentOfCap: capBytes > 0 ? Math.min(100, (safeBytes / capBytes) * 100) : 0,
    profileId: profile.id,
    regionId: region.id,
  };
}

/** UTF-8 byte length without allocating a Node Buffer (works in browsers). */
export function utf8ByteLength(text: string): number {
  return new TextEncoder().encode(text).length;
}

export interface TextEstimate {
  bytes: number;
  frames: number;
  airtimeMs: number;
  /** False when the message cannot fit a single frame and must become a resource. */
  fitsSingleFrame: boolean;
}

/**
 * Estimate a direct text message. `identityIncluded` mirrors PROTOCOL §5 when
 * the first message to a peer carries the 72-byte public identity.
 */
export function estimateText(
  text: string,
  profileInput: string | RadioProfile = DEFAULT_PROFILE_ID,
  options: { identityIncluded?: boolean; replyTo?: boolean } = {},
): TextEstimate {
  const profile = typeof profileInput === 'string' ? getProfile(profileInput) : profileInput;
  const bytes =
    MESSAGE_CBOR_OVERHEAD +
    utf8ByteLength(text) +
    (options.replyTo ? 5 : 0) +
    (options.identityIncluded ? PUBLIC_IDENTITY_BYTES : 0);
  const bodyBytes = 1 + bytes + SEAL_OVERHEAD_BYTES;
  const frameBytes = FRAME_HEADER_BYTES + bodyBytes;
  const fitsSingleFrame = frameBytes <= FRAME_MTU_BYTES;
  const frames = fitsSingleFrame ? 1 : Math.ceil(bodyBytes / (RESOURCE_CHUNK_BYTES * 0.95));
  const airtimeMs = Math.round(
    fitsSingleFrame ? loraAirtimeMs(frameBytes, profile) : estimateAttachment(bodyBytes, profile).airtimeMs,
  );
  return { bytes: bodyBytes, frames, airtimeMs, fitsSingleFrame };
}

/** Airtime a single announce costs (PROTOCOL §6 / Appendix A). */
export function announceAirtimeMs(profileInput: string | RadioProfile = DEFAULT_PROFILE_ID): number {
  const profile = typeof profileInput === 'string' ? getProfile(profileInput) : profileInput;
  return Math.round(loraAirtimeMs(ANNOUNCE_FRAME_BYTES, profile));
}

/** Milliseconds of transmit time available per hour in a region (null = unlimited). */
export function hourlyAirtimeBudgetMs(regionInput: string | Region = DEFAULT_REGION_ID): number | null {
  const region = typeof regionInput === 'string' ? getRegion(regionInput) : regionInput;
  if (region.dutyCyclePct === null) return null;
  return Math.round((region.dutyCyclePct / 100) * 3_600_000);
}

/** How many attachment bytes fit in one hour of duty-cycle budget. */
export function attachmentBytesPerHour(
  profileInput: string | RadioProfile = DEFAULT_PROFILE_ID,
  regionInput: string | Region = DEFAULT_REGION_ID,
): number | null {
  const profile = typeof profileInput === 'string' ? getProfile(profileInput) : profileInput;
  const budget = hourlyAirtimeBudgetMs(regionInput);
  if (budget === null) return null;
  const bytesPerFrame = chunkFrameBytes();
  const airtimePerFrame = loraAirtimeMs(bytesPerFrame, profile);
  const frames = Math.floor(budget / airtimePerFrame);
  return Math.max(0, frames * RESOURCE_CHUNK_BYTES);
}

export type DutyWarningLevel = 'ok' | 'warn' | 'danger';

export interface DutyCycleAssessment {
  level: DutyWarningLevel;
  /** Share of the hourly budget consumed by announces alone, 0..100+. */
  announceBudgetPct: number;
  announcesPerHour: number;
  budgetMs: number | null;
  message: string;
}

/**
 * Sanity-check announce interval against the region duty cycle. This powers the
 * settings warning and stops users from picking a 5-minute announce on EU868 by
 * accident.
 */
export function assessDutyCycle(
  profileId: string,
  regionId: string,
  announceIntervalS: number,
  extraAirtimeMsPerHour = 0,
): DutyCycleAssessment {
  const budget = hourlyAirtimeBudgetMs(regionId);
  const intervalS = Math.max(ANNOUNCE_MIN_INTERVAL_MS / 1000, announceIntervalS || 0);
  if (budget === null) {
    return {
      level: 'ok',
      announceBudgetPct: 0,
      announcesPerHour: 3600 / intervalS,
      budgetMs: null,
      message: 'No duty cycle in this region — dwell time and polite-use limits still apply.',
    };
  }
  const announcesPerHour = 3600 / intervalS;
  const announceMs = announcesPerHour * announceAirtimeMs(profileId);
  const usedPct = ((announceMs + extraAirtimeMsPerHour) / budget) * 100;
  const announceBudgetPct = (announceMs / budget) * 100;
  let level: DutyWarningLevel = 'ok';
  if (usedPct >= 30) level = 'danger';
  else if (usedPct >= 10) level = 'warn';
  const message =
    level === 'ok'
      ? `Announces use ${announceBudgetPct.toFixed(1)}% of the ${getRegion(regionId).dutyCyclePct}% duty-cycle budget.`
      : level === 'warn'
        ? `Announces use ${announceBudgetPct.toFixed(1)}% of the hourly budget — media may be delayed.`
        : `Announces alone use ${announceBudgetPct.toFixed(1)}% of the hourly budget. Raise the interval or pick a slower announce.`;
  return { level, announceBudgetPct, announcesPerHour, budgetMs: budget, message };
}

/** Max voice duration for a profile, ms (PROTOCOL §9.4). */
export function voiceCapMs(profileId: string | null | undefined): number {
  return getProfile(profileId).voiceCapMs;
}

/** Max attachment bytes for a profile (PROTOCOL §9.4). */
export function maxAttachmentBytes(profileId: string | null | undefined): number {
  return getProfile(profileId).attachmentCapBytes;
}

/** Round a value up to a "nice" step for sliders/labels. */
export function niceCeil(value: number, step = 1024): number {
  return Math.max(step, roundUpTo(value, step));
}
