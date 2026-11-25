# Install OpenMesh

OpenMesh ships as a single bridge daemon plus a web app, with optional CLI tools and radio
firmware. This topic covers the supported ways to install it, what each piece is for, and the
paths it writes — so you can find (and back up) everything later.

## What it is

* **openmesh-bridge** — the daemon. It owns your identity, radios, message store and the local
  API/UI server. Everything else is a client of it.
* **openmesh-cli** — a terminal client and diagnostic tool. Handy for scripting and for checking
  things when the browser will not cooperate.
* **web app** — the browser interface served by the bridge at `/`, including the Help Center.
* **radio firmware** — optional, flashed separately to LoRa boards (see the hardware docs).

## Why it matters

A correct install is what makes "no account, no cloud" work: the identity and messages live on
*your* disk, and the bridge only listens on loopback. Installing it the wrong way (running an old
binary, missing the web bundle, exposing the API port) is the most common source of confusion
later on. The installer verifies checksums and never overwrites your data.

## Quick start

### One-line installer (Linux x86_64, macOS Apple Silicon)

```sh
curl -fsSL https://github.com/martintondel02/openmesh/releases/latest/download/install.sh | sh
```

The script:

1. resolves the latest release (or a version you pin with `--version v0.3.0`);
2. downloads the binary archive and the web bundle and verifies both against the release's
   `SHA256SUMS.txt` — a mismatch aborts before anything is installed;
3. installs to user paths (no root): binaries in `~/.local/bin`, web in
   `~/.local/share/openmesh/web`, docs in `~/.local/share/doc/openmesh`;
4. creates `~/.openmesh` (data) and `~/.config/openmesh` (API token) with owner-only permissions;
5. prints the panel URL, the command to start a backbone, firewall hints and a `PATH` note.

Useful options: `--dry-run` (show the plan), `--version v0.3.0` (pin), `--system` (install under
`/usr/local` and `/usr/share/openmesh`, asks for sudo), `--service` (install a systemd user
service), `--no-web`, `--uninstall`. Re-running the installer is the supported upgrade path.

### Manual install from a release

Download these files from the [releases page](https://github.com/martintondel02/openmesh/releases):

| Asset | Contents |
| --- | --- |
| `openmesh-v0.3.0-linux-x86_64.tar.gz` | binaries + LICENSE + NOTICE + README (macOS: `-macos-arm64`, Windows: `-windows-x86_64` with `.exe`) |
| `openmesh-web-v0.3.0.tar.gz` | `dist/` — the web app |
| `SHA256SUMS.txt` | checksums for all assets |

Verify, then copy:

```sh
sha256sum -c SHA256SUMS.txt --ignore-missing      # macOS: shasum -a 256 -c …
tar -xzf openmesh-v0.3.0-linux-x86_64.tar.gz
mkdir -p ~/.local/bin ~/.local/share/openmesh/web ~/.openmesh
cp openmesh-v0.3.0-linux-x86_64/openmesh-bridge ~/.local/bin/
cp openmesh-v0.3.0-linux-x86_64/openmesh-cli ~/.local/bin/
tar -xzf openmesh-web-v0.3.0.tar.gz
cp -R dist/. ~/.local/share/openmesh/web/
```

### Run it

```sh
openmesh-bridge --demo        # virtual mesh; great for a first look
openmesh-bridge               # real radios/LAN, local UI at http://127.0.0.1:8787
```

`--demo` needs no hardware. For real radios remove `--demo` and see the radio topic.

### Windows and macOS notes

* **Windows** — releases are `.tar.gz` archives. Windows 10+ has `tar.exe`; 7-Zip also works.
  Extract, then run `openmesh-bridge.exe --demo`. The browser UI works the same. There is no
  supported Windows service wrapper in this release; run it from a terminal or Task Scheduler.
* **macOS Intel** — no prebuilt Intel build; build from source. Apple Silicon has a release asset.
  If Gatekeeper blocks the binary, right-click → Open, or remove the quarantine flag with
  `xattr -d com.apple.quarantine <binary>`.
* **Linux ARM (Raspberry Pi)** — no release asset yet; build from source on the device. The web
  bundle from the release can be reused.

## Advanced notes

* **Custom locations** — `--data-dir DIR`, `--listen ADDR`, `--serve-web DIR` and `--token-file
  PATH` are global flags. The installer defaults are `~/.openmesh`, `127.0.0.1:8787`,
  `~/.local/share/openmesh/web` and `~/.config/openmesh/bridge-token`.
* **System install** — `sudo ./install.sh --system` installs to `/usr/local/bin` and
  `/usr/share/openmesh/web`; a systemd unit and dedicated user are provided in `packaging/`.
  Run the bridge as an unprivileged user and keep the API on loopback.
* **Services** — `openmesh-bridge install-service --backbone` writes a hardened systemd user unit
  (or `--system` for a system unit) and prints the enable commands. `install.sh --service` does
  the same during install.
* **Tokens** — the API token is a bearer secret stored mode `0600`. The panel asks for it once
  and keeps it in browser storage. If it leaks, stop the bridge, delete the token file and restart
  to generate a new one (all clients must paste the new token).
* **Build from source** — `cargo build --release -p openmesh-bridge -p openmesh-cli` and
  `pnpm --dir apps/web build`; the default build needs no system libraries (serial support is
  opt-in with `--features real-serial`).
* **Upgrades** — keep `identity.key` and `backbone.json`; binaries are replaceable. Keep the
  previous binary until `/api/health` and your peers are healthy after the restart.

## Troubleshooting

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| `checksum mismatch` from the installer | corrupt download or wrong mirror | Re-download; never bypass the check. |
| `no official build for …` | unsupported platform (e.g. Linux ARM, Intel macOS) | Build from source. |
| Installer says `sudo` needed | you passed `--system` | Re-run with `--yes` for non-interactive provisioning, or install as a user. |
| Browser shows blank page | web bundle missing or `--serve-web` points at the wrong directory | Point it at the directory containing `index.html`, or reinstall the web asset. |
| API returns 401 | wrong/missing API token | Paste `~/.config/openmesh/bridge-token` into Settings → Bridge. |
| Port already in use | another bridge instance, or port 8787 busy | Stop the other process or pass `--listen 127.0.0.1:8788`. |

## Related topics

[Your first mesh in 10 minutes](getting-started.md) · [Run a backbone](backbone.md) ·
[Security and privacy](security.md) · [Troubleshooting](troubleshooting.md)
