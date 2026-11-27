# UX & Help System — v0.3.0 specification

**Status:** normative for v0.3.0 · **Applies to:** `apps/web`, `crates/openmesh-bridge` (help +
setup APIs), `content/help/` (content), `docs/` (long-form guides)
**Related:** [AI-NATIVE.md](../architecture/AI-NATIVE.md), [WEBSOCKET-API.md](../architecture/WEBSOCKET-API.md)

Beginner users must be able to go from "box on desk" to a secure, working mesh without reading a
manual. Advanced users must be able to see and change everything, and understand the cost of each
choice. Both must be served by **the same help content**, so the docs, the in-app help center and
AI agents never disagree.

---

## 1. Principles

1. **Progressive disclosure, never a wall.** Beginner mode shows the smallest set of choices that
   produces a correct, secure setup. Everything else is one "Show advanced" / mode switch away —
   nothing is removed or hidden behind docs.
2. **Explain the trade-off, not just the field.** Every control that costs airtime, battery or
   privacy states the cost in one short sentence (`HelpTip`), with a "Learn more" link.
3. **Secure defaults are the fast path.** A beginner clicking "Secure my node" gets: token auth,
   loopback API, private channel with generated key, position sharing off, sane announce interval,
   no open backbone mode.
4. **One source of truth for help.** `content/help/*.md` + `index.json` feed the web Help Center,
   the bridge help API and the MCP/agent prompt. Long-form docs link back to the same topics.
5. **No dead ends.** Every empty state, error and onboarding step offers a next action and a help
   link (topic + anchor).
6. **Accessibility is part of "great UX".** Every help tip is keyboard reachable, has an
   accessible name and never the only carrier of required information.

---

## 2. Modes

| | Beginner (default for new installs) | Advanced |
| --- | --- | --- |
| Navigation | Chats, Map, Nodes, Contacts, **Help** | + Backbone, Agents, Activity |
| Settings | Guided sections: identity, radio (region/profile only), privacy, "Secure my node" checklist | Full radio/announce/TTL/power, raw identity, backbone allowlist/token rotation, airtime math, API/MCP |
| Copy | Plain language, no jargon without a glossary popover | Precise, includes protocol terms and numbers |
| Onboarding | 3-step wizard + persistent "Get started" checklist | Dismissible; same wizard skippable |
| Danger zone | Typed confirmations with explanations | Same, plus bulk/raw operations |

* Persisted as `localStorage openmesh.mode = "beginner" | "advanced"` + bridge config field
  `features.ui_mode` (synced when connected; local wins for layout).
* A visible toggle in the top bar (`Beginner · Advanced`) and in Settings; switching to advanced
  never changes any setting by itself.

---

## 3. Onboarding flow (beginner)

`POST /api/setup/quick` does the work; the UI collects three screens:

1. **What is this box?** role: `client` (phone/laptop), `repeater` (solar/roof), `gateway`
   (bridge to internet), `tracker`. Pre-selected from hardware/radio detection when possible.
2. **Where are you?** country selector (drives region) + auto-detected public host. Node name
   with a friendly suggestion. Toggle "keep my location private" (default ON).
3. **Secure my node** — one button that applies the quick-setup preset, then shows:
   * ✅ checklist (identity created, region set, channel created, secure defaults, radio detected)
   * the channel invite (copy + QR) and "Invite someone" flow
   * next steps cards: send a message, add a contact, run a backbone, connect an agent
   * "You can change anything later in Settings" + mode switch hint.

`GET /api/setup/state` powers resume/checklist rendering. `POST /api/setup/quick` is idempotent
and returns exactly what it changed (§5).

---

## 4. Help surfaces (required in beginner *and* advanced)

### 4.1 `HelpTip` (inline)
Small `?` button next to a label, stat or action. Popover contains:
* one-sentence **what** and one-sentence **why/cost**
* `topic` link → opens Help Center at that topic (§4.4)
* keyboard: focusable, Enter/Space opens, Escape closes, focus returns; `aria-describedby`;
  never a hover-only tooltip for essential info.
* Implementation detail: one component, content from a central help map keyed by `helpKey`.
  **Acceptance: every field/control/stat card in Chats, Map, Nodes, Contacts, Settings, Backbone
  and Agents has a `HelpTip` or an inline explanation. An ESLint-style unit test asserts the
  settings/backbone control registry has no missing `helpKey`.**

### 4.2 `Term` (glossary popover)
Dotted-underlined term (LoRa, airtime, node id, announce, PSK, store-and-forward, OBB/1, …) with a
one-line definition + glossary link. ~30 terms, defined once in `src/lib/glossary.ts`, reused in
tooltips, onboarding and empty states.

### 4.3 Empty states & errors
Every screen's empty state: icon, plain-language title, 1–2 sentence explanation, primary action,
secondary "How does this work?" link. Errors from the bridge/API surface as actionable messages:
what happened, what to try next, help link, copy diagnostics button.

### 4.4 Help Center (`/help`)
* Search box (fuzzy over title/summary/body), audience filter **Beginner / Advanced**, topic cards
  with reading time, related topics.
* Reader: rendered markdown, sticky table of contents, "Was this useful?" (local-only), previous/
  next, deep links `/help/:slug#anchor`, deep link from any `HelpTip`.
