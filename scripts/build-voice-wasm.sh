#!/usr/bin/env bash
# Build the OpenMesh voice WASM module used by the web client.
#
#   scripts/build-voice-wasm.sh
#
# Produces apps/web/public/voice/openmesh_voice.wasm (gitignored build artifact) from
# crates/openmesh-voice-wasm. CI and release workflows call this before the web build.
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET=wasm32-unknown-unknown
OUT_DIR=apps/web/public/voice
OUT_FILE="$OUT_DIR/openmesh_voice.wasm"

echo "==> host smoke test (Codec2 encode/decode via the C ABI)"
cargo test -q -p openmesh-voice-wasm

echo "==> ensuring $TARGET is installed"
if ! rustup target list --installed 2>/dev/null | grep -qx "$TARGET"; then
    rustup target add "$TARGET"
fi

echo "==> building openmesh-voice-wasm ($TARGET, release)"
cargo build -q -p openmesh-voice-wasm --release --target "$TARGET"

WASM=target/$TARGET/release/openmesh_voice_wasm.wasm
if [ ! -f "$WASM" ]; then
    echo "error: expected wasm artifact not found at $WASM" >&2
    exit 1
fi

mkdir -p "$OUT_DIR"
cp "$WASM" "$OUT_FILE"

# Verify it is a real WebAssembly module (magic \0asm) with a plausible size.
magic=$(od -An -tx1 -N4 "$OUT_FILE" | tr -d ' \n')
if [ "$magic" != "0061736d" ]; then
    echo "error: $OUT_FILE is not a WebAssembly module (magic=$magic)" >&2
    exit 1
fi
size=$(stat -c%s "$OUT_FILE")
if [ "$size" -lt 10000 ]; then
    echo "error: $OUT_FILE is suspiciously small ($size bytes)" >&2
    exit 1
fi
echo "==> wrote $OUT_FILE ($size bytes)"
