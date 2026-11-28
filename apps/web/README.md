# OpenMesh web client

The reference client for OpenMesh (v0.3.0): a dense, dark, mobile-responsive React app for chatting,
sharing images and sending voice notes over a LoRa mesh — plus a map of the mesh, node
diagnostics, contact verification, radio settings, a beginner/advanced guided experience, an
offline-capable Help Center and an AI-agent (MCP) page.

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
[Mobile thread](docs/screenshots/mobile-thread.png) ·
[**Backbone panel**](docs/screenshots/backbone.png) ·
[Backbone setup wizard](docs/screenshots/backbone-wizard.png) ·
[Backbone on mobile](docs/screenshots/backbone-mobile.png) ·
[**Onboarding**](docs/screenshots/onboarding.png) ·
[Secure setup result](docs/screenshots/onboarding-success.png) ·
[**Help Center**](docs/screenshots/help-center.png) ·
[**Agents**](docs/screenshots/agents.png) ·
[Beginner settings](docs/screenshots/settings-beginner.png) ·
[Help on mobile](docs/screenshots/mobile-help.png)

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
open "http://127.0.0.1:5173/?demo=1&screen=backbone"      # or the /backbone route
open "http://127.0.0.1:5173/backbone?demo=1"
open "http://127.0.0.1:5173/?demo=1&onboarding=1"         # guided setup wizard
open "http://127.0.0.1:5173/?demo=1&screen=agents&mode=advanced"
open "http://127.0.0.1:5173/?demo=1&screen=settings&mode=beginner"
open "http://127.0.0.1:5173/help/getting-started?demo=1"  # Help Center deep link

pnpm --dir apps/web test         # vitest (30 suites / 379 unit tests)
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

### Help corpus in the web build

`pnpm dev` and `pnpm build` run `scripts/copy-help.mjs` first, which copies `../../content/help`
(17 topics + index) into the gitignored `apps/web/public/help/`. Demo/offline mode fetches that
bundle; a connected bridge serves the same content from `GET /api/help` (with a bundled fallback).
Never edit `public/help/` by hand — edit `content/help/` and let the build copy.

### Bundled AVIF encoder (web build)

`pnpm dev` and `pnpm build` also run `scripts/build-image-wasm.mjs`: if
`public/image/openmesh_image.wasm` is missing it reuses
`crates/openmesh-image-wasm/build-wasm.sh`, otherwise it skips the Rust build. The artifact is
gitignored (~1.3 MB raw / ~406 KB gzip-9) and CI/release build it explicitly before the web build.
On a machine without Rust or the `wasm32-unknown-unknown` target the script warns and lets the
web build continue; the client then lazily falls back to canvas WebP/JPEG and keeps the real MIME,
so AVIF is never mislabelled. The WASM is fetched in a Worker only on the first image encode, not
at app startup.

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

### Backbone demo (v0.2)

`/?demo=1&screen=backbone` (or the route `/backbone?demo=1`) opens a seeded backbone:
`oslo-backbone` running on `0.0.0.0:9443` with a public host, three peers (`hytta-gateway`,
`lofoten-gateway`, `tromso-relay`), ticking frame/byte counters, live throughput sparkline,
12 stored envelopes and 17 rejected handshakes so every statistic tells a story.

Screenshot flow (works with no backend):

1. Open `http://127.0.0.1:4173/?demo=1&screen=backbone` (wait ~1 s for the demo mesh).
2. The panel, peer table, connection QR, stats and controls are visible immediately — no clicks.
3. **Setup wizard:** click **Set up your backbone** → numbered steps, firewall tabs, systemd unit,
   install section, panel URL (see `docs/screenshots/backbone-wizard.png`).
4. **Token rotation:** click **Rotate** → confirm → the new 32-byte base64url token is shown exactly
   once with a "disconnect all peers" toggle; close it to see the peers drop.
5. Mobile: load the same URL at 390×844; the panel stacks with a 6-item bottom nav.

In demo mode every control works locally: toggling the listener, editing the config and saving,
kicking peers, rotating the token and the enable/disable states are simulated by the mock transport
(`src/lib/mock/transport.ts`) so the whole flow can be reviewed offline.

### Beginner mode, onboarding, Help Center and agents (v0.3)

The release adds a beginner layer on top of the same screens:

