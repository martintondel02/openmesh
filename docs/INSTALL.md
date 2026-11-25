# Installing and running OpenMesh

**Status:** v0.2.0 · applies to Linux, macOS, Windows and Docker · **Last updated:** 2026-11-25

OpenMesh is two programs plus an optional web panel:

| Component | What it does |
| --- | --- |
| `openmesh-bridge` | The daemon: radio/network transports, protocol engine, message store, WebSocket/REST API, and **backbone mode** (authenticated TCP relay for joining distant meshes). |
| `openmesh-cli` | Terminal client and diagnostics, including `backbone info/peers/kick/rotate-token`. |
| web bundle | The control panel and chat UI served by the bridge at `/` (and the backbone panel at `/backbone`). |

Everything runs **without an account**. A backbone needs a public TCP port (default
**9443/tcp**); the API/panel stays on loopback (**127.0.0.1:8787**) and is never exposed directly.

> New to the concepts? Read [Architecture](architecture/ARCHITECTURE.md) and
> [Backbone mode](architecture/BACKBONE.md) first. For running a public backbone (sizing,
> monitoring, TLS, WireGuard, incident checklists) see
> [Backbone deployment & operations](operations/backbone-deployment.md).

---

## 1. Supported platforms

| Platform | Script install | Prebuilt release | Source build | Service manager |
| --- | --- | --- | --- | --- |
| Linux x86_64 | ✅ `install.sh` | ✅ `linux-x86_64` | ✅ | systemd (user and system) |
| Linux aarch64 (Raspberry Pi, ARM VPS) | ❌ no release asset yet | ❌ | ✅ build on the device | systemd |
| macOS Apple Silicon (arm64) | ✅ `install.sh` | ✅ `macos-arm64` | ✅ | launchd or terminal (no systemd) |
| macOS Intel (x86_64) | ❌ no release asset | ❌ | ✅ build from source | launchd |
| Windows x86_64 | ◐ Git Bash/MSYS only | ✅ `windows-x86_64` `.tar.gz` | ✅ MSVC | run manually / Task Scheduler |
| Docker (Linux host) | ✅ `packaging/Dockerfile` | — | ✅ multi-stage build | Docker / Compose |

