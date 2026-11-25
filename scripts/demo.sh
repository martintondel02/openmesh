#!/usr/bin/env bash
# One-command demo: simulated mesh + bridge daemon + built web UI.
# Usage: ./scripts/demo.sh [--headless]
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> building web client (if needed)"
if [ ! -d apps/web/dist ]; then
  pnpm --dir apps/web install
  pnpm --dir apps/web build
fi

echo "==> starting bridge in demo mode on http://127.0.0.1:8787"
echo "    (simulated radio, seeded network, demo traffic)"
exec cargo run -p openmesh-bridge --release -- --demo --serve-web apps/web/dist
