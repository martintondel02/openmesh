/** PCM capture + 8 kHz resampling + playback helpers for Codec2 voice (media-tiers §1/§5). */
import type { VoiceTierId } from '../lib/media-tiers';
import { decodePcmCodec2 } from './codec2';

export const CODEC2_SAMPLE_RATE = 8000;

export interface PcmCaptureHandle {
  /** Collected mono PCM at the capture sample rate. */
  stop: () => Promise<Float32Array>;
  /** Capture sample rate (Hz). */
  sampleRate: number;
  /** Peak level 0..1 of the most recent block. */
  level: () => number;
  durationMs: () => number;
  cancel: () => void;
}

/** Capture mono Float32 PCM from the microphone with a ScriptProcessor (widely supported). */
export async function startPcmCapture(): Promise<PcmCaptureHandle> {
  if (!navigator.mediaDevices?.getUserMedia) throw new Error('This browser cannot capture audio.');
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true },
  });
  const AudioContextClass =
    window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!AudioContextClass) {
    for (const track of stream.getTracks()) track.stop();
    throw new Error('Web Audio is not available in this browser.');
  }
  const context = new AudioContextClass();
  if (context.state === 'suspended') await context.resume().catch(() => undefined);
  const source = context.createMediaStreamSource(stream);
  const processor = context.createScriptProcessor(4096, 1, 1);
  const chunks: Float32Array[] = [];
  let level = 0;
  let stopped = false;
  const startedAt = performance.now();
  processor.onaudioprocess = (event) => {
    if (stopped) return;
    const channels = event.inputBuffer.numberOfChannels;
    const length = event.inputBuffer.length;
    const mono = new Float32Array(length);
    for (let channel = 0; channel < channels; channel += 1) {
      const data = event.inputBuffer.getChannelData(channel);
      for (let i = 0; i < length; i += 1) mono[i] += data[i]! / channels;
    }
    let peak = 0;
    for (let i = 0; i < mono.length; i += 1) peak = Math.max(peak, Math.abs(mono[i]!));
    level = Math.min(1, peak);
    chunks.push(mono);
  };
  source.connect(processor);
  processor.connect(context.destination);
  const cleanup = () => {
    stopped = true;
    processor.disconnect();
    source.disconnect();
    for (const track of stream.getTracks()) track.stop();
    if (context.state !== 'closed') void context.close().catch(() => undefined);
  };
  return {
    sampleRate: context.sampleRate,
    level: () => level,
    durationMs: () => performance.now() - startedAt,
    stop: async () => {
      const elapsed = performance.now() - startedAt;
      cleanup();
      const total = chunks.reduce((sum, chunk) => sum + chunk.length, 0);
      const merged = new Float32Array(total);
      let offset = 0;
      for (const chunk of chunks) {
        merged.set(chunk, offset);
        offset += chunk.length;
      }
      // Drop trailing audio beyond the wall-clock duration to avoid capture latency tails.
      const wanted = Math.max(1, Math.min(merged.length, Math.ceil((context.sampleRate * elapsed) / 1000)));
      return merged.subarray(0, wanted);
    },
    cancel: cleanup,
  };
}

/** Resample mono float PCM to 8 kHz using an OfflineAudioContext (high-quality SRC). */
export async function resampleTo8kMono(samples: Float32Array, inputRate: number): Promise<Float32Array> {
  const length = Math.max(1, Math.ceil((samples.length / inputRate) * CODEC2_SAMPLE_RATE));
  const offline = new OfflineAudioContext(1, length, CODEC2_SAMPLE_RATE);
  const buffer = offline.createBuffer(1, Math.max(1, samples.length), inputRate);
  buffer.copyToChannel(samples as Float32Array<ArrayBuffer>, 0);
  const source = offline.createBufferSource();
  source.buffer = buffer;
  source.connect(offline.destination);
  source.start();
  const rendered = await offline.startRendering();
  return rendered.getChannelData(0).slice();
}

export function floatToPcm16(samples: Float32Array): Int16Array {
  const pcm = new Int16Array(samples.length);
  for (let i = 0; i < samples.length; i += 1) {
    const value = Math.max(-1, Math.min(1, samples[i]!));
    pcm[i] = value < 0 ? Math.round(value * 0x8000) : Math.round(value * 0x7fff);
  }
  return pcm;
}

export function pcm16ToFloat(pcm: Int16Array): Float32Array {
  const float = new Float32Array(pcm.length);
  for (let i = 0; i < pcm.length; i += 1) float[i] = pcm[i]! / 0x8000;
  return float;
}

export function pcmDurationMs(pcm: Int16Array): number {
  return Math.round((pcm.length / CODEC2_SAMPLE_RATE) * 1000);
}

/** Simple waveform peaks for the recorder/bubble (32 bars). */
export function pcmWaveform(pcm: Int16Array, bars = 32): number[] {
  if (pcm.length === 0) return Array.from({ length: bars }, () => 0.08);
  const block = Math.max(1, Math.floor(pcm.length / bars));
  const result: number[] = [];
  for (let bar = 0; bar < bars; bar += 1) {
    let peak = 0;
    const start = bar * block;
    const end = Math.min(pcm.length, start + block);
    for (let i = start; i < end; i += 1) peak = Math.max(peak, Math.abs(pcm[i]!));
    result.push(Math.max(0.05, Math.min(1, (peak / 0x8000) * 3)));
  }
  return result;
}

/** Encode a WAV copy purely for local preview / outgoing bubble fallback. */
export function pcmToWavBlob(pcm: Int16Array, sampleRate = CODEC2_SAMPLE_RATE): Blob {
  const buffer = new ArrayBuffer(44 + pcm.byteLength);
  const view = new DataView(buffer);
  const writeString = (offset: number, text: string) => {
    for (let i = 0; i < text.length; i += 1) view.setUint8(offset + i, text.charCodeAt(i));
  };
  writeString(0, 'RIFF');
  view.setUint32(4, 36 + pcm.byteLength, true);
  writeString(8, 'WAVE');
  writeString(12, 'fmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  writeString(36, 'data');
  view.setUint32(40, pcm.byteLength, true);
  new Int16Array(buffer, 44).set(pcm);
  return new Blob([buffer], { type: 'audio/wav' });
}

export interface CodedAudioPlayback {
  stop: () => void;
  durationMs: number;
}

/** Decode a Codec2 payload with the WASM and play it as an AudioBuffer at 8 kHz. */
export async function playCodec2Payload(payload: Uint8Array, tier: VoiceTierId): Promise<CodedAudioPlayback> {
  const pcm = await decodePcmCodec2(payload, tier);
  const AudioContextClass =
    window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!AudioContextClass) throw new Error('Web Audio is not available in this browser.');
  const context = new AudioContextClass();
  const buffer = context.createBuffer(1, Math.max(1, pcm.length), CODEC2_SAMPLE_RATE);
  buffer.copyToChannel(pcm16ToFloat(pcm) as Float32Array<ArrayBuffer>, 0);
  const source = context.createBufferSource();
  source.buffer = buffer;
  source.connect(context.destination);
  source.start();
  return {
    durationMs: pcmDurationMs(pcm),
    stop: () => {
      try {
        source.stop();
      } catch {
        // already stopped
      }
      void context.close().catch(() => undefined);
    },
  };
}