* **Beginner / Advanced mode** — top-bar toggle plus Settings; persisted as
  `localStorage.openmesh.mode` and synced to `features.ui_mode` when a bridge is connected.
  Beginner keeps plain language, safe defaults and a guided Settings screen; Backbone and Agents
  appear only when configured. Advanced reveals every radio/identity/backbone/agent control. The
  `?mode=beginner|advanced` query parameter wins for screenshots without overwriting the preference.
* **Three-screen onboarding** — `?demo=1&onboarding=1` (or first run on a live bridge) shows
  role → country/name/privacy → "Secure my node". The success screen has the ✅ checklist, channel
  invite with copy + QR ("shown once") and next-step cards. `GET/POST /api/setup/state|quick|recommend`
  are used when connected; demo mode uses the same shapes locally. The Get-started card on Chats
  resumes it and is dismissible for the session; Advanced gets an operator checklist instead.
* **Help Center at `/help`** — search, Beginner/Advanced audience filter, reading time, related
  topics, sticky "On this page", previous/next, local-only usefulness feedback and deep links
  (`/help/:slug#anchor`, `?help=slug`). The corpus is bundled for demo/offline via a predev/prebuild
  copy of `content/help/` into `public/help/` (gitignored) and fetched from `GET /api/help` when a
  bridge is connected. A `HelpTip` `?` lives on fields, stat cards and non-obvious actions; a
  35-term glossary powers dotted-underline `Term` popovers.
* **Agents page** (Advanced, or whenever an agent is configured in demo) — MCP config snippets for
  Claude/Cursor/DSH/generic with copy, beginner/advanced system prompt with copy, read-only
  explainer, activity table from `GET /api/activity` with source badges and failed actions, and
  honest revocation guidance (the panel cannot rotate the bridge token through the v0.3 API).
* **Polish** — guided empty states and "How does this work?" links on every screen, actionable
  error toasts with a help link and copy-diagnostics, insecurity/no-radio banners, composer drafts
  preserved across Help navigation, 390×844 layouts and visible focus states.

Screenshot flow (all demo, no backend):

1. `/?demo=1&onboarding=1` → Continue twice → **Secure my node** → success screen
   (`docs/screenshots/onboarding.png`, `onboarding-success.png`).
2. `/help/getting-started?demo=1` → search, audience filter, click a related topic, copy a deep
   link; the left list, reader and sticky ToC (`docs/screenshots/help-center.png`).
3. `/?demo=1&screen=agents&mode=advanced` → activity table, config snippet tabs, prompt tiers
   (`docs/screenshots/agents.png`).
4. `/?demo=1&screen=settings&mode=beginner` → guided settings + Secure-my-node checklist
   (`docs/screenshots/settings-beginner.png`); 390×844 at `mobile-help.png`.

---

## What is implemented

