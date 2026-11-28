import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AvifEncodeRequest, AvifWorkerResponse } from '../avif-protocol';

function avifHeader(brand = 'avif'): ArrayBuffer {
  const bytes = new Uint8Array(16);
  bytes.set([0x00, 0x00, 0x00, 0x10], 0);
  bytes.set([...'ftyp'].map((character) => character.charCodeAt(0)), 4);
  bytes.set([...brand].map((character) => character.charCodeAt(0)), 8);
  return bytes.buffer;
}

type FakeMode = 'success' | 'fetch-error' | 'non-avif';

class FakeWorker {
  static instances: FakeWorker[] = [];
  static mode: FakeMode = 'success';

  onmessage: ((event: MessageEvent<AvifWorkerResponse>) => void) | null = null;
  onerror: ((event: ErrorEvent) => void) | null = null;
  onmessageerror: ((event: MessageEvent) => void) | null = null;
  readonly posted: AvifEncodeRequest[] = [];

  constructor(..._args: unknown[]) {
    FakeWorker.instances.push(this);
  }

  postMessage(request: AvifEncodeRequest): void {
    this.posted.push(request);
    const reply: AvifWorkerResponse =
      FakeWorker.mode === 'fetch-error'
        ? { type: 'error', id: request.id, reason: 'fetch', message: 'encoder blocked' }
        : FakeWorker.mode === 'non-avif'
          ? {
              type: 'encoded',
              id: request.id,
              bytes: new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0, 0, 0, 0, 0, 0, 0, 0]).buffer,
              ms: 1,
              quality: 35,
              webpQuality: 28,
              speed: 10,
            }
          : {
              type: 'encoded',
              id: request.id,
              bytes: avifHeader(),
              ms: 7,
              quality: 35,
              webpQuality: 28,
              speed: 10,
            };
    queueMicrotask(() => this.onmessage?.({ data: reply } as MessageEvent<AvifWorkerResponse>));
  }

  terminate(): void {}
}

async function loadModule() {
  vi.resetModules();
  return import('../avif-encoder');
}

describe('avif encoder loader: lazy load-once and graceful failure', () => {
  beforeEach(() => {
    FakeWorker.instances = [];
    FakeWorker.mode = 'success';
    vi.stubGlobal('Worker', FakeWorker as unknown as typeof Worker);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('creates no Worker at import time and loads the encoder only on the first encode', async () => {
    const { encodeAvifRgba, avifEncoderState } = await loadModule();
    expect(avifEncoderState()).toBe('idle');
    expect(FakeWorker.instances).toHaveLength(0);

    const first = await encodeAvifRgba(new Uint8ClampedArray(4 * 4 * 4), 4, 4, 0.28);
    const second = await encodeAvifRgba(new Uint8ClampedArray(4 * 4 * 4), 4, 4, 0.4);

    expect(FakeWorker.instances).toHaveLength(1);
    expect(FakeWorker.instances[0]!.posted).toHaveLength(2);
    expect(avifEncoderState()).toBe('available');
    expect(first.bytes.byteLength).toBe(16);
    expect(second.quality).toBe(35);
    expect(second.webpQuality).toBe(28);
  });

  it('returns error details and marks the encoder unavailable without retrying a failed load', async () => {
    FakeWorker.mode = 'fetch-error';
    const { encodeAvifRgba, avifEncoderState, avifEncoderUnavailableReason } = await loadModule();

    await expect(encodeAvifRgba(new Uint8ClampedArray(4 * 4 * 4), 4, 4, 0.4)).rejects.toMatchObject({
      name: 'AvifEncoderError',
      reason: 'fetch',
    });
    await expect(encodeAvifRgba(new Uint8ClampedArray(4 * 4 * 4), 4, 4, 0.4)).rejects.toMatchObject({
      reason: 'unsupported',
    });
    expect(FakeWorker.instances).toHaveLength(1);
    expect(avifEncoderState()).toBe('unavailable');
    expect(avifEncoderUnavailableReason()).toContain('encoder blocked');
  });

  it('rejects non-AVIF output instead of mislabelling it', async () => {
    FakeWorker.mode = 'non-avif';
    const { encodeAvifRgba } = await loadModule();
    await expect(encodeAvifRgba(new Uint8ClampedArray(4 * 4 * 4), 4, 4, 0.4)).rejects.toMatchObject({
      reason: 'invalid',
    });
  });
});