Release assets are built by `.github/workflows/release.yml` for exactly:
`linux-x86_64`, `macos-arm64`, `windows-x86_64`. The backbone is a normal Linux/macOS/Windows
program; a Raspberry Pi is a great backbone host, but there is no prebuilt `linux-aarch64`
artifact yet — see [§5](#5-build-from-source) to build it on the device.

---

## 2. Quick install (recommended)

Linux x86_64 and macOS arm64:

```bash
curl -fsSL https://github.com/martintondel02/openmesh/releases/latest/download/install.sh | sh
```

Then start a backbone:

```bash
openmesh-bridge backbone \
  --data-dir ~/.openmesh \
  --serve-web ~/.local/share/openmesh/web \
  --listen 127.0.0.1:8787
```

On first run the bridge:

* creates `~/.openmesh/` (mode `0700`) and `~/.openmesh/backbone.json` (mode `0600`) with a
  freshly generated 32-byte backbone token,
* creates the local API token at `~/.config/openmesh/bridge-token` (mode `0600`),
* prints an onboarding guide (listen address, panel URL, connection string, firewall commands),
* serves the panel at <http://127.0.0.1:8787/backbone>.

Open the panel, then paste the **bridge API token** (`cat ~/.config/openmesh/bridge-token`) when
the UI asks for it. The panel can read live statistics, edit backbone settings, rotate the
backbone token, kick peers and generate a QR code for joining gateways.

> `curl | sh` runs the released installer. If your policy forbids that, download it first and
> read it: `curl -fsSLO .../install.sh && less install.sh && sh install.sh` (see §3).

### 2.1 What the installer does

1. Detects the platform (`linux-x86_64`, `macos-arm64`, `windows-x86_64` under Git Bash).
2. Resolves the latest GitHub release (or the `--version` you pin).
3. Downloads `SHA256SUMS.txt`, the binary asset and (unless `--no-web`) the web bundle.
4. **Verifies every SHA-256 checksum before installing anything**; a mismatch aborts with no
   changes.
5. Installs `openmesh-bridge` and `openmesh-cli` to `~/.local/bin`, the web bundle to
   `~/.local/share/openmesh/web`, docs to `~/.local/share/doc/openmesh`, and creates the
   `~/.openmesh` + `~/.config/openmesh` directories (`0700`).
6. Optionally writes and enables a **systemd user service** (`--service`).
7. Prints the panel URL, the command to start the backbone, firewall hints, and a `PATH` hint.

Existing data, configuration, tokens and identity seeds are **never overwritten**: the installer
is safe to re-run (for example to upgrade).

### 2.2 Installer options

| Option | Meaning |
| --- | --- |
| `--version v0.2.0` | Pin a release (accepts `v0.2.0` or `0.2.0`). |
| `--system` | Install to `/usr/local/bin` and `/usr/share/openmesh/web` (asks for sudo; also `/var/lib/openmesh`, `/etc/openmesh` for the service). |
| `--service` | Write `~/.config/systemd/user/openmesh-backbone.service` and `systemctl --user enable --now` it. Cannot be combined with `--system`. |
| `--bin-dir DIR` | Install the binaries somewhere else. |
| `--data-dir DIR` | Data directory used in the generated service (default `~/.openmesh`). |
| `--no-web` | Skip the web panel bundle (CLI/API only). |
| `--dry-run` | Print exactly what would happen; download and change nothing. |
| `--uninstall` | Remove binaries, web bundle, docs and the installer-written unit. Keeps config/data. |
| `-y`, `--yes` | Non-interactive (needed for `--system` without a terminal, e.g. in provisioning). |
| `-h`, `--help` | Full help, including testing/mirror environment variables. |

Examples:

```bash
./install.sh --dry-run
./install.sh --version v0.2.0
./install.sh --service
sudo ./install.sh --system --yes
./install.sh --uninstall
```

### 2.3 Installer environment overrides

Useful for mirror/testing (documented in `--help`):

| Variable | Effect |
| --- | --- |
| `OPENMESH_VERSION` | Same as `--version`. |
| `OPENMESH_BASE_URL` | Download base URL or local directory containing the release assets, `SHA256SUMS.txt` and `VERSION` (for mirrors and tests). |
| `OPENMESH_API_URL` | Override the releases API used for "latest" resolution. |
| `OPENMESH_REPO` | GitHub repository (`owner/name`), for a mirror. |
| `OPENMESH_PREFIX` | Install into a filesystem root instead of `/` (packaging/testing; disables sudo). |
| `OPENMESH_BIN_DIR`, `OPENMESH_WEB_DIR`, `OPENMESH_DOC_DIR`, `OPENMESH_DATA_DIR`, `OPENMESH_CONFIG_DIR`, `OPENMESH_SYSTEMD_DIR` | Override individual locations (advanced/testing). |
| `OPENMESH_SKIP_SYSTEMCTL=1` | Write the unit but do not call `systemctl` (containers/CI). |
| `GITHUB_TOKEN` | Optional token for the GitHub releases API rate limit. |

---

## 3. Manual install from a release

Use this when you want to inspect the archive, install the panel elsewhere, or your platform has
no `sh` installer path (native Windows). Assets on the
[releases page](https://github.com/martintondel02/openmesh/releases) are:

| Asset | Contents |
| --- | --- |
| `openmesh-vX.Y.Z-linux-x86_64.tar.gz` | `openmesh-vX.Y.Z-linux-x86_64/` with `openmesh-bridge`, `openmesh-cli`, `LICENSE`, `NOTICE`, `README.md` |
| `openmesh-vX.Y.Z-macos-arm64.tar.gz` | same layout for Apple Silicon |
| `openmesh-vX.Y.Z-windows-x86_64.tar.gz` | same layout with `.exe` binaries (a `.tar.gz`, not a `.zip`) |
| `openmesh-web-vX.Y.Z.tar.gz` | `dist/` — the built web client/panel |
| `SHA256SUMS.txt` | SHA-256 digest of every release asset |
| `install.sh` | the installer from §2 (also hashed) |

### 3.1 Download and verify

Replace `vX.Y.Z` with the release tag and `linux-x86_64` with your platform:

```bash
VERSION=v0.2.0
PLATFORM=linux-x86_64
BASE="https://github.com/martintondel02/openmesh/releases/download/${VERSION}"

curl -fsSLO "$BASE/openmesh-${VERSION}-${PLATFORM}.tar.gz"
curl -fsSLO "$BASE/openmesh-web-${VERSION}.tar.gz"
curl -fsSLO "$BASE/SHA256SUMS.txt"

# Linux
sha256sum -c SHA256SUMS.txt --ignore-missing
# macOS
shasum -a 256 -c SHA256SUMS.txt --ignore-missing
```

`--ignore-missing` lets the same checksum file (which also lists firmware images and
`install.sh`) verify only the files you downloaded. Do not continue if a line reports `FAILED`.

### 3.2 Extract and install (user, no root)

```bash
tar -xzf "openmesh-${VERSION}-${PLATFORM}.tar.gz"
mkdir -p ~/.local/bin ~/.local/share/openmesh/web ~/.openmesh
cp "openmesh-${VERSION}-${PLATFORM}/openmesh-bridge" ~/.local/bin/
cp "openmesh-${VERSION}-${PLATFORM}/openmesh-cli" ~/.local/bin/
chmod 755 ~/.local/bin/openmesh-bridge ~/.local/bin/openmesh-cli

tar -xzf "openmesh-web-${VERSION}.tar.gz"
rm -rf ~/.local/share/openmesh/web
mkdir -p ~/.local/share/openmesh/web
cp -R dist/. ~/.local/share/openmesh/web/
chmod -R u=rwX,go=rX ~/.local/share/openmesh/web

mkdir -p -m 700 ~/.config/openmesh
```

`~/.local/bin` must be on your `PATH` (`export PATH="$HOME/.local/bin:$PATH"` in your shell
profile if it is not).

### 3.3 Extract and install (system-wide)

```bash
sudo tar -xzf "openmesh-${VERSION}-${PLATFORM}.tar.gz" -C /tmp
sudo install -m 755 "/tmp/openmesh-${VERSION}-${PLATFORM}/openmesh-bridge" /usr/local/bin/
sudo install -m 755 "/tmp/openmesh-${VERSION}-${PLATFORM}/openmesh-cli" /usr/local/bin/
sudo mkdir -p /usr/share/openmesh/web /var/lib/openmesh /etc/openmesh
sudo tar -xzf "openmesh-web-${VERSION}.tar.gz" -C /tmp
sudo cp -R /tmp/dist/. /usr/share/openmesh/web/
sudo chmod -R u=rwX,go=rX /usr/share/openmesh/web
```

For a system-wide service, follow [§6.2](#62-system-wide-service-root) (dedicated user,
`/var/lib/openmesh`, `/etc/openmesh`).

### 3.4 macOS notes

* Releases are built for **Apple Silicon (arm64)** only. Intel Macs must build from source (§5).
* The binaries are not notarised. If Gatekeeper blocks them:
  `xattr -d com.apple.quarantine ~/.local/bin/openmesh-bridge ~/.local/bin/openmesh-cli`.
* There is no systemd: run the bridge in a terminal, or use a `launchd` agent with
  `--data-dir "$HOME/.openmesh" --serve-web "$HOME/.local/share/openmesh/web"`.

### 3.5 Windows notes

The release asset is `openmesh-vX.Y.Z-windows-x86_64.tar.gz` (there is no `.zip`). Windows 10+
ships `tar.exe`; 7-Zip also opens `.tar.gz`.

```powershell
# Extract (PowerShell; adjust the version)
tar -xzf openmesh-v0.2.0-windows-x86_64.tar.gz
$env:HOME = $env:USERPROFILE   # makes default token/data paths predictable
New-Item -ItemType Directory -Force "$env:USERPROFILE\.openmesh" | Out-Null
.\openmesh-v0.2.0-windows-x86_64\openmesh-bridge.exe backbone `
  --data-dir "$env:USERPROFILE\.openmesh" `
  --serve-web ".\openmesh-v0.2.0-windows-x86_64\web" `
  --listen 127.0.0.1:8787
```

* The web bundle is not inside the Windows binary archive; extract the separate
  `openmesh-web-vX.Y.Z.tar.gz` and point `--serve-web` at its `dist/` directory (or omit
  `--serve-web` and use the API/CLI only).
* `install.sh` works under Git Bash/MSYS and installs into the Git Bash home, but the panel's
  shell examples are POSIX. Native Windows users should use the archive.
* Allow `9443/tcp` inbound in Windows Defender Firewall if the backbone should accept remote
  peers: *Windows Defender Firewall → Advanced settings → Inbound Rules → New Rule → Port*.
* There is no supported Windows service wrapper in v0.2.0. Run at logon with Task Scheduler, or
  use WSL2/Docker. Do not expose the API port (`8787`) beyond loopback.

---

## 3b. Android app

The Android app is a standalone mesh client: the protocol engine runs on the phone, and it talks
to a radio over BLE or USB. No bridge is needed.

### Install from a release (sideload)

1. On your phone, download the APK from the release page:
   `https://github.com/martintondel02/openmesh/releases/latest` → `openmesh-<version>-android-debug.apk`
   (or `…-android-release.apk` if the release is signed).
2. Allow installs from the browser/file manager when prompted (Android "Install unknown apps" for
   that one app), then open the APK to install.
3. First run: pick *phone/laptop* (or your role), country, node name, privacy → **Secure my node**.
   The app applies the same secure preset as the desktop/web clients and creates a private channel.
4. Connect your radio:
   * **USB:** plug the radio in, allow USB access, choose it under *Radio → USB*.
   * **BLE:** power the radio, *Radio → Bluetooth*, pick the OpenMesh device.
   The radio must be flashed with OpenMesh firmware supporting the companion link (see
   [`docs/hardware/boards.md`](hardware/boards.md) and `firmware/README.md`).
5. Invite someone: **Channels → share invite** (QR or copy), then send a message.

### Requirements

* Android 8.0 (API 26) or newer, Bluetooth LE and/or USB-OTG depending on the link.
* The phone does **not** need internet. Map tiles are optional/offline.

### Building the APK yourself

```bash
# Rust + Android SDK/NDK + cargo-ndk are required (CI installs them automatically)
export ANDROID_HOME=$HOME/android-sdk
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/27.0.12077973
apps/android/scripts/build-ffi.sh          # Rust engine for arm64-v8a/armeabi-v7a/x86_64 + Kotlin
cd apps/android && ./gradlew assembleDebug  # app/build/outputs/apk/debug/app-debug.apk
```

### Signing a release APK

CI signs `assembleRelease` when these repository secrets exist (never commit a keystore):

| Secret | Meaning |
| --- | --- |
| `ANDROID_KEYSTORE_BASE64` | base64 of your `.jks` keystore |
| `ANDROID_KEYSTORE_PASSWORD` | keystore password |
| `ANDROID_KEY_ALIAS` | key alias |
| `ANDROID_KEY_PASSWORD` | key password |

Generate one with:

```bash
keytool -genkeypair -v -keystore openmesh-release.jks -keyalg RSA -keysize 4096 \
  -validity 10000 -alias openmesh
base64 -w0 openmesh-release.jks   # paste as ANDROID_KEYSTORE_BASE64
```

Without the secrets, CI still produces an unsigned release APK and a debug APK. The app is
sideload-only for now (no Play Store/F-Droid listing); the manifest is F-Droid-friendly (no
proprietary dependencies for the mesh path, Play Services not required).

### Known limits

* No background sync when Android force-stops the app; the foreground service keeps the link alive
  while the app is running/foregrounded normally.
* Voice notes are AAC/MP4 on Android (web uses Opus/OGG); both clients can play both.
* Battery: the foreground service keeps a BLE/USB link; expect phone-typical drain while connected.

## 4. Docker

A multi-stage image (web build → Rust build → non-root runtime) lives in
[`packaging/Dockerfile`](../packaging/Dockerfile) with a Compose example in
[`packaging/docker-compose.yml`](../packaging/docker-compose.yml). The image is
**not built on the OpenMesh development host** (no Docker there), so treat it as a reviewed
recipe, not a verified build.

```bash
# from the repository root
docker build -f packaging/Dockerfile -t openmesh-backbone:0.2.0 .

docker volume create openmesh-data
docker run -d --name openmesh-backbone --restart unless-stopped \
  --network host \
  -v openmesh-data:/var/lib/openmesh \
  openmesh-backbone:0.2.0
```

* On Linux, `--network host` is recommended: the backbone then sees real peer IPs (per-IP
  handshake rate limiting) and port 9443 is directly reachable. The container binds the panel to
  `127.0.0.1:8787` by default in the Compose file.
* On Docker Desktop (macOS/Windows), host networking is restricted; publish
  `9443:9443` and `127.0.0.1:8787:8787` instead. All remote peers then appear as the Docker
  gateway IP, so per-IP rate limiting becomes global — acceptable for a small trusted network,
  not ideal for a public backbone.
* State (identity, redb store, `backbone.json`, API token, attachments) lives in the
  `openmesh-data` volume under `/var/lib/openmesh`. Back it up before upgrades; see
  [operations guide §5](operations/backbone-deployment.md#5-backup-and-restore).
* Docker published ports bypass ufw/firewalld input rules; see
  [`packaging/firewall.md`](../packaging/firewall.md#docker-caveats).

---

## 5. Build from source

### 5.1 Prerequisites

| Distro | Command |
| --- | --- |
| Fedora | `sudo dnf install rust cargo nodejs pnpm git` (or rustup + corepack) |
| Debian/Ubuntu | `sudo apt install build-essential pkg-config curl git` then [rustup](https://rustup.rs/) and Node 22 + `corepack enable` |
| Arch | `sudo pacman -S rust nodejs npm git` (pnpm via `corepack enable`) |
| macOS | `xcode-select --install`, rustup, Node 22 + `corepack enable` |

The default build has **no C library dependencies** (`real-serial` is opt-in); the Rust
`stable` toolchain and a C linker from your build tools are enough.

### 5.2 Build

```bash
git clone https://github.com/martintondel02/openmesh
cd openmesh

cargo build --release -p openmesh-bridge -p openmesh-cli
pnpm --dir apps/web install --frozen-lockfile
pnpm --dir apps/web build
```

Binaries are in `target/release/openmesh-bridge` and `target/release/openmesh-cli`, the web
bundle in `apps/web/dist`. Install them like §3.2/§3.3 (replace the archive paths with these).

Optional USB-serial radio support (needs libudev on Linux):

```bash
cargo build --release -p openmesh-bridge --features real-serial
```

On a Raspberry Pi or other aarch64 Linux box this source build is currently the supported
installation path (there is no `linux-aarch64` release asset yet). The web bundle can be built on
the Pi (slow) or copied from a release's `openmesh-web-*.tar.gz`.

---

## 6. Run as a service

### 6.1 systemd user service (recommended; no root)

```bash
# Either use the installer...
./install.sh --service

# ...or let the bridge write the unit (v0.2.0):
openmesh-bridge install-service --backbone

# ...or install the template manually:
mkdir -p ~/.config/systemd/user
cp packaging/systemd/openmesh-backbone.user.service \
   ~/.config/systemd/user/openmesh-backbone.service
systemctl --user daemon-reload
systemctl --user enable --now openmesh-backbone.service
```

Check it:

```bash
systemctl --user status openmesh-backbone.service
journalctl --user -u openmesh-backbone.service -f
```

To keep the service running when you log out (headless VPS):

```bash
sudo loginctl enable-linger "$USER"
```

Three ways to get a user unit are supported. All of them run the same `backbone` command and
apply hardening, but the generated unit bodies differ in details — inspect what is actually
installed with `systemctl --user cat openmesh-backbone.service`:

* `scripts/install.sh --service` writes the unit with absolute paths and applies
  `NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome=read-only` with `ReadWritePaths` for the
  data/config dirs, `PrivateTmp`, `Restart=on-failure`, `MemoryMax=512M`.
* `openmesh-bridge install-service --backbone` writes a hardened unit for the current binary and
  arguments (`backbone --data-dir <DIR> --listen <ADDR> [--serve-web <DIR>] [--port <PORT>]
  [--token-file <PATH>]`, `Restart=on-failure`, `RestartSec=3`,
  `ReadWritePaths=<data-dir> <token-dir>`, `WorkingDirectory=$HOME`) and prints the enable
  commands. `<token-dir>` is the parent of `--token-file` when supplied, otherwise the default
  `$XDG_CONFIG_HOME/openmesh` (`~/.config/openmesh`), so the service can create the API token on
  first start. With `--system` it writes to `/etc/systemd/system/` (or prints `sudo tee`
  instructions when not root).
* The template in `packaging/systemd/openmesh-backbone.user.service` uses `%h` for manual
  installs.

Inspect what is actually installed with `systemctl --user cat openmesh-backbone.service`.

### 6.2 System-wide service (root)

Use this for a dedicated VPS account/box. It runs as a dedicated `openmesh` system user and keeps
state in `/var/lib/openmesh`, config in `/etc/openmesh`.

```bash
# 1. Create the unprivileged user/group
sudo systemd-sysusers packaging/sysusers/openmesh.conf

# 2. Install binaries, web bundle and docs per §3.3

# 3. Install the unit
sudo install -m 0644 packaging/systemd/openmesh-backbone.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now openmesh-backbone.service

# 4. Verify
systemctl status openmesh-backbone.service
journalctl -u openmesh-backbone.service -f
curl -fsS http://127.0.0.1:8787/api/health
```

The unit creates `/var/lib/openmesh` (`StateDirectoryMode=0700`) and `/etc/openmesh`
(`ConfigurationDirectoryMode=0750`), and passes `--token-file /etc/openmesh/bridge-token`. On first
start the daemon generates `backbone.json` (0600) and the API token. Note that the unit's
`ProtectHome=yes` means user homes are inaccessible — this is intentional.

`systemd-analyze verify packaging/systemd/openmesh-backbone.service` may warn that
`/usr/local/bin/openmesh-bridge` is missing if you run it before installing; the unit itself is
valid.

### 6.3 Containers

See [§4](#4-docker) and `packaging/docker-compose.yml`. With Docker, the API is inside the
container; reach it from the host (`curl http://127.0.0.1:8787/api/health`) or via a TLS reverse
proxy/SSH tunnel. Do not publish `8787`.

---

## 7. First run, panel and joining

### 7.1 Start and read the onboarding output

```bash
openmesh-bridge backbone --data-dir ~/.openmesh \
  --serve-web ~/.local/share/openmesh/web --listen 127.0.0.1:8787
```

The first start prints an onboarding guide with the listen address, the **OBB backbone token
(shown once)**, the connection string, panel URL, firewall examples and service commands. The
guide is printed again, without starting the daemon, with `--print-setup`
(`openmesh-bridge backbone --data-dir ~/.openmesh --print-setup`). Useful diagnostics:

```bash
curl -fsS http://127.0.0.1:8787/api/health
cat ~/.openmesh/backbone.json          # config; mode 0600 — contains the OBB backbone token
cat ~/.config/openmesh/bridge-token    # API/panel bearer token; mode 0600 (value not printed)
```

There are **two different secrets**, and they are easy to confuse:

| Secret | Where | Used for |
| --- | --- | --- |
| **OBB backbone token** | `<data-dir>/backbone.json` (`token` field), printed once on first run / `--print-setup` | authenticating peer links (`openmesh://backbone?...&token=...`); never sent on the wire, only proved via HMAC |
| **Bridge API token** | `~/.config/openmesh/bridge-token` (or `/etc/openmesh/bridge-token`), mode 0600 | the local REST/WebSocket API and the web panel (Settings → Bearer token) |

On the `backbone` subcommand, `--token` / `--token-file` configure the **bridge API token**
(global bridge flags, same as for a normal bridge run) — not the OBB secret. Automation can
pre-provision the OBB secret with the additive `--backbone-token` / `--backbone-token-file`
flags. Do not commit or log either secret.

`backbone.json` holds the OBB/1 settings (`bind`, `port`, `auth`, `token`, `max_peers`,
`allowlist`, `store_forward`, `name`, `public_host`, `ping_interval_s`). The OBB listener address
comes from `--bind`/`--port` or this file (default `0.0.0.0:9443`); `--listen` always configures
the **local API** (`127.0.0.1:8787`). Changing settings via the panel applies live; changing
`auth`/`token`/`allowlist` disconnects peers that no longer authenticate.

### 7.2 Open the panel

<http://127.0.0.1:8787/backbone>

* The panel is served from the installed web bundle. Open **Settings** in the UI, set the bridge
  URL (`http://127.0.0.1:8787`) and paste the bearer token from
  `~/.config/openmesh/bridge-token`; the token is stored in the browser's local storage.
* The setup payload (`GET /api/backbone/setup`, or `--print-setup`) reports the OBB/1 listener as
  `listen` (`0.0.0.0:9443`) and the local API as `panel_url`; the connection string includes the
  **OBB backbone token**, not the bridge API token.
* The backbone page shows status, live counters, peers (with RTT/bytes/kick), configuration
  controls, token rotation (the new token is shown **once**), the setup wizard with copy buttons
  and a QR code for the `openmesh://backbone?...` link.
* From a remote machine, do not expose 8787. Use an SSH tunnel
  (`ssh -L 8787:127.0.0.1:8787 user@host`) or a TLS reverse proxy with authentication — see
  `packaging/examples/Caddyfile.panel`, `nginx-panel.conf` and the
  [operations guide §6](operations/backbone-deployment.md#6-tls-and-vpn-options).

### 7.3 Join a backbone from another bridge/gateway

On the other host (install first), paste the connection string from the panel:

```bash
openmesh-bridge join 'openmesh://backbone?host=mesh.example.org&port=9443&token=…&node=…&name=…&v=1' \
  --data-dir ~/.openmesh
```

The joining bridge reconnects with backoff and appears in the backbone peer table (and in the
panel). Use `openmesh-cli backbone info` / `peers` on the backbone host for terminal diagnostics;
`openmesh-cli backbone rotate-token`, `kick <node_id>` and `connect <url>` wrap the same API.

**Open mode** (`auth: "open"`, explicit opt-in) accepts peers without a token. Only use it on a
trusted LAN or VPN; the panel shows a red warning, because anyone who can reach the port can
inject OMP frames (they still cannot read messages — payloads are end-to-end encrypted — but they
can relay, spam and observe metadata).

### 7.4 Firewall

Open only the backbone port publicly (default **9443/tcp**); keep **8787/tcp** loopback-only.
Fedora firewalld:

```bash
sudo firewall-cmd --permanent --add-port=9443/tcp
sudo firewall-cmd --reload
```

Debian/Ubuntu ufw:

```bash
sudo ufw allow 9443/tcp
```

Cloud hosts have a second security-group firewall: allow 9443/tcp there too (or only the
WireGuard/Tailscale port if you use a VPN). Full rules, nftables and Docker caveats:
[`packaging/firewall.md`](../packaging/firewall.md).

---

## 8. Updating

The script installer is the supported upgrade path: it is idempotent and never touches config,
tokens, identity or messages.

```bash
# latest release
curl -fsSL https://github.com/martintondel02/openmesh/releases/latest/download/install.sh | sh

# or a pinned release
curl -fsSL https://github.com/martintondel02/openmesh/releases/latest/download/install.sh | sh -s -- --version v0.2.0

# then restart the service if you use one
systemctl --user restart openmesh-backbone.service      # user service
sudo systemctl restart openmesh-backbone.service        # system service
```

Manual upgrades: repeat §3 downloading the new version and copying the binaries/web bundle,
then restart. Keep the old binary (e.g. `cp openmesh-bridge openmesh-bridge.prev`) until you have
verified `/api/health` and the peer table after the restart — rolling back is a copy away.

Wire compatibility: v0.2.0 keeps the v1 bridge API and OMP/1 wire format; backbone mode is
additive. Back up `~/.openmesh/identity.key` (or `/var/lib/openmesh/identity.key`) before any
upgrade: it is the node identity, and losing it changes the node id.

---

## 9. Uninstalling

Script installs:

```bash
./install.sh --uninstall          # user install
sudo ./install.sh --system --uninstall
```

This removes the binaries, web bundle, docs and the unit written by the installer. It **keeps**
data and config so a reinstall keeps identity and messages; remove them manually for a clean
slate:

```bash
rm -rf ~/.openmesh ~/.config/openmesh
# system install: sudo rm -rf /var/lib/openmesh /etc/openmesh
```

Manual/systemd installs: disable the service (`systemctl --user disable --now
openmesh-backbone.service` or `sudo systemctl disable --now openmesh-backbone.service`), remove
the unit and binaries, then remove the data/config directories if desired.

---

## 10. Troubleshooting

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| `no official linux-aarch64 build yet` | Running the installer on ARM Linux | Build from source (§5) or use the Docker image on x86_64. |
| `no official build for Intel macOS` | Prebuilt releases are arm64 only | Build from source (§5). |
| `checksum mismatch for …` | Corrupt/tampered download or stale mirror | Re-download; verify `SHA256SUMS.txt` from the same release; the installer installs nothing on mismatch. |
| `SHA256SUMS.txt does not list …` | Incomplete release/mirror or wrong `--version` | Use the matching release or regenerate the mirror's checksums; do not install unverified files. |
| `download failed: …` | Network/proxy/DNS or GitHub outage | Retry, set `HTTPS_PROXY`, or use `--base-url`/manual install (§3). |
| `--system needs sudo` / confirmation prompt in CI | Root install on a non-interactive session | Re-run with `--yes` (and ensure the user may sudo) or install as a user. |
| `openmesh-bridge: command not found` | `~/.local/bin` not on `PATH` | `export PATH="$HOME/.local/bin:$PATH"`, or log in again. |
| Permission denied binding 9443 | Port <1024 or SELinux/AppArmor | Default 9443 is high; check `sudo ss -ltnp` for another process and your firewall/SELinux policy. |
| Panel shows "unauthorized"/401 | Missing/wrong bridge API token | Paste `~/.config/openmesh/bridge-token` into Settings → Bridge; the health endpoint is public, everything else needs the token. |
| Panel is blank after install | Wrong `--serve-web` path or stale bundle | Point `--serve-web` at the directory containing `index.html` (the `dist/` contents) and restart. |
| Peers cannot connect | Firewall/cloud security group, wrong public port, TLS wrapper mismatch | Check `nc -vz host 9443`, open 9443/tcp, and if using nginx-TLS/stunnel ensure the client joins the local stunnel port (§6 of the ops guide). |
| `auth_failed` in logs/panel | Wrong/rotated backbone token, allowlist miss, max peers | Copy a fresh connection string from the panel, check the allowlist/max_peers; rotate only when intended. |
| Token lost (backbone) | `backbone.json` deleted/corrupt | Restore the file/backup, or rotate the token in the panel (`Rotate token`) and update clients. |
| Service exits with "path not found" | `--bin-dir`/`--data-dir` moved after install | Re-run the installer or reinstall the unit with the new paths. |
| High CPU/disk usage | Large attachments, small `store_ttl_ms`, many peers | See [operations guide §3–§4](operations/backbone-deployment.md#3-sizing-and-capacity); tune `max_peers`, storage, retention. |
| macOS "cannot be opened because the developer cannot be verified" | Not notarised | `xattr -d com.apple.quarantine` on both binaries (§3.4). |
| Windows firewall prompts each start | New binary path allowed? | Allow the app for private/public networks as intended, or add an inbound rule for 9443/tcp. |

Still stuck? Include `openmesh-bridge --version`, `uname -a`, the relevant `journalctl` lines and
the API `/api/health` and `/api/backbone` output (redact tokens) in a
[GitHub issue](https://github.com/martintondel02/openmesh/issues).

---

## 11. Security checklist

* [ ] The panel/API (`8787`) is **not** reachable from the internet; use SSH tunnel, TLS reverse
      proxy with auth, or WireGuard/Tailscale.
* [ ] Only `9443/tcp` (or your VPN port) is open publicly.
* [ ] `backbone.json` and the API token file are mode `0600` and owned by the service user.
* [ ] The backbone runs as an unprivileged user (systemd units already do this; containers run as
      uid 10001).
* [ ] The token is pasted into clients/panel deliberately, never committed or logged. Rotate it
      from the panel if it may have leaked; rotation disconnects stale peers immediately.
* [ ] Open mode is off unless the network is trusted; the panel shows a red warning when it is on.
* [ ] On hostile networks use WireGuard/Tailscale (recommended) or TLS wrapping
      (`packaging/examples/nginx-stream-tls.conf` + `stunnel-client.conf`); v0.2.0 has no native
      TLS inside OBB/1.
* [ ] Backups of `identity.key`, `backbone.json` and the API token exist, offline, with the same
      care as any other credential.
* [ ] Host is patched; use the hardened systemd unit or the non-root container image.

Backbone relayed payloads are **end-to-end encrypted** OMP frames; the operator can see
pseudonymous node ids, sizes and timing, but not message contents. This and the trust model are
documented in [BACKBONE.md §10](architecture/BACKBONE.md#10-security-model) and
[ADR 0009](adr/0009-backbone-mode.md).

---

## 12. Path reference

| Path (user install) | Path (system install) | Contents |
| --- | --- | --- |
| `~/.local/bin/openmesh-bridge` | `/usr/local/bin/openmesh-bridge` | daemon |
| `~/.local/bin/openmesh-cli` | `/usr/local/bin/openmesh-cli` | CLI |
| `~/.local/share/openmesh/web` | `/usr/share/openmesh/web` | web panel bundle (`index.html`, assets) |
| `~/.local/share/doc/openmesh` | `/usr/share/doc/openmesh` | LICENSE, NOTICE, README |
| `~/.openmesh` | `/var/lib/openmesh` | `identity.key`, `openmesh.redb`, `attachments/`, `backbone.json` |
| `~/.config/openmesh/bridge-token` | `/etc/openmesh/bridge-token` | bridge API/panel bearer token (0600) |
| `~/.config/systemd/user/openmesh-backbone.service` | `/etc/systemd/system/openmesh-backbone.service` | service unit |

`~/.openmesh` is the data directory used by the installer, README examples and generated unit.
If you launch the bridge **without** `--data-dir`, its built-in default is
`$XDG_DATA_HOME/openmesh` (`~/.local/share/openmesh`) — pick one and be consistent, or always pass
`--data-dir`.

---

## 13. See also

* [Backbone mode specification](architecture/BACKBONE.md) — OBB/1, configuration, API, security.
* [Backbone deployment & operations](operations/backbone-deployment.md) — sizing, monitoring, TLS,
  WireGuard, multi-region, upgrades, incident checklist.
* [Bridge API](architecture/WEBSOCKET-API.md) §8 — panel/backbone REST + WS surface.
* [Roadmap](ROADMAP.md) · [Architecture](architecture/ARCHITECTURE.md) · [ADRs](adr/).
* Packaging templates: [`packaging/README.md`](../packaging/README.md).
