/**
 * AVIF encoder Worker (lazy, main thread keeps rendering).
 *
 * Loads `public/image/openmesh_image.wasm` on the first encode request, then encodes RGBA
 * images through the C ABI from `crates/openmesh-image-wasm`:
 *   om_image_alloc / om_image_encode / om_image_free
 * plus the quality/speed helpers. The four no-op rav1e externref imports match the Rust
 * wasm build (see `crates/openmesh-image-wasm/test-wasm.mjs`).
 */
import {
  avifEncodeSettings,
  calibratedAvifQuality,
  isAvifFileHeader,
  type AvifEncodeRequest,
  type AvifEncodedResponse,
  type AvifWorkerErrorReason,
  type AvifWorkerResponse,
} from './avif-protocol';

interface ImageWasmExports {
  memory: WebAssembly.Memory;
  om_image_alloc: (len: number) => number;
  om_image_free: (ptr: number, len: number) => void;
  om_image_encode: (
    rgbaPtr: number,
    width: number,
    height: number,
    quality: number,
    speed: number,
    outLenPtr: number,
  ) => number;
  om_image_quality_from_fraction: (fraction: number) => number;
  om_image_calibrated_quality?: (webpQuality: number) => number;
  om_image_speed_for_quality: (quality: number) => number;
}

interface WorkerScope {
  onmessage: ((event: MessageEvent<AvifEncodeRequest>) => void) | null;
  postMessage: (message: AvifWorkerResponse, transfer?: Transferable[]) => void;
}

const scope = self as unknown as WorkerScope;

class WorkerEncoderError extends Error {
  readonly reason: AvifWorkerErrorReason;
  constructor(reason: AvifWorkerErrorReason, message: string) {
    super(message);
    this.name = 'WorkerEncoderError';
    this.reason = reason;
  }
}

const WASM_IMPORTS = {
  __wbindgen_externref_xform__: {
    __wbindgen_externref_statics_end: () => 0,
    __wbindgen_externref_table_set_null: () => {},
    __wbindgen_externref_table_grow: () => 0,
  },
  __wbindgen_placeholder__: {
    __wbindgen_describe: () => {},
  },
};

let wasmPromise: Promise<ImageWasmExports> | null = null;

async function instantiate(url: string): Promise<ImageWasmExports> {
  let response: Response;
  try {
    response = await fetch(url, { credentials: 'same-origin' });
  } catch (cause) {
    throw new WorkerEncoderError(
      'fetch',
      `Could not download the AVIF encoder (${cause instanceof Error ? cause.message : String(cause)})`,
    );
  }
  if (!response.ok) {
    throw new WorkerEncoderError('fetch', `AVIF encoder download failed (HTTP ${response.status})`);
  }
  let bytes: ArrayBuffer;
  try {
    bytes = await response.arrayBuffer();
  } catch (cause) {
    throw new WorkerEncoderError(
      'fetch',
      `Could not read the AVIF encoder download (${cause instanceof Error ? cause.message : String(cause)})`,
    );
  }
  let instance: WebAssembly.Instance;
  try {
    const result = await WebAssembly.instantiate(bytes, WASM_IMPORTS);
    instance = result.instance;
  } catch (cause) {
    throw new WorkerEncoderError(
      'instantiate',
      `Could not instantiate the AVIF encoder (${cause instanceof Error ? cause.message : String(cause)})`,
    );
  }
  const exports = instance.exports as unknown as Partial<ImageWasmExports>;
  if (
    !(exports.memory instanceof WebAssembly.Memory) ||
    typeof exports.om_image_alloc !== 'function' ||
    typeof exports.om_image_free !== 'function' ||
    typeof exports.om_image_encode !== 'function'
  ) {
    throw new WorkerEncoderError('invalid', 'The AVIF encoder WASM is missing the expected C ABI.');
  }
  return exports as ImageWasmExports;
}

