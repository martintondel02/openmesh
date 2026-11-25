# OpenMesh web client

The reference client for OpenMesh: a dense, dark, mobile-responsive React app for chatting,
sharing images and sending voice notes over a LoRa mesh — plus a map of the mesh, node
diagnostics, contact verification and radio settings.

It speaks the normative bridge API in
[`docs/architecture/WEBSOCKET-API.md`](../../docs/architecture/WEBSOCKET-API.md) when a bridge is
running, and falls back to a seeded demo mesh so the UI can be reviewed with no backend at all.

**Stack:** React 19 · TypeScript · Vite · Tailwind CSS 3.4 · zustand · lucide-react · MapLibre GL
JS · Vitest. Packaged for desktop with Tauri 2 (`src-tauri/`).

**Screenshots (all captured in demo mode, no backend):**
[Chats](docs/screenshots/chats.png) ·
[Thread with image + voice](docs/screenshots/thread.png) ·
[Mesh map](docs/screenshots/map.png) ·
[Nodes](docs/screenshots/nodes.png) ·
[Contacts](docs/screenshots/contacts.png) ·
[Settings](docs/screenshots/settings.png) ·
[Mobile list](docs/screenshots/mobile-chats.png) ·
[Mobile thread](docs/screenshots/mobile-thread.png)

---

## Quick start

```bash
# from the repo root
pnpm --dir apps/web install
pnpm --dir apps/web dev          # http://127.0.0.1:5173

# force the seeded demo mesh (works with zero backend)
open "http://127.0.0.1:5173/?demo=1"

# deep-link directly to a screen or conversation (handy for screenshots)
open "http://127.0.0.1:5173/?demo=1&screen=map"
open "http://127.0.0.1:5173/?demo=1&thread=anne"          # or ?thread=familien

pnpm --dir apps/web test         # vitest (119 unit tests)
pnpm --dir apps/web build        # tsc -b && vite build
pnpm --dir apps/web preview      # serve the production build
```

If no bridge answers on `http://127.0.0.1:8787/api/health` within ~1.8 s, the app automatically
enters demo mode and says so in the header. The connection pill (top right) shows
`Connected`/`Demo mesh`/`Reconnecting`, the bridge version, and lets you retry the bridge.

### Connecting to a real bridge

1. Start `openmesh-bridge` (see `crates/openmesh-bridge`). Default: `http://127.0.0.1:8787`.
2. Copy the token from `~/.config/openmesh/bridge-token`.
3. In **Settings → Bridge connection** enter the URL + token and press **Save & reconnect**.

The WebSocket handshake uses `?token=<token>` (browsers cannot set headers); REST attachment
uploads use `Authorization: Bearer <token>`.

### Demo mode

Demo mode is a first-class review mode, not a stub:

- 8 seeded nodes around Oslo (clients, repeaters, a solar gateway, trackers) with realistic
  RSSI/SNR, hops, battery, caps, identities and 25-point position trails.
- Direct chats (unread traffic, day separators, reply quotes, a failed send with retry) and two
  signed channels (`familien`, `nordmarka-tur`).
- A generated SVG "ridge" photo and hand-drawn trail sketch, plus a real, playable voice note
  (synthesised WAV exposed as `localUrl`).
- The mock transport answers every command the same way the bridge does (`sync`, `send_message`
  → `send_result` + echo + `ack`, `set_config`, `create_channel`, `contact_qr`, …) and keeps the
  mesh alive with status ticks and incoming traffic.

Forced with `?demo=1`, `localStorage.openmesh.demo = '1'`, or the Settings toggle. The URL
parameter wins over everything, so screenshots are reproducible.

---

## What is implemented

