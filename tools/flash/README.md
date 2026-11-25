# openmesh-flash — Linux flashing CLI

`openmesh-flash` flashes OpenMesh firmware to the four reference boards from Linux. It is a
single Python 3 script using only the standard library (no `pip install` required for nRF52
boards; ESP32 boards need `esptool`).

```
tools/flash/openmesh-flash --list
tools/flash/openmesh-flash --board auto --dry-run
tools/flash/openmesh-flash --board t1000e
tools/flash/openmesh-flash --board heltec_tracker --port /dev/ttyACM0
```

No root required. nRF52 boards are flashed by copying a `.uf2` file to the bootloader's
mass-storage drive; ESP32-S3 boards are flashed with `esptool`.

## Supported boards

| Key | Board | MCU | Flash method | Release artefact |
| --- | --- | --- | --- | --- |
| `t1000e` | Seeed SenseCAP T1000-E | nRF52840 | UF2 copy | `openmesh-t1000e.uf2` |
| `rak4631_solar` | RAK WisBlock 4631 solar node | nRF52840 | UF2 copy | `openmesh-rak4631_solar.uf2` |
| `seeed_solar` | Seeed SenseCAP Solar Node P1 | nRF52840 | UF2 copy | `openmesh-seeed_solar.uf2` |
| `heltec_tracker` | Heltec Wireless Tracker | ESP32-S3 | esptool | `openmesh-heltec_tracker-full.bin` |

`--list` also prints currently attached USB devices with their VID:PID and the board they map
to, plus any detected UF2 bootloader drive. `--list-json` emits the same information as JSON for
scripts.

## Fedora setup

Python 3 is installed by default. Install esptool for the ESP32-S3 target:

```bash
python3 -m pip install --user esptool
# or, from Fedora repositories:
sudo dnf install python3-esptool
```

Serial permissions: add yourself to the `dialout` group, then **log out and back in** (group
changes only apply to new sessions):

```bash
sudo usermod -aG dialout "$USER"
groups | grep dialout        # after re-login
```

Most Fedora systems already include a `uaccess` rule for Espressif (VID `303a`) and common USB
serial bridges, so `/dev/ttyACM0` is user-accessible once `dialout` membership is active. If you
use a CP210x/CH340 adapter that is not covered, copy the vendor udev rule shipped in
`/usr/share/doc/` or create `/etc/udev/rules.d/99-openmesh.rules`:

```udev
# Espressif ESP32-S3 native USB (WebSerial/esptool)
SUBSYSTEM=="usb", ATTRS{idVendor}=="303a", MODE="0666", TAG+="uaccess"
# CP210x / CH340 USB-UART bridges
SUBSYSTEM=="tty", ATTRS{idVendor}=="10c4", ATTRS{idProduct}=="ea60", MODE="0666", TAG+="uaccess"
SUBSYSTEM=="tty", ATTRS{idVendor}=="1a86", ATTRS{idProduct}=="7523", MODE="0666", TAG+="uaccess"
```

Reload rules with `sudo udevadm control --reload && sudo udevadm trigger` (replug the board).

UF2 drives (`/run/media/<user>/T1000E`) mount as your user and need no udev rule.

## nRF52 boards (T1000-E, RAK4631 solar, Seeed Solar P1)

1. Enter the UF2 bootloader:
   * **T1000-E**: double-tap the side button (or double reset) until the drive `T1000E` appears.
   * **RAK4631**: double-tap the reset button (`RAK4631` drive, requires an Adafruit nRF52
     bootloader; a bare RAK4631 without one must be flashed with `nrfutil`/J-Link, see
     `docs/hardware/boards.md`).
   * **Seeed Solar P1**: double reset (`SEEEDSOLAR` drive).
2. Run `openmesh-flash --board auto` (detects the drive) or pass the board key explicitly.
3. The tool downloads the release artefact (cached in `~/.cache/openmesh-flash`), verifies the
   published `.sha256` when available, copies it, and waits for the board to reboot.

## ESP32-S3 (Heltec Wireless Tracker)

1. Plug in USB. If the tool cannot open the port, force download mode: hold **BOOT**, tap
   **RESET**, release **BOOT**.
2. `openmesh-flash --board heltec_tracker --port /dev/ttyACM0`
3. `--erase` performs a full `erase_flash` first (wipes NVS/settings). Flashing already replaces
   the firmware; `--erase` is only needed to clear a corrupted NVS.

## Offline / custom images

```bash
# local artefact (no network)
openmesh-flash --board t1000e --file ./openmesh-t1000e.uf2 \
  --sha256 "$(sha256sum ./openmesh-t1000e.uf2 | cut -d' ' -f1)" --dry-run

# a local release directory (served or mounted)
openmesh-flash --board seeed_solar --base-url /srv/openmesh-release
openmesh-flash --board seeed_solar --base-url file:///srv/openmesh-release
```

`--dry-run` still performs detection, download and checksum verification, then prints the exact
copy/esptool operation without writing anything.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | success |
| 1 | flashing error / aborted |
| 2 | usage error (unknown board, missing argument) |
| 3 | board or port not found |
| 4 | download or checksum failure |

## Troubleshooting

| Symptom | Fix |
| --- | --- |
| `No UF2 bootloader drive found` | Re-enter bootloader mode; check `--list` output; mount manually and pass `--board`. |
| `no permission to open /dev/ttyACM0` | `sudo usermod -aG dialout "$USER"`, then log out/in. |
| `esptool is not installed` | `python3 -m pip install --user esptool`. |
| `sha256 mismatch` | Delete `~/.cache/openmesh-flash` and retry; if using `--file`, verify the file yourself. |
| Board reboots but no mesh traffic | Check antenna and region (`radio.region` config via companion API), see `docs/hardware/boards.md`. |

## Development

The script has no test double; run the self-checks:

```bash
python3 -m py_compile tools/flash/openmesh-flash
tools/flash/openmesh-flash --list
tools/flash/openmesh-flash --board auto --dry-run
```
