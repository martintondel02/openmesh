# packaging/ — deployment assets for OpenMesh backbone mode

These files are templates and examples for operators. The supported one-line path is
`scripts/install.sh` ([docs/INSTALL.md](../docs/INSTALL.md)); this directory is for manual
installs, distribution packagers, container deployments and people who want to read the unit
before installing it.

| Path | Purpose |
| --- | --- |
| `systemd/openmesh-backbone.service` | System-wide hardened unit (dedicated `openmesh` user, `StateDirectory`/`ConfigurationDirectory`) |
| `systemd/openmesh-backbone.user.service` | systemd **user** unit template (no root; uses `%h`) |
| `sysusers/openmesh.conf` | `systemd-sysusers` fragment to create the `openmesh` system user/group |
| `backbone.json.example` | Example `<data-dir>/backbone.json`; the daemon generates a real token on first run |
| `firewall.md` | firewalld/ufw/nftables rules and Docker caveats |
| `Dockerfile` | Multi-stage image: Rust binaries + web panel, non-root runtime |
| `docker-compose.yml` | Compose example (host networking, named volumes, loopback panel) |
| `Dockerfile.dockerignore` | Build-context exclusions for the Docker image |
| `examples/Caddyfile.panel` | TLS reverse proxy for the panel/API (HTTPS + basic auth) |
| `examples/nginx-panel.conf` | Same, for nginx |
| `examples/nginx-stream-tls.conf` | TLS termination for the OBB/1 port (paired with `stunnel`) |
| `examples/stunnel-client.conf` | Client-side TLS wrapper for `openmesh-bridge join` on hostile networks |
| `examples/wireguard-backbone.conf` | WireGuard snippet (recommended alternative to TLS wrapping) |

Conventions used by every file:

* the backbone TCP port is **9443/tcp** (OBB/1, authenticated, plaintext unless wrapped);
* the bridge API/panel stays on **127.0.0.1:8787** and must not be exposed directly;
* data lives in `/var/lib/openmesh` (system) or `~/.openmesh` (user);
* config/token lives in `/etc/openmesh` (system) or `~/.config/openmesh` (user).

Read [docs/architecture/BACKBONE.md](../docs/architecture/BACKBONE.md) for the protocol and
[docs/operations/backbone-deployment.md](../docs/operations/backbone-deployment.md) for sizing,
monitoring, TLS/WireGuard and incident procedures.
