# OpenMesh web flasher

Static, zero-build web page that flashes OpenMesh firmware to the four reference boards. No
Node.js, no bundler: open `index.html` or serve the directory with any static file server.

```
cd apps/flasher
python3 -m http.server 8000
# open http://localhost:8000
```

## What works where

| Context | Board cards / info | UF2 downloads | ESP32 WebSerial flash |
| --- | --- | --- | --- |
| `file://index.html` | yes | yes | no (WebSerial requires a secure context) |
| `http://localhost:8000` | yes | yes | yes in Chrome/Edge 89+ |
| `https://…` | yes | yes | yes in Chrome/Edge 89+ |
| Firefox/Safari | yes | yes | no (no WebSerial); the page points at the CLI |

The page detects `navigator.serial` and secure-context support and shows a status banner. When
opened from `file://`, `fetch('boards.json')` is blocked by the browser, so the page falls back to
the embedded copy in `js/boards-data.js` automatically.

## Files

```
index.html                 page shell
css/styles.css             styling (dark, responsive, board cards)
js/app.js                  rendering, browser detection, copy buttons
js/boards-data.js          offline fallback copy of boards.json
boards.json                release tag + base URL + per-board artefacts (fetched when served)
manifests/                 esp-web-tools manifests (ESP32-S3 boards)
assets/                    optional board images
```

## Updating releases

`boards.json` drives every download link. On a new firmware release, update:

```json
"release": { "tag": "v0.1.1", "base_url": "https://github.com/<owner>/<repo>/releases/download/v0.1.1" }
```

and, if the artefact names changed, the per-board `uf2` / `manifest` fields. Keep
`js/boards-data.js` in sync so the `file://` fallback still links to valid files.

Expected artefacts (produced by `firmware/ci/build.yml`):

| Board | File | Flash path |
| --- | --- | --- |
| T1000-E | `openmesh-t1000e.uf2` | UF2 copy |
| RAK4631 solar | `openmesh-rak4631_solar.uf2` | UF2 copy |
| Seeed Solar P1 | `openmesh-seeed_solar.uf2` | UF2 copy |
| Heltec Wireless Tracker | `openmesh-heltec_tracker-full.bin` | esp-web-tools manifest at offset 0 |

For ESP32, build a merged image for the web flasher:

```bash
esptool.py --chip esp32s3 merge_bin -o openmesh-heltec_tracker-full.bin \
  --flash_mode dio --flash_freq 80m --flash_size 8MB \
  0x0 bootloader.bin 0x8000 partitions.bin 0xe000 boot_app0.bin 0x10000 firmware.bin
```

`manifests/heltec_tracker.json` references the file as a relative path, so keep the `.bin` next
to the manifest (or pass an absolute URL) when publishing.

## esp-web-tools

The `<esp-web-install-button>` custom element is loaded from the unpkg CDN
(`esp-web-tools@10.1.0`). To make the page fully offline, vendor
`install-button.js` + `esp-web-tools.js` from the npm package into `vendor/esp-web-tools/` and
point the `<script type="module">` at the local copy. WebSerial itself does not need network
after the page has loaded.

## Browser support notes

* WebSerial is Chromium-only (Chrome, Edge, Opera) and requires HTTPS, `localhost`, or `127.0.0.1`.
* The ESP32-S3 USB-JTAG interface appears as a serial port; a USB cable that can carry data is
  required (charge-only cables are a common failure).
* If the install button stays disabled, check the banner: it distinguishes "WebSerial missing",
  "insecure context" and "no USB device" cases.
* nRF52 flashing never uses WebSerial: the operating system copies the UF2 to the bootloader
  drive, so it works in every browser and from `file://`.

## Development checks

```bash
node --check js/app.js
node --check js/boards-data.js
python3 -m json.tool boards.json > /dev/null
python3 -m json.tool manifests/heltec_tracker.json > /dev/null
```
