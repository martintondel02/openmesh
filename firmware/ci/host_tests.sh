#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Host unit tests for the OpenMesh firmware protocol library.
#
# This machine (and CI runners without a system compiler) may only have zig available; the
# script uses zig cc/c++ when present, otherwise gcc/g++. It compiles the portable core library
# (no Arduino headers) plus the vendored Monocypher, then runs the test suite. Golden vectors in
# protocol/vectors are replayed when present and reported as SKIP when not yet generated.
#
# Usage: firmware/ci/host_tests.sh [--verbose]
# Exit:  0 on success (skipped vector files are not failures), 1 on test failure.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LIB="$ROOT/firmware/lib/openmesh"
TEST="$ROOT/firmware/test/test_protocol"
OUT="${TMPDIR:-/tmp}/openmesh-host-tests.$$"
mkdir -p "$OUT"

CXX="${CXX:-}"
CC="${CC:-}"
if [[ -z "$CXX" ]]; then
  if command -v zig >/dev/null 2>&1; then
    CXX="zig c++"
    CC="zig cc"
  elif command -v g++ >/dev/null 2>&1; then
    CXX="g++"
    CC="gcc"
  elif command -v clang++ >/dev/null 2>&1; then
    CXX="clang++"
    CC="clang"
  else
    echo "error: no C++ compiler found (tried zig, g++, clang++)" >&2
    exit 2
  fi
fi
echo "==> compiler: $CXX"

CXXFLAGS=(-std=c++17 -O2 -g -Wall -Wextra -Wno-unused-parameter -DOM_HOST_TESTS
          -I"$LIB/include" -I"$TEST")
CFLAGS=(-std=c99 -O2 -g -I"$LIB/include")

sources=()
for f in "$LIB"/src/*.cpp "$LIB"/src/*.c; do sources+=("$f"); done
sources+=("$LIB/third_party/monocypher/monocypher.c" "$LIB/third_party/monocypher/monocypher-ed25519.c")
sources+=("$TEST/test_main.cpp" "$TEST/test_protocol.cpp" "$TEST/test_vectors.cpp"
          "$TEST/test_shim.cpp" "$TEST/json_lite.cpp")

objects=()
for f in "${sources[@]}"; do
  obj="$OUT/$(echo "$f" | tr '/' '_').o"
  case "$f" in
    *.c) $CC "${CFLAGS[@]}" -c "$f" -o "$obj" ;;
    *)   $CXX "${CXXFLAGS[@]}" -c "$f" -o "$obj" ;;
  esac
  objects+=("$obj")
done

$CXX -o "$OUT/test_protocol" "${objects[@]}"
echo "==> running firmware protocol tests"
cd "$ROOT"
"$OUT/test_protocol"
status=$?
echo "==> artifacts in $OUT (remove manually if desired)"
exit $status
