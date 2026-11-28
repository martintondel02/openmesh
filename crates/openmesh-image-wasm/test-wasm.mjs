// Node smoke test for the AVIF wasm module: instantiates the C ABI with the four rav1e
// wasm-bindgen externref stubs, encodes an RGBA file and reports time/size. Used by the task-35
// spike report; the web client's Worker loader follows the same import shape.
//
//   node crates/openmesh-image-wasm/test-wasm.mjs <openmesh_image.wasm> <input.rgba> <width> <height>
import fs from 'node:fs';

const [, , wasmPath, rgbaPath, widthArg = '512', heightArg = '512', qualityArg, speedArg] = process.argv;
const bytes = fs.readFileSync(wasmPath);
const width = Number(widthArg);
const height = Number(heightArg);
const imports = {
  __wbindgen_externref_xform__: {
    __wbindgen_externref_statics_end: () => 0,
    __wbindgen_externref_table_set_null: () => {},
    __wbindgen_externref_table_grow: () => 0,
  },
  __wbindgen_placeholder__: { __wbindgen_describe: () => {} },
};
const { instance } = await WebAssembly.instantiate(bytes, imports);
const e = instance.exports;
const rgba = fs.readFileSync(rgbaPath);
if (rgba.length !== width * height * 4) throw new Error('RGBA size mismatch');
const input = Number(e.om_image_alloc(rgba.length));
new Uint8Array(e.memory.buffer, input, rgba.length).set(rgba);
const outLen = Number(e.om_image_alloc(4));
const encode = (quality, speed) => {
  const t0 = performance.now();
  const outPtr = Number(e.om_image_encode(input, width, height, quality, speed, outLen));
  const ms = performance.now() - t0;
  const len = new DataView(e.memory.buffer).getUint32(outLen, true);
  if (!outPtr) throw new Error('encode failed');
  const out = new Uint8Array(e.memory.buffer, outPtr, len).slice();
  e.om_image_free(outPtr, len);
  return { ms, bytes: out };
};
const quality = qualityArg ? Number(qualityArg) : e.om_image_quality_from_fraction(0.4);
const speed = speedArg ? Number(speedArg) : e.om_image_speed_for_quality(quality);
encode(quality, speed); // warm-up: pages in the module's lazy state
const first = encode(quality, speed);
const second = encode(quality, speed);
const tier = first.bytes.length === second.bytes.length ? second : first;
console.log(`wasm ${width}x${height} quality=${quality} speed=${speed} bytes=${tier.bytes.length} time_ms=${Math.min(first.ms, second.ms).toFixed(1)} magic=${Buffer.from(tier.bytes.slice(4, 12)).toString()} deterministic=${Buffer.from(first.bytes).equals(Buffer.from(second.bytes))}`);
fs.writeFileSync('/tmp/openmesh-image-wasm-test.avif', tier.bytes);