| Area | Details |
| --- | --- |
| **App shell** | Left nav rail (compact icon rail on md, labelled on xl), mobile bottom tab bar, context top bar with mesh stats, connection pill, node identity chip (click to copy), demo banner. |
| **Chats** | Channels + direct conversations, unread badges, last message + delivery tick, search (⌘K), filters; built-in **Public Mesh** channel auto-created/joined, pinned first with a PUBLIC badge + no-privacy warning and no leave action. Thread: day separators, grouped bubbles, reply quoting with jump-to, optimistic send, retry on failure, delivery ticks (pending/sent/delivered/read/failed), auto-scroll + jump-to-latest, sticky thread header, share-position action. |
| **Composer** | Enter to send / Shift+Enter newline, grapheme-safe text handling, live airtime estimate (`~0.7 s airtime · 1 frame`), attachment + voice buttons, drag-and-drop and clipboard-paste images, image lightbox. |
| **Received media** | Inline image thumbnails with full-size modal + Save, Codec2 voice notes decoded in-browser (Rust→WASM) with play/pause and duration from metadata or payload size, "Receiving…" progress for in-flight channel/direct attachments. |
| **Identity & display name** | "You are <name> · <short id>" card (copy id, My QR) in beginner and advanced Settings; 1–32-byte name validation with counter; Save applies the config and immediately fires an announce so peers see the new name. Announced name → pinned contact alias → `node <first8>` fallback everywhere (chats, thread headers, sender labels, nodes, map, contacts, search). |
| **Delivery & peer link** | Explicit delivery labels plus a Message details dialog (direct: delivered/read semantics; channel: "Sent to channel" with no per-recipient receipt; failed → retry). Direct-thread header quality chip (Strong/OK/Weak/Offline · dBm · hops) opening a peer & link modal with role, RSSI/SNR, last heard, contact state and your radio's own state/device/profile; the node inspector shows the same quality strip. |
| **Groups** | Create channel (generated 256-bit PSK shown once with copy + `openmesh://channel` invite link + QR), join via pasted link or name+PSK, channel invite modal (QR + share sheet), per-message Ed25519 signature badge, leave channel with confirmation. |
| **Images** | File picker / drag-drop / paste → canvas downscale, then Auto-encode quality-matched AVIF (lazy bundled WASM Worker, first image send only) and WebP (canvas), sending the strictly smaller file (WebP on a tie; JPEG fallback when WebP is unavailable; every candidate's MIME is verified, so a silent PNG fallback can never be mislabelled) with an encode loop until the target size fits the profile cap; slider from 1 KiB to the profile cap with Fastest/Balanced/Max presets; chunks, airtime, transfer time and % of the region duty-cycle budget shown before sending; progress overlay for incoming resources. |
| **Voice** | `MediaRecorder` with `audio/ogg;codecs=opus` preferred (WebM fallback), live timer + waveform from a WebAudio analyser, auto-stop at the profile voice cap, review with playback, then bubble with waveform bars, seek, playback progress and duration. |
| **Beginner/Advanced + help (v0.3)** | Mode toggle (persisted + synced to `features.ui_mode`), nav/field gating with plain-language copy, 3-screen secure onboarding wire-compatible with `/api/setup/*`, resumable Get-started checklist, Help Center with search/audience/ToC/related/deep links, central `HelpTip` + 35-term glossary, guided empty states, actionable error toasts, insecurity/no-radio banners, composer draft preservation. |
| **Agents (v0.3)** | MCP snippets (Claude/Cursor/DSH/generic), beginner/advanced system prompt copy, read-only explainer, activity table with source badges and failures, bridge-token revocation guidance, security notes; demo seeds one connected agent plus eight real-looking write actions. |
| **Backbone (v0.2)** | Control panel for an always-on OBB/1 relay: live stats cards (peers, frames, throughput sparkline, bytes, store-forward envelopes, auth failures), peer table with direction/remote/RTT/traffic and kick, live config form (enable, bind, port, max peers, auth token/open with red typed confirmation, allowlist editor, store-forward, name/public host/ping), connection string + QR with token reveal/copy, token rotation flow (new token shown once + kick-all), setup wizard from `GET /api/backbone/setup` (numbered copy-button steps, firewalld/ufw tabs, systemd unit, gateway join command, verify step), install section (Linux/macOS/Windows), panel URL hint, graceful disabled/one-click-enable state. Data over `backbone_status`/`backbone_peer` WS events + the 7 REST endpoints. |
| **Map** | MapLibre GL with a **bundled offline schematic basemap** of the Oslo area (fjord, lakes, Nordmarka, roads — no tile requests, no glyph server). Node markers coloured by role (DOM markers, hover labels), own-position pulse + accuracy circle, position-history polyline, mesh link lines, role filters, selected-node popover, fit-to-nodes, geolocation locate/share, optional online OSM raster with automatic "tiles unavailable" fallback. |
| **Nodes** | Table (desktop) / cards (mobile) with role, caps icons, hops, RSSI bars + dBm, SNR, battery, last heard; filters + search; detail drawer with 72-byte identity, contact QR, raw identity copy, position, link metrics, contact add/remove, message action. |
| **Contacts** | Contact cards with verified badge and last seen; add via invite link or 72-byte identity + node id (auto-derived); QR display; delete with confirmation; own contact QR + explicit "no camera scanner in the browser" note. |
| **Settings** | Node name/class, announce interval with duty-cycle warning, auto-announce, radio profile + region selector with computed attachment/voice caps, announce vs budget percentage, hourly media budget, TX power, position sharing, store-forward, bridge URL + token (show/hide, test, save & reconnect), live session/radios panel, demo-mode toggle, and a danger zone with two typed-confirmation actions: clear this browser's local state, or wipe the bridge (messages/contacts/channels/attachments/activity), regenerate the bridge node identity and re-create Public Mesh. |
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

228 Vitest tests cover: LoRa airtime + chunk/transfer math and protocol worked examples, image
compression planning (dimensions, quality, slider mapping, Opus sizes), formatting/Unicode/focus
helpers, `openmesh://` link parsing + identity validation, WebSocket envelope parsing + reconnect
transport (fake socket), store reducers (optimistic send, acks, merge semantics, conversations),
seeded demo-dataset invariants (including the seeded backbone), geo helpers, the offline map style,
plus the v0.2 backbone surface (connection-string build/redact/QR helpers, REST client for all seven
backbone endpoints, config normalize/merge/diff, allowlist validation, peer reducers, status pills,
setup-payload fallback, install options, panel selectors) and the v0.3 UX/AI surface: mode
persistence/nav gating/copy rules, glossary resolution, help search/audience/ToC/markdown/XSS/
deep-link helpers, 100 % of `content/help` topics rendering, help-tip + control-registry
completeness (no missing `helpKey` or broken topic link), setup presets/normalizers/checklist/
onboarding state machine, setup REST client, MCP snippets, prompt tiers, activity normalisation
and the Agents REST client.

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
│   │   ├── backbone.ts         OBB/1 connection strings, config/peer/setup helpers
│   │   ├── backbone-api.ts     REST client for the 7 backbone endpoints
│   │   ├── mode.ts             beginner/advanced mode + copy rules
│   │   ├── setup.ts            setup presets, normalisers, onboarding state machine
│   │   ├── setup-api.ts        REST client for /api/setup/*
│   │   ├── help.ts             help index/search/markdown/ToC/deep links/loaders
│   │   ├── help-tips.ts        central HelpTip content map
│   │   ├── control-registry.ts every settings/backbone/agents control → helpKey
│   │   ├── glossary.ts         35 one-line terms
│   │   ├── agents.ts           MCP snippets, prompts, activity rendering
│   │   ├── agents-api.ts       /api/activity client
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
│   ├── store/backbone-selectors.ts  panel view models (stats, pills, setup)
│   ├── components/onboarding/  3-screen wizard
│   ├── components/ui/help.tsx  HelpTip, Term, HelpLink
│   └── screens/                chats/, backbone/, settings/, HelpCenter, Agents, map, nodes, contacts
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
   emits it during simulated uploads so the progress UI is exercised. A channel image/voice arrives
   as `group_message` first and the stored attachment in a later `message_update`, so the client
   merges updates into channel messages too and only fetches bytes once `progress` reaches 1.0.
   Incoming media shows "Receiving…" until then; Codec2 voice duration is derived from the payload
   size when the message carries no `duration_ms`.
10. **`get_backbone` config reply is under-specified.** §4.5 says it returns `backbone_status` +
   `config` ("backbone section") but not how the section is carried. The client accepts a `config`
   event with an optional `backbone` object and, for the live panel, loads the full config from
   `GET /api/backbone` (which *is* specified).
11. **`backbone_token` event payload** is named in §4.5 but not specified; the client accepts
   `{token}` from either the REST rotate response (primary live path) or a `backbone_token` WS event
   (demo/fallback).
12. **`direction` semantics** in the peer table are not defined (`inbound` vs `outbound` assumed);
   unknown values are shown verbatim and sorted after inbound links.
13. **`totals` in `GET /api/backbone`** has no documented shape; the panel types the same counters as
   `backbone_status` and falls back to summing peers until the first status event arrives.
14. **Setup step `note`** may be absent/`null`; typed `string | null` and rendered conditionally.
15. **CORS:** the panel's REST calls are cross-origin when the SPA is served by Vite/preview instead
   of the bridge (`:5173` → `:8787`). The release bridge serves the built panel same-origin; for
   dev against a remote bridge, enable CORS or use a proxy. WS already uses `?token=`.
16. **`GET /api/backbone/qr.svg`** is implemented and tested in the REST client, but the panel draws
   its own QR from the connection string so invites work without a second round-trip and in demo
   mode (no bridge).
17. **`GET /api/setup/state` checks are `{ok, detail}` objects** in the live bridge rather than the
   booleans the UX summary implies. The client normalises both shapes, uses `first_run` /
   `setup_completed` for resume, and reads the extra `counts.radios` / `counts.backbone_peers`.
18. **`GET /api/setup/recommend` names privacy `position_sharing` (boolean)** while the wizard
   speaks in `privacy: private|shared`. The client maps between them, renders the `warning` banner
   for unknown countries, and shows the exact frequency only in Advanced mode.
19. **`POST /api/setup/quick` returns more than the summary lists** (`recommended`, `first_run`,
   `setup_completed`; checklist entries use `id`). The client accepts `id`/`key`, optional
   recommended/completion fields and only-changed `applied` keys. `psk_once` is treated as
   shown-once: displayed with a copy button and a warning, never persisted.
20. **`GET /api/activity` is a wrapper object** (`{entries, capacity, agent_connected,
   last_agent_at_ms, sources}`) while demo/older fixtures may send a bare array. The client accepts
   both and drives the connected-agent badge from the metadata. All bridge REST calls send
   `X-OpenMesh-Client: web` so mutations are attributed correctly.
21. **WebSocket attribution limitation:** browsers cannot set `X-OpenMesh-Client` on the WS upgrade,
   so WS-originated writes may appear as `user` instead of `web`. REST mutations are attributed.
   A bridge-side query parameter or origin-based attribution would close this.
22. **No bridge-token rotation API in v0.3.0.** AI-NATIVE.md says “one click revokes (rotate token)”,
   but only the *backbone* OBB token has `POST /api/backbone/token/rotate`. The Agents page
   deliberately shows the bridge-host steps (stop, regenerate token file, restart, re-paste) instead
   of inventing a route; the copy is checked by tests.
23. **Help API 404 bodies:** `GET /api/help/:slug` returns `{error, code, message, help_topic}` for an
   unknown slug. The loader falls back to the bundled copy; if both fail the reader shows a generic
   recoverable error with a retry/copy-link action, not the bridge `message`. Worth surfacing.
24. **SPA deep links need absolute asset URLs:** the web build uses `base: '/'` so
   `/help/getting-started` and `/backbone` load their JS/CSS; Tauri builds keep `base: './'`.
   Previously the relative base made nested routes request `/help/assets/...` and fail.

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
firefox --headless --window-size=1440,900 \
  --screenshot=/tmp/openmesh-backbone.png "http://127.0.0.1:5173/?demo=1&screen=backbone"
firefox --headless --window-size=1440,900 \
  --screenshot=/tmp/openmesh-onboarding.png "http://127.0.0.1:5173/?demo=1&onboarding=1"
firefox --headless --window-size=1440,900 \
  --screenshot=/tmp/openmesh-help.png "http://127.0.0.1:5173/help/getting-started?demo=1"
```

---

## Radio link state (v0.7.1+)

The settings radio card, the shell connection panel, the system banner and the chat composer all
describe the bridge radio link with the same five states, derived from `hello.radios[].state` and
`bridgeStatus.radio.{state,last_rx_ms,reconnect_attempts}`:

* `connected` — serial radio attached and handshaken; shows RSSI/SNR and "last frame heard … ago".
* `virtual` — simulator/LAN/backbone transport; neutral, never an error.
* `connecting` / `reconnecting` — progress copy with spinner **and** text; reconnecting says the
  bridge retries automatically and shows the attempt count when reported.
* `disconnected` — actionable cable/power guidance; messages queue until the radio is back.

Wording decisions live in `src/lib/radio-link.ts` (unit-tested) so every surface stays consistent.
The copy rules are in `docs/design/ux-help-system.md` §6.1.

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
- Backbone transport TLS is out of scope for v0.2 (front the port with Caddy/nginx/stunnel or run
  inside WireGuard/Tailscale, as BACKBONE.md §10 documents). The panel surfaces this in setup notes.
- Live Backbone REST calls need bridge CORS if the web client is not served by the bridge itself;
  the release setup serves the panel from `/backbone` on the bridge origin.
- The rotated backbone token lives only in panel memory; after a reload the connection string falls
  back to the setup payload/CLI (the API never re-returns tokens by design).
- The v0.3 bridge has no bridge-token rotation API, so the Agents page's revoke action is guidance
  (with copyable steps) rather than a button that silently fails.
- WS write attribution cannot be set from a browser WebSocket upgrade; REST calls are tagged
  `X-OpenMesh-Client: web`.
- HelpTip coverage is enforced by a control registry for Settings/Backbone/Agents; Chats, Map,
  Contacts and Nodes use inline explanations or `HelpTip`s for the non-obvious actions, but there is
  no DOM-level test asserting every button on those screens has a tip (structural registry only).
- No visual-regression tests: screenshots and keyboard/mobile flows are verified manually in
  headless Firefox (BiDi), including zero console errors in demo mode.
