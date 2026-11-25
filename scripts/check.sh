#!/usr/bin/env bash
# Full local verification used by CI and before releases.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> cargo fmt"
cargo fmt --all -- --check

echo "==> clippy"
cargo clippy --workspace --all-targets -- -D warnings

echo "==> tests"
cargo test --workspace --all-targets

echo "==> golden vectors drift"
cargo run -q -p openmesh-core --example gen_vectors
git diff --exit-code -- protocol/vectors || {
  echo "protocol/vectors is stale; commit the regenerated files" >&2
  exit 1
}

echo "==> simulator"
cargo run -q -p openmesh-sim -- --profile ci

if [ -d apps/web/node_modules ]; then
  echo "==> web"
  pnpm --dir apps/web build
  pnpm --dir apps/web test -- --run
fi

echo "==> docs"
python3 scripts/check_docs.py

echo "OK"