| Area | Details |
| --- | --- |
| **App shell** | Left nav rail (compact icon rail on md, labelled on xl), mobile bottom tab bar, context top bar with mesh stats, connection pill, node identity chip (click to copy), demo banner. |
| **Chats** | Channels + direct conversations, unread badges, last message + delivery tick, search (⌘K), filters. Thread: day separators, grouped bubbles, reply quoting with jump-to, optimistic send, retry on failure, delivery ticks (pending/sent/delivered/read/failed), auto-scroll + jump-to-latest, sticky thread header, share-position action. |
| **Composer** | Enter to send / Shift+Enter newline, grapheme-safe text handling, live airtime estimate (`~0.7 s airtime · 1 frame`), attachment + voice buttons, drag-and-drop and clipboard-paste images, image lightbox. |
| **Groups** | Create channel (generated 256-bit PSK shown once with copy + `openmesh://channel` invite link + QR), join via pasted link or name+PSK, channel invite modal (QR + share sheet), per-message Ed25519 signature badge, leave channel with confirmation. |
| **Images** | File picker / drag-drop / paste → canvas downscale + WebP (JPEG fallback) encode loop until the target size fits the profile cap; slider from 1 KiB to the profile cap with Fastest/Balanced/Max presets; chunks, airtime, transfer time and % of the region duty-cycle budget shown before sending; progress overlay for incoming resources. |
| **Voice** | `MediaRecorder` with `audio/ogg;codecs=opus` preferred (WebM fallback), live timer + waveform from a WebAudio analyser, auto-stop at the profile voice cap, review with playback, then bubble with waveform bars, seek, playback progress and duration. |
| **Map** | MapLibre GL with a **bundled offline schematic basemap** of the Oslo area (fjord, lakes, Nordmarka, roads — no tile requests, no glyph server). Node markers coloured by role (DOM markers, hover labels), own-position pulse + accuracy circle, position-history polyline, mesh link lines, role filters, selected-node popover, fit-to-nodes, geolocation locate/share, optional online OSM raster with automatic "tiles unavailable" fallback. |
| **Nodes** | Table (desktop) / cards (mobile) with role, caps icons, hops, RSSI bars + dBm, SNR, battery, last heard; filters + search; detail drawer with 72-byte identity, contact QR, raw identity copy, position, link metrics, contact add/remove, message action. |
| **Contacts** | Contact cards with verified badge and last seen; add via invite link or 72-byte identity + node id (auto-derived); QR display; delete with confirmation; own contact QR + explicit "no camera scanner in the browser" note. |
| **Settings** | Node name/class, announce interval with duty-cycle warning, auto-announce, radio profile + region selector with computed attachment/voice caps, announce vs budget percentage, hourly media budget, TX power, position sharing, store-forward, bridge URL + token (show/hide, test, save & reconnect), live session/radios panel, demo-mode toggle, danger zone reset identity with typed confirmation. |
| **Accessibility** | Skip link, focus-visible rings, aria labels/pressed/current/live, modal focus handling + Escape, keyboard nav, `prefers-reduced-motion` respected, AA-ish contrast on dark surfaces. |

---

## Media numbers (from the protocol)

Sizes and caps come from [`PROTOCOL.md` §9.4](../../docs/architecture/PROTOCOL.md); airtime uses the
Semtech LoRa time-on-air formula with explicit header, CRC on, 8 preamble symbols and
low-data-rate optimisation for SF11/SF12 at 125 kHz:

