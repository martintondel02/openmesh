#!/usr/bin/env sh
# Build the OpenMesh Rust engine (UniFFI) for Android and generate the Kotlin bindings.
#
#   apps/android/scripts/build-ffi.sh
#
# Outputs (gitignored):
#   app/src/main/jniLibs/<abi>/libopenmesh_ffi.so   Rust engine for each Android ABI
#   app/src/main/java/uniffi/openmesh_ffi/*.kt      generated Kotlin bindings
#
# Requirements: Rust with the Android targets, cargo-ndk, Android NDK (ANDROID_NDK_HOME),
# and the workspace at the repository root. Java/Android SDK are not needed here.
#
# Environment:
#   ANDROID_NDK_HOME       NDK path (required; CI installs NDK 27.0.12077973)
#   OPENMESH_ANDROID_ABIS  space-separated ABIs (default: arm64-v8a armeabi-v7a x86_64)
#   OPENMESH_FFI_PROFILE   cargo profile (default: release)
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ANDROID_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
REPO_ROOT=$(CDPATH= cd -- "$ANDROID_DIR/../.." && pwd)
JNI_LIBS="$ANDROID_DIR/app/src/main/jniLibs"
KOTLIN_OUT="$ANDROID_DIR/app/src/main/java"
HOST_TARGET_DIR="$REPO_ROOT/target"

ABIS=${OPENMESH_ANDROID_ABIS:-"arm64-v8a armeabi-v7a x86_64"}
PROFILE=${OPENMESH_FFI_PROFILE:-release}

say() { printf '==> %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

command -v cargo >/dev/null 2>&1 || die "cargo not found (install Rust: https://rustup.rs)"
command -v cargo-ndk >/dev/null 2>&1 || die "cargo-ndk not found (cargo install cargo-ndk --locked)"
[ -n "${ANDROID_NDK_HOME:-}" ] || die "ANDROID_NDK_HOME is not set (e.g. \$HOME/android-sdk/ndk/27.0.12077973)"
[ -d "$ANDROID_NDK_HOME" ] || die "ANDROID_NDK_HOME does not exist: $ANDROID_NDK_HOME"

for abi in $ABIS; do
    case "$abi" in
        arm64-v8a|armeabi-v7a|x86_64) : ;;
        *) die "unsupported ABI '$abi' (use arm64-v8a, armeabi-v7a or x86_64)" ;;
    esac
done

say "repository: $REPO_ROOT"
say "NDK:        $ANDROID_NDK_HOME"
say "ABIs:       $ABIS"

# 1. Cross-compile the cdylib for Android. cargo-ndk writes app/src/main/jniLibs/<abi>/.
say "cargo ndk build -p openmesh-ffi ($PROFILE)"
cd "$REPO_ROOT"
# cargo-ndk wants one repeated -t per ABI.
set -- ""
for abi in $ABIS; do set -- "$@" -t "$abi"; done
shift
if [ "$PROFILE" = "release" ]; then
    cargo ndk "$@" -o "$JNI_LIBS" build -p openmesh-ffi --release
else
    cargo ndk "$@" -o "$JNI_LIBS" build -p openmesh-ffi
fi

for abi in $ABIS; do
    so="$JNI_LIBS/$abi/libopenmesh_ffi.so"
    [ -f "$so" ] || die "missing $so after cargo-ndk (check the ABIs and NDK)"
    say "$(du -h "$so" | cut -f1)  ${so#"$ANDROID_DIR/"}"
done

# 2. Build the host cdylib so uniffi-bindgen can read the FFI metadata. The bindgen only parses
#    metadata; the Android .so files above are what actually ships.
say "cargo build -p openmesh-ffi (host bindgen metadata; debug profile keeps UNIFFI_META symbols)"
cargo build -p openmesh-ffi

HOST_LIB="$HOST_TARGET_DIR/debug/libopenmesh_ffi.so"
case "$(uname -s)" in
    Darwin) HOST_LIB="$HOST_TARGET_DIR/debug/libopenmesh_ffi.dylib" ;;
    MINGW*|MSYS*|CYGWIN*) HOST_LIB="$HOST_TARGET_DIR/debug/openmesh_ffi.dll" ;;
esac
[ -f "$HOST_LIB" ] || die "host FFI library not found at $HOST_LIB"

# 3. Generate Kotlin into app/src/main/java (package uniffi.openmesh_ffi). Remove previous
#    generated output so renamed types never linger as stale duplicates.
rm -rf "$KOTLIN_OUT/uniffi/openmesh_ffi"
say "uniffi-bindgen generate --language kotlin"
cargo run -q -p openmesh-ffi --bin uniffi-bindgen -- \
    generate --library "$HOST_LIB" --language kotlin --out-dir "$KOTLIN_OUT"

GEN_DIR="$KOTLIN_OUT/uniffi/openmesh_ffi"
[ -d "$GEN_DIR" ] || die "bindgen did not create $GEN_DIR"
find "$KOTLIN_OUT/uniffi" -name '*.kt' | while IFS= read -r kt; do say "generated ${kt#"$ANDROID_DIR/"}"; done

say "done: $(find "$JNI_LIBS" -name 'libopenmesh_ffi.so' | wc -l | tr -d ' ') ABIs, Kotlin in app/src/main/java/uniffi/openmesh_ffi"
say "next: cd apps/android && ./gradlew :app:assembleDebug :app:testDebugUnitTest"
