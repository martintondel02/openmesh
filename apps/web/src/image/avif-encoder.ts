/**
 * Lazy main-thread client for the bundled AVIF encoder (`openmesh-image-wasm`).
 *
 * The ~1.3 MB raw / ~406 KiB gzip-9 WASM is never requested at app startup: the Worker is
 * created and the module downloaded only on the first image encode. If anything fails
 * (module missing, Worker unsupported, encode error) callers fall back to the existing
 * canvas WebP/JPEG path and keep the browser-produced MIME — bytes are never relabelled AVIF.
 */
import {
  AVIF_MIME,
  isAvifFileHeader,
  type AvifEncodeRequest,
  type AvifWorkerResponse,
} from './avif-protocol';

export { AVIF_MIME };

export type AvifEncoderState = 'idle' | 'loading' | 'available' | 'unavailable';

export type AvifEncoderErrorReason =
  | 'unsupported'
  | 'fetch'
  | 'instantiate'
  | 'invalid'
  | 'memory'
  | 'encode'
  | 'timeout';

export class AvifEncoderError extends Error {
  readonly reason: AvifEncoderErrorReason;
  constructor(reason: AvifEncoderErrorReason, message: string) {
    super(message);
    this.name = 'AvifEncoderError';
    this.reason = reason;
  }
}

export interface AvifEncodeResult {
  bytes: Uint8Array;
  /** Worker-side encode time in ms (download/instantiation excluded). */
  ms: number;
  /** Calibrated ravif quality used for the AVIF candidate. */
  quality: number;
  /** Preset WebP quality the AVIF quality was calibrated from. */
  webpQuality: number;
  speed: number;
}

const ENCODE_TIMEOUT_MS = 25_000;

let workerPromise: Promise<Worker> | null = null;
let state: AvifEncoderState = 'idle';
let unavailableReason: string | null = null;
let nextRequestId = 1;
const pending = new Map<
  number,
  { resolve: (result: AvifEncodeResult) => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> }
>();

export function avifEncoderState(): AvifEncoderState {
  return state;
}

export function avifEncoderUnavailableReason(): string | null {
  return unavailableReason;
}

/** Absolute URL of the generated WASM module (respects Vite/Tauri base URLs). */
export function avifWasmUrl(): string {
  const base = import.meta.env.BASE_URL || '/';
  const relative = `${base}image/openmesh_image.wasm`;
  if (typeof document !== 'undefined' && document.baseURI) {
    try {
      return new URL(relative, document.baseURI).toString();
    } catch {
      return relative;
    }
  }
  return relative;
}

function failAll(error: Error): void {
  for (const entry of pending.values()) {
    clearTimeout(entry.timer);
    entry.reject(error);
  }
  pending.clear();
}

function markUnavailable(reason: string): void {
  state = 'unavailable';
  unavailableReason = reason;
  const worker = workerPromise;
  workerPromise = null;
  void worker?.then((instance) => instance.terminate()).catch(() => {});
  failAll(new AvifEncoderError('unsupported', reason));
}

function handleMessage(response: AvifWorkerResponse): void {
  const entry = pending.get(response.id);
  if (!entry) return;
  pending.delete(response.id);
  clearTimeout(entry.timer);
  if (response.type === 'encoded') {
    state = 'available';
    const bytes = new Uint8Array(response.bytes);
    if (!isAvifFileHeader(bytes)) {
      entry.reject(new AvifEncoderError('invalid', 'The AVIF encoder returned a non-AVIF file.'));
      return;
    }
    entry.resolve({
      bytes,
      ms: response.ms,
      quality: response.quality,
      webpQuality: response.webpQuality,
      speed: response.speed,
    });
    return;
  }
  if (response.reason === 'fetch' || response.reason === 'instantiate' || response.reason === 'invalid') {
    markUnavailable(response.message);
  }
  entry.reject(new AvifEncoderError(response.reason, response.message));
}

function createWorker(): Promise<Worker> {
  if (typeof Worker === 'undefined') {
    markUnavailable('Web Workers are not available in this browser.');
    return Promise.reject(new AvifEncoderError('unsupported', unavailableReason ?? 'Workers unavailable'));
  }
  state = 'loading';
  const worker = new Worker(new URL('./avif-worker.ts', import.meta.url), {
    type: 'module',
    name: 'openmesh-avif',
  });
  console.info('[openmesh-avif] worker created');
  worker.onmessage = (event: MessageEvent<AvifWorkerResponse>) => handleMessage(event.data);
  worker.onerror = (event: ErrorEvent) => {
    const message = event.message || 'The AVIF encoder Worker crashed.';
    markUnavailable(message);
    failAll(new AvifEncoderError('instantiate', message));
  };
  worker.onmessageerror = () => {
    failAll(new AvifEncoderError('encode', 'The AVIF encoder Worker sent an unreadable message.'));
  };
  return Promise.resolve(worker);
}

function getWorker(): Promise<Worker> {
  if (state === 'unavailable') {
    return Promise.reject(
      new AvifEncoderError('unsupported', unavailableReason ?? 'The AVIF encoder is unavailable.'),
    );
  }
  if (!workerPromise) workerPromise = createWorker();
  return workerPromise;
}

/**
 * Encode RGBA8 pixels to a complete AVIF file in the Worker. The RGBA buffer is copied and
 * transferred; the caller keeps ownership of its own array. Rejects with `AvifEncoderError`
 * when the bundled encoder is unavailable or fails — callers must then fall back to canvas.
 */
export async function encodeAvifRgba(
  rgba: Uint8ClampedArray,
  width: number,
  height: number,
  qualityFraction: number,
): Promise<AvifEncodeResult> {
  if (state === 'unavailable') {
    throw new AvifEncoderError('unsupported', unavailableReason ?? 'The AVIF encoder is unavailable.');
  }
  const worker = await getWorker();
  const requestId = nextRequestId++;
  const buffer = rgba.slice().buffer;
  const request: AvifEncodeRequest = {
    type: 'encode',
    id: requestId,
    wasmUrl: avifWasmUrl(),
    rgba: buffer,
    width,
    height,
    qualityFraction,
  };

  return new Promise<AvifEncodeResult>((resolve, reject) => {
    const withLogging = (result: AvifEncodeResult) => {
      console.info(
        `[openmesh-avif] encoded ${JSON.stringify({
          width,
          height,
          bytes: result.bytes.byteLength,
          ms: Math.round(result.ms),
          quality: result.quality,
          webpQuality: result.webpQuality,
          speed: result.speed,
        })}`,
      );
      resolve(result);
    };
    const timer = setTimeout(() => {
      pending.delete(requestId);
      workerPromise = null;
      worker.terminate();
      reject(new AvifEncoderError('timeout', 'The AVIF encoder took too long — using the browser encoder instead.'));
    }, ENCODE_TIMEOUT_MS);
    pending.set(requestId, { resolve: withLogging, reject, timer });
    try {
      worker.postMessage(request, [buffer]);
    } catch (cause) {
      clearTimeout(timer);
      pending.delete(requestId);
      reject(
        new AvifEncoderError('encode', `Could not hand the image to the AVIF Worker (${cause instanceof Error ? cause.message : String(cause)}).`),
      );
    }
  });
}