* `?help=topic` and `/help/topic` routes; opening help should not lose an in-progress message
  (composer draft preserved).
* Content source: `GET /api/help` + `GET /api/help/:slug` when connected; bundled copy
  (`apps/web/public/help`, produced by the prebuild copy script from `content/help/`) in demo mode
  and offline. Search index built client-side from `index.json` + topic bodies.

### 4.5 Contextual guidance
* First-run "Get started" checklist (dismissible, reappears until complete or dismissed):
  name set · secure defaults applied · radio connected · channel joined · contact added · invite
  shared.
* Advanced checklist instead: backbone configured · agent connected · airtime budget reviewed ·
  identity backed up.
* Inline banners for: demo mode, insecure config (open backbone/LAN API), duty-cycle risk, no
  radio detected, token missing, outdated bridge, agent read-only mode.

---

## 5. Bridge APIs (normative summary; full detail in WEBSOCKET-API.md §9)

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/api/setup/state` | First-run/checklist state: `{first_run, checks:{identity,radio,api_token,channel,backbone,agent}, recommended:{region,profile,role}, counts}` |
| POST | `/api/setup/quick` | Apply a secure preset: `{role, country, node_name?, privacy:"private"|"shared", create_channel:true}` → `{applied:{...}, channel:{channel_id,name,invite_url,psk_once}, checklist:[...], next_steps:[{title,detail,help_topic}]}` |
| GET | `/api/setup/recommend?country=NO&role=client` | Region/profile/duty-cycle recommendation used by the wizard |
| GET | `/api/help` | `{topics:[{slug,title,summary,audience,minutes,related}]}` (from `content/help`) |
| GET | `/api/help/:slug` | Raw markdown + title/audience metadata |
| GET | `/api/activity?limit=100` | Recent write actions for transparency (see AI-NATIVE.md) |
| GET | `/api/openapi.json` | Machine-readable REST API description (see AI-NATIVE.md) |

Quick-setup presets (server-owned, so CLI/agents/UI behave identically):

| role | region | profile | store_forward | position | announce | extras |
| --- | --- | --- | --- | --- | --- | --- |
| client | by country | `MEDIUM_FAST` | off | private | 30 min | create private channel invite |
| tracker | by country | `MEDIUM_FAST` | off | shared only with channel | 60 min | battery-safe caps |
| repeater | by country | `LONG_FAST` | **on** | off | 60 min | airtime cap 1 % warnings |
| gateway | by country | `LONG_FAST` | **on** | off | 60 min | backbone suggestion |

Region by country: EU/UK/NO/CH/IS → `EU868`; US/CA/MX → `US915`; AU/NZ → `AU915`; IN → `IN865`;
KR → `KR920`; SG/MY/TH/ID/PH/VN → `AS923`; otherwise `EU868` with a visible warning.

---

## 6. Copy & terminology rules

* Second person, active voice, sentence case. No "simply", "just", "obviously".
* Define jargon on first use (glossary popover), then use it normally.
* Numbers always carry units and a comparison where useful ("~18 s of airtime — about the same as
  six text messages").
* Never blame the user in errors. "The radio isn't answering" not "Invalid device".
* Beginner copy never mentions: SF, bandwidth, PSK bytes, TTL, MTU, OBB, HMAC. Advanced copy always
  shows the exact values.

### 6.1 Radio link states (normative)

Every surface that talks about the radio link (Android radio screen, chat banner, settings row,
nodes header, foreground-service notification; web settings card, connection panel, banners,
composer) uses the same five states and never hides the difference:

| state | meaning | required treatment |
| --- | --- | --- |
| `connected` | radio attached and the companion handshake finished | positive status, no warning |
| `virtual` | no hardware radio: simulator, LAN or backbone transport | neutral "virtual radio", not an error |
| `connecting` | link is being established | progress copy, spinner **plus** text, cancellable |
| `reconnecting` | link was lost and is retried automatically | "the bridge/app keeps retrying", queue copy, show attempt count when known |
| `disconnected` | no link and no retry in progress | actionable cable/power guidance |

Rules: describe retrying as retrying (never "broken"); state failures persist until the next
attempt instead of disappearing as a snackbar; every state has visible text (spinner or colour is
never the only indicator); a ready link that stops answering keep-alives shows "not answering" and a
reconnect action rather than staying silently green; messages sent while not connected are shown as
**queued** (sent when the link is ready), never as sent.

---

## 7. Acceptance criteria (v0.3.0)

1. Fresh install → beginner wizard → secure mesh with channel + invite in under 3 minutes,
   zero required reading; every step resumable.
2. Beginner mode exposes no setting without a HelpTip/explanation; advanced mode exposes every
   documented field and the airtime/duty math.
3. Help Center works offline/demo (bundled) and connected, with search + audience filter + deep
   links; 100 % of `content/help` topics render.
4. All glossary terms resolve; no broken `HelpTip` topic links (unit test).
5. Empty states, error toasts and onboarding all offer a next action + help link.
6. Mobile (390×844) and keyboard-only flows pass; no console errors; screenshots captured for
   README/release.
7. Tests: new component/store tests covering mode switching, help search, onboarding state
   machine and quick-setup payload handling; existing 169 tests stay green.