- Resource chunk = 180 B, frame = 22 B header + 7 B resource fields + 180 B payload = 209 B ≤ 240 B MTU.
- 5 KiB image → 29 chunks → ~16.6 s airtime at `MEDIUM_FAST` (docs' illustrative ~20 s).
- 8 s Opus voice (~7 KiB) → ~22.5 s airtime at `MEDIUM_FAST`.
- Announce (200 B) → ~0.5 s at `MEDIUM_FAST`.
- Duty-cycle wall clock: at EU868 1 %, 20 s of airtime needs ~33 min of budget time, which the
  attachment dialog shows honestly before you send.

Profiles not in the §9.4 table (`SHORT_SLOW`, `MEDIUM_SLOW`, `LONG_TURBO`) get conservative derived
caps documented in `src/lib/airtime.ts`.

---

## Tests

```bash
pnpm --dir apps/web test
```

119 Vitest tests cover: LoRa airtime + chunk/transfer math and protocol worked examples, image
compression planning (dimensions, quality, slider mapping, Opus sizes), formatting/Unicode/focus
helpers, `openmesh://` link parsing + identity validation, WebSocket envelope parsing + reconnect
transport (fake socket), store reducers (optimistic send, acks, merge semantics, conversations),
seeded demo-dataset invariants, geo helpers and the offline map style.

**Integration smoke check** (headless Firefox via WebDriver BiDi, production build served by
`vite preview`): opens all five demo conversations and all five tabs, opens the new-conversation
dialog, and verifies the thread renders 10 messages including the image bubble and the voice
waveform bubble, scrolled to the bottom — with **0 console errors/warnings** and no React error
boundary. The same pass was reproduced by the Lead with headless Chromium/Playwright.

---

## Project layout

```
apps/web/
├── index.html / vite / tailwind / tsconfig
├── public/                     favicon, manifest, PWA icons
├── src/
│   ├── main.tsx, App.tsx, index.css
│   ├── types/api.ts            wire types copied from WEBSOCKET-API.md
│   ├── lib/
│   │   ├── ws.ts               envelope parser + reconnecting LiveTransport
│   │   ├── transport.ts        transport abstraction (live + demo)
│   │   ├── mock/               seeded Oslo dataset + MockTransport
│   │   ├── airtime.ts          LoRa airtime, chunks, caps, duty cycle
│   │   ├── compression.ts      pure image-planning math
│   │   ├── attachments.ts      canvas compress loop
│   │   ├── audio.ts            waveform/WAV synth + Opus mime picker
│   │   ├── links.ts            openmesh:// invite parsing/building
│   │   ├── geo.ts, map-style.ts  circles/bounds + bundled basemap
│   │   ├── utils.ts            formatting/validation/unicode helpers
│   │   └── __tests__/
│   ├── store/                  zustand store, reducers, selectors (+ tests)
│   ├── components/             hand-rolled UI primitives, shell, QR
│   └── screens/                chats/, map, nodes, contacts, settings
└── src-tauri/                  Tauri 2 desktop shell (see its README)
```

---

## Bridge API conformance notes / ambiguities found

Implemented exactly against the doc; these gaps are worth resolving in the spec:

1. **`hello.node.caps` is a `u16` bitmask** in §3.1 but **`node.caps` is a string array** in §3.3.
   The client accepts both.
2. **Own identity is not exposed.** `hello.node` has no public identity, so the client cannot
   build its own contact QR from local data. The UI calls `contact_qr` with its own node id and
   degrades gracefully if the bridge refuses. A `contact_qr`/`get_identity` response for self (or
   `identity` in `hello.node`) would fix this.
3. **No `mark_group_read` command.** Channel unread state is tracked client-side with per-channel
   read cursors (§4.2 only covers direct `mark_read`).
4. **`join_channel` response is unspecified.** The client accepts `channel_created` and/or a
   `channels` event; demo mode replies with both.
5. **`channel_created`** is documented as "id + PSK once" but the field names are not given; the
   client expects `{channel_id, name, psk?}`.
6. **Attachment bytes auth for `<img>`/`<audio>`**: REST requires a Bearer header, which media
   elements cannot send. The client fetches with `Authorization` and plays from a blob URL.
7. **No identity-reset API** (danger zone explains the `--reset-identity` bridge restart instead).
8. **Profile names beyond the §9.4 size table** (`SHORT_SLOW`, `MEDIUM_SLOW`, `LONG_TURBO`) lack
   attachment/voice caps; derived conservatively and labelled in code.
9. **`message_update` with `attachment.progress`** is used as documented; the demo transport also
   emits it during simulated uploads so the progress UI is exercised.

---

## Desktop packaging (Tauri 2) — not built in this environment

`src-tauri/` contains a Tauri 2 shell (window config, CSP, capabilities, Cargo manifest, icons).
Building it needs the system WebKitGTK development stack, which is **not installed here** — the
web build and tests do not need it.

On Fedora:

```bash
sudo dnf install -y webkit2gtk4.1-devel libappindicator-gtk3-devel librsvg2-devel \
  openssl-devel gtk3-devel libsoup3-devel
sudo dnf group install -y "c-development"   # gcc/g++/make/pkg-config
cargo install tauri-cli --version "^2" --locked

cd apps/web
cargo tauri dev      # or: pnpm dlx @tauri-apps/cli@2 dev
cargo tauri build    # bundles AppImage + .deb (and platform formats elsewhere)
```

Icons are generated from `public/icon-512.png`; regenerate all platforms with
`pnpm dlx @tauri-apps/cli@2 icon public/icon-512.png`.

The CSP only allows loopback bridge URLs (`127.0.0.1`/`localhost`). If you run the bridge on
another host, widen `app.security.csp.connect-src` in `src-tauri/tauri.conf.json`.

---

## Screenshots

With the dev server running, `?demo=1` plus `&screen=chats|map|nodes|contacts|settings` gives a
fully populated screen, and `&thread=anne|familien|…` opens a conversation directly. Example with
headless Firefox:

```bash
pnpm --dir apps/web dev &
firefox --headless --window-size=1440,900 \
  --screenshot=/tmp/openmesh-chats.png "http://127.0.0.1:5173/?demo=1"
firefox --headless --window-size=1440,900 \
  --screenshot=/tmp/openmesh-map.png "http://127.0.0.1:5173/?demo=1&screen=map"
firefox --headless --window-size=390,844 \
  --screenshot=/tmp/openmesh-mobile.png "http://127.0.0.1:5173/?demo=1"
```

---

## Known gaps

- No PWA service worker yet (manifest + install icons are in place); the app works offline only
  while the page is loaded and the bridge is loopback.
- No camera/QR scanner in the browser client — links are pasted and QR codes are shown; the
  Android app owns scanning.
- Browser geolocation is optional; the bridge's configured position is authoritative for radio
  purposes.
- Map is a hand-drawn schematic, not a survey-accurate basemap; online OSM tiles are opt-in.
- No message search inside a thread, no group read receipts (not in the v1 API).