function loadWasm(url: string): Promise<ImageWasmExports> {
  if (!wasmPromise) {
    wasmPromise = instantiate(url).catch((error) => {
      wasmPromise = null; // allow a retry on the next image if the network hiccups
      throw error;
    });
  }
  return wasmPromise;
}

function encode(wasm: ImageWasmExports, request: AvifEncodeRequest): AvifEncodedResponse {
  const { width, height } = request;
  if (!Number.isInteger(width) || !Number.isInteger(height) || width <= 0 || height <= 0) {
    throw new WorkerEncoderError('invalid', 'Invalid AVIF image dimensions.');
  }
  const expected = width * height * 4;
  if (request.rgba.byteLength !== expected) {
    throw new WorkerEncoderError(
      'invalid',
      `AVIF input must be ${expected} RGBA bytes, got ${request.rgba.byteLength}.`,
    );
  }

  const fallback = avifEncodeSettings(request.qualityFraction);
  const webpQuality =
    typeof wasm.om_image_quality_from_fraction === 'function'
      ? wasm.om_image_quality_from_fraction(request.qualityFraction)
      : fallback.webpQuality;
  const quality =
    typeof wasm.om_image_calibrated_quality === 'function'
      ? wasm.om_image_calibrated_quality(webpQuality)
      : calibratedAvifQuality(webpQuality);
  // Cap work from the preset WebP quality, not the calibrated AVIF quality: tiny stays speed 10.
  const speed =
    typeof wasm.om_image_speed_for_quality === 'function'
      ? wasm.om_image_speed_for_quality(webpQuality)
      : fallback.speed;

  const inputPtr = wasm.om_image_alloc(expected);
  if (!inputPtr) throw new WorkerEncoderError('memory', 'The AVIF encoder is out of memory.');
  let outLenPtr = 0;
  let outPtr = 0;
  try {
    new Uint8Array(wasm.memory.buffer, inputPtr, expected).set(new Uint8Array(request.rgba));
    outLenPtr = wasm.om_image_alloc(4);
    if (!outLenPtr) throw new WorkerEncoderError('memory', 'The AVIF encoder is out of memory.');

    const started = performance.now();
    outPtr = wasm.om_image_encode(inputPtr, width, height, quality, speed, outLenPtr);
    const ms = performance.now() - started;
    const outLen = new DataView(wasm.memory.buffer).getUint32(outLenPtr, true);
    if (!outPtr || outLen === 0) {
      throw new WorkerEncoderError('encode', 'The AVIF encoder returned an error.');
    }
    const bytes = new Uint8Array(wasm.memory.buffer, outPtr, outLen).slice();
    if (!isAvifFileHeader(bytes)) {
      throw new WorkerEncoderError('encode', 'The encoder output is not an AVIF file.');
    }
    return { type: 'encoded', id: request.id, bytes: bytes.buffer, ms, quality, webpQuality, speed };
  } finally {
    if (outPtr) wasm.om_image_free(outPtr, new DataView(wasm.memory.buffer).getUint32(outLenPtr, true));
    if (outLenPtr) wasm.om_image_free(outLenPtr, 4);
    wasm.om_image_free(inputPtr, expected);
  }
}

async function handle(request: AvifEncodeRequest): Promise<AvifEncodedResponse> {
  try {
    const wasm = await loadWasm(request.wasmUrl);
    return encode(wasm, request);
  } catch (cause) {
    if (cause instanceof WorkerEncoderError) throw cause;
    throw new WorkerEncoderError(
      'encode',
      `AVIF encode failed (${cause instanceof Error ? cause.message : String(cause)})`,
    );
  }
}

scope.onmessage = (event: MessageEvent<AvifEncodeRequest>) => {
  const request = event.data;
  if (!request || request.type !== 'encode') return;
  handle(request)
    .then((response) => {
      scope.postMessage(response, [response.bytes]);
    })
    .catch((cause: unknown) => {
      const reason = cause instanceof WorkerEncoderError ? cause.reason : 'encode';
      const message = cause instanceof Error ? cause.message : String(cause);
      scope.postMessage({ type: 'error', id: request.id, reason, message });
    });
};

export {};
