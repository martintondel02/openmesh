/**
 * Voice-note helpers: waveform extraction for the recorder/playback UI and a
 * tiny WAV synthesiser used by demo mode (so seeded voice bubbles are actually
 * playable without a bridge).
 */

import { base64ToBytes, bytesToBase64 } from './utils';

/** Peak-downsample arbitrary samples into `bars` normalised amplitudes (0..1). */
export function downsampleWaveform(samples: ArrayLike<number>, bars = 32): number[] {
  const total = samples.length;
  if (total === 0 || bars <= 0) return [];
  const bucket = Math.max(1, Math.floor(total / bars));
  const peaks: number[] = [];
  for (let bar = 0; bar < bars; bar += 1) {
    const start = bar * bucket;
    const end = Math.min(total, start + bucket);
    let peak = 0;
    for (let i = start; i < end; i += 1) {
      const value = Math.abs(samples[i] ?? 0);
      if (value > peak) peak = value;
    }
    peaks.push(peak);
  }
  const max = peaks.reduce((m, p) => Math.max(m, p), 0);
  if (max === 0) return peaks.map(() => 0);
  return peaks.map((p) => Math.round((p / max) * 100) / 100);
}

/** Convert live analyser time-domain bytes (128 = silence) into display bars. */
export function waveformFromAnalyser(data: Uint8Array, bars = 32): number[] {
  const floats = new Float32Array(data.length);
  for (let i = 0; i < data.length; i += 1) floats[i] = Math.abs((data[i] ?? 128) / 128 - 1);
  return downsampleWaveform(floats, bars);
}

/** Deterministic 32-bit PRNG (mulberry32) shared by demo data + waveform tests. */
export function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** Speech-cadence envelope (syllable bursts) for demo waveforms/audio. */
export function speechEnvelope(durationMs: number, seed = 7): number[] {
  const rand = mulberry32(seed);
  const steps = Math.max(1, Math.round(durationMs / 80));
  const envelope: number[] = [];
  let energy = 0.2;
  for (let i = 0; i < steps; i += 1) {
    // Occasional pauses, mostly sustained syllables.
    const pause = rand() < 0.12;
    energy = pause ? 0.05 : 0.35 + rand() * 0.6;
    envelope.push(Math.min(1, energy));
  }
  // Light smoothing so bars look organic rather than spiky.
  return envelope.map((v, i) => {
    const prev = envelope[i - 1] ?? v;
    const next = envelope[i + 1] ?? v;
    return Math.round(((prev + v * 2 + next) / 4) * 100) / 100;
  });
}

export function speechWaveform(durationMs: number, bars = 32, seed = 7): number[] {
  const envelope = speechEnvelope(durationMs, seed);
  const step = envelope.length / bars;
  const out: number[] = [];
  for (let bar = 0; bar < bars; bar += 1) {
    const start = Math.floor(bar * step);
    const end = Math.max(start + 1, Math.floor((bar + 1) * step));
    let peak = 0;
    for (let i = start; i < end && i < envelope.length; i += 1) peak = Math.max(peak, envelope[i] ?? 0);
    out.push(Math.round(peak * 100) / 100);
  }
  return out;
}

const WAV_SAMPLE_RATE = 8000;

/**
 * Synthesise a hushed, speech-like clip (formant-ish tones with syllable
 * envelope) for demo voice notes. Returns mono 16-bit samples.
 */
export function synthesizeDemoVoice(durationMs: number, seed = 7, sampleRate = WAV_SAMPLE_RATE): Float32Array {
  const length = Math.max(1, Math.round((durationMs / 1000) * sampleRate));
  const samples = new Float32Array(length);
  const rand = mulberry32(seed);
  const envelope = speechEnvelope(durationMs, seed);
  let phase = 0;
  let phase2 = 0;
  for (let i = 0; i < length; i += 1) {
    const t = i / sampleRate;
    const envelopeIndex = Math.min(envelope.length - 1, Math.floor((i / length) * envelope.length));
    const amp = (envelope[envelopeIndex] ?? 0) * 0.16;
    // Fundamental + shifted formant, slowly wobbling like speech.
    const f0 = 118 + Math.sin(t * 2.3 + rand() * 0.02) * 12;
    phase += (2 * Math.PI * f0) / sampleRate;
    phase2 += (2 * Math.PI * (f0 * 2.6)) / sampleRate;
    const tone = Math.sin(phase) * 0.7 + Math.sin(phase2) * 0.25;
    const noise = (rand() - 0.5) * 0.06;
    samples[i] = (tone + noise) * amp;
  }
  return samples;
}

/** Encode mono Float32 samples as a 16-bit PCM WAV byte array. */
export function encodeWav(samples: Float32Array, sampleRate = WAV_SAMPLE_RATE): Uint8Array {
  const bytesPerSample = 2;
  const dataSize = samples.length * bytesPerSample;
  const buffer = new ArrayBuffer(44 + dataSize);
  const view = new DataView(buffer);
  const writeString = (offset: number, text: string) => {
    for (let i = 0; i < text.length; i += 1) view.setUint8(offset + i, text.charCodeAt(i));
  };
  writeString(0, 'RIFF');
  view.setUint32(4, 36 + dataSize, true);
  writeString(8, 'WAVE');
  writeString(12, 'fmt ');
  view.setUint32(16, 16, true); // PCM chunk size
  view.setUint16(20, 1, true); // format = PCM
  view.setUint16(22, 1, true); // mono
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * bytesPerSample, true);
  view.setUint16(32, bytesPerSample, true);
  view.setUint16(34, 16, true);
  writeString(36, 'data');
  view.setUint32(40, dataSize, true);
  for (let i = 0; i < samples.length; i += 1) {
    const clamped = Math.max(-1, Math.min(1, samples[i] ?? 0));
    view.setInt16(44 + i * 2, Math.round(clamped * 0x7fff), true);
  }
  return new Uint8Array(buffer);
}

/** Data URL for a demo voice clip, safe for `<audio src>` and `<a download>`. */
export function demoVoiceDataUrl(durationMs: number, seed = 7): string {
  const wav = encodeWav(synthesizeDemoVoice(durationMs, seed));
  return `data:audio/wav;base64,${bytesToBase64(wav)}`;
}

/** Decode a WAV/OGG data URL back into bytes (used when sending a demo clip). */
export function dataUrlToBytes(dataUrl: string): Uint8Array | null {
  const comma = dataUrl.indexOf(',');
  if (comma < 0) return null;
  const meta = dataUrl.slice(0, comma);
  const payload = dataUrl.slice(comma + 1);
  if (meta.includes(';base64')) {
    try {
      return base64ToBytes(payload);
    } catch {
      return null;
    }
  }
  try {
    return new TextEncoder().encode(decodeURIComponent(payload));
  } catch {
    return null;
  }
}

/** `audio/ogg; codecs=opus` per PROTOCOL §9.4; falls back to whatever MediaRecorder gives. */
export function pickVoiceMimeType(isTypeSupported?: (mime: string) => boolean): string {
  const supported = isTypeSupported ?? (() => false);
  const candidates = [
    'audio/ogg;codecs=opus',
    'audio/ogg; codecs=opus',
    'audio/webm;codecs=opus',
    'audio/webm',
  ];
  for (const candidate of candidates) {
    try {
      if (supported(candidate)) return candidate;
    } catch {
      // fall through
    }
  }
  return 'audio/webm';
}
