# syntax=docker/dockerfile:1
#
# OpenMesh backbone container (multi-stage: web panel -> Rust binaries -> minimal runtime).
#
# Build from the repository root (the Dockerfile needs the workspace and apps/web):
#
#   docker build -f packaging/Dockerfile -t openmesh-backbone:0.2.0 .
#
# Or use packaging/docker-compose.yml. The image runs as an unprivileged user and keeps all
# mutable state under /var/lib/openmesh (mount a volume there). The panel/API listens on
# 0.0.0.0:8787 by default so a published port works; override the command to bind
# 127.0.0.1:8787 when using host networking (the compose example does this).
#
# Not built on the OpenMesh development host (no Docker); syntax and stages are reviewed and
# documented rather than verified locally. See docs/install for the supported installer path.

# ---------------------------------------------------------------------------------------------
# Stage 1: build the web control panel (Vite/React -> apps/web/dist)
# ---------------------------------------------------------------------------------------------
FROM node:22-bookworm-slim AS web
RUN corepack enable
WORKDIR /src/apps/web
COPY apps/web/package.json apps/web/pnpm-lock.yaml apps/web/pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY apps/web/ ./
RUN pnpm build

# ---------------------------------------------------------------------------------------------
# Stage 2: build the Rust binaries
# ---------------------------------------------------------------------------------------------
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ ./crates/
# The default feature set needs no C libraries (serial support is opt-in via real-serial).
RUN cargo build --release --locked -p openmesh-bridge -p openmesh-cli

# ---------------------------------------------------------------------------------------------
# Stage 3: runtime
# ---------------------------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 openmesh \
    && useradd --system --uid 10001 --gid openmesh --home-dir /var/lib/openmesh \
        --shell /usr/sbin/nologin openmesh

COPY --from=build /src/target/release/openmesh-bridge /usr/local/bin/openmesh-bridge
COPY --from=build /src/target/release/openmesh-cli /usr/local/bin/openmesh-cli
COPY --from=web /src/apps/web/dist /usr/share/openmesh/web
COPY LICENSE NOTICE README.md /usr/share/doc/openmesh/

RUN mkdir -p /var/lib/openmesh \
    && chown -R openmesh:openmesh /var/lib/openmesh /usr/share/doc/openmesh \
    && chmod 0700 /var/lib/openmesh

# The bridge stores its identity, database, backbone.json and (by default) the API token under
# HOME. Keeping HOME on the state volume makes the panel token survive container rebuilds.
ENV HOME=/var/lib/openmesh \
    RUST_LOG=info

USER openmesh
WORKDIR /var/lib/openmesh
VOLUME ["/var/lib/openmesh"]
EXPOSE 9443 8787
STOPSIGNAL SIGTERM

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD ["curl", "-fsS", "http://127.0.0.1:8787/api/health"]

ENTRYPOINT ["openmesh-bridge"]
CMD ["backbone", "--data-dir", "/var/lib/openmesh", \
     "--serve-web", "/usr/share/openmesh/web", "--listen", "0.0.0.0:8787"]
