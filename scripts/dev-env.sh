#!/usr/bin/env bash
# OpenMesh developer environment helper.
#
# This machine has no system C compiler; zig is used as `cc` so that Rust build scripts and
# linkers work. On machines with gcc/clang nothing here is needed.
set -euo pipefail

if command -v cc >/dev/null 2>&1; then
  echo "cc found: $(command -v cc) — no shim needed"
else
  echo "no system cc found; configuring zig shim in ~/.local/bin and ~/.cargo/config.toml"
  mkdir -p "$HOME/.local/bin"
  cat > "$HOME/.local/bin/zig-cc" <<'SH'
#!/bin/sh
exec zig cc "$@"
SH
  chmod +x "$HOME/.local/bin/zig-cc"
  ln -sf "$HOME/.local/bin/zig-cc" "$HOME/.local/bin/cc"
  mkdir -p "$HOME/.cargo"
  cat > "$HOME/.cargo/config.toml" <<TOML
[target.x86_64-unknown-linux-gnu]
linker = "$HOME/.local/bin/zig-cc"
TOML
fi

echo
echo "toolchain:"
rustc --version || true
cargo --version || true
node --version || true
pnpm --version || true
zig version 2>/dev/null || true
