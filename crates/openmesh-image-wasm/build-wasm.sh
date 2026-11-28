#!/usr/bin/env bash
# Build the OpenMesh AVIF WASM module used by the web client.
#
#   crates/openmesh-image-wasm/build-wasm.sh
#
# Produces apps/web/public/image/openmesh_image.wasm (gitignored build artifact; add the path to
# .gitignore alongside the voice module when wiring CI) from crates/openmesh-image-wasm.
# Override OUT_DIR to build elsewhere. Mirrors scripts/build-voice-wasm.sh.
set -euo pipefail
cd "$(dirname "$0")/../.."

TARGET=wasm32-unknown-unknown
OUT_DIR=${OUT_DIR:-apps/web/public/image}
OUT_FILE="$OUT_DIR/openmesh_image.wasm"

echo "==> host smoke test (AVIF encode via the C ABI path)"
cargo test -q -p openmesh-image-wasm

echo "==> ensuring $TARGET is installed"
if ! rustup target list --installed 2>/dev/null | grep -qx "$TARGET"; then
    rustup target add "$TARGET"
fi

echo "==> building openmesh-image-wasm ($TARGET, release)"
cargo build -q -p openmesh-image-wasm --release --target "$TARGET"

WASM=target/$TARGET/release/openmesh_image_wasm.wasm
if [ ! -f "$WASM" ]; then
    echo "error: expected wasm artifact not found at $WASM" >&2
    exit 1
fi

mkdir -p "$OUT_DIR"
cp "$WASM" "$OUT_FILE"

magic=$(od -An -tx1 -N4 "$OUT_FILE" | tr -d ' \n')
if [ "$magic" != "0061736d" ]; then
    echo "error: $OUT_FILE is not a WebAssembly module (magic=$magic)" >&2
    exit 1
fi
raw=$(stat -c%s "$OUT_FILE")
gzip=$(gzip -9 -c "$OUT_FILE" | wc -c)
echo "==> wrote $OUT_FILE (raw $raw bytes, gzip-9 $gzip bytes)"
