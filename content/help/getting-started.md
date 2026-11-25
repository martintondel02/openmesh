# Your first mesh in 10 minutes

OpenMesh lets phones, laptops and small radios talk to each other directly — no account, no
cell tower, no internet required. This page walks you from "I just installed it" to "I sent my
first encrypted message" in about ten minutes. You do not need to understand radios to finish it.

## What it is

OpenMesh is two things working together:

* a small program called **openmesh-bridge** that runs on your computer (or on a little radio box)
  and does the networking;
* an app — the web client at `http://127.0.0.1:8787` in your browser — where you read and write
  messages.

The bridge holds your identity and your messages. The app is just a window into it, so you can
close the browser and your mesh keeps running.

## Why it matters

Most messaging apps need a company's server in the middle. OpenMesh puts the network in your
hands: messages are sealed on your device and relayed by radios that cannot read them. That also
means **nobody can reset your password or recover your identity for you** — the short setup below
is what keeps you safe.

## Quick start

### 1. Install (2 minutes)

On Linux or macOS, open a terminal and run the installer:

```sh
curl -fsSL https://github.com/martintondel02/openmesh/releases/latest/download/install.sh | sh
```

On Windows, download the release archive from the [OpenMesh releases page](https://github.com/martintondel02/openmesh/releases)
and follow the Windows notes in the Install OpenMesh topic. If you prefer to build it yourself,
the repository README has a source-build section.

### 2. Try it without any hardware (2 minutes)

The fastest way to see OpenMesh is **demo mode**, which creates a virtual mesh on your computer:

```sh
openmesh-bridge --demo
```

Leave that terminal running and open <http://127.0.0.1:8787> in your browser. You will see seeded
conversations, a map, nodes and the Help Center (this page lives there too). Demo mode is safe:
it cannot reach real radios, and nothing it creates is permanent.

### 3. Secure your node (3 minutes)

In the app, open **Get started** (or the setup wizard on first run) and press **Secure my node**.
That one button:

* confirms your identity was created;
* picks a radio region for your country;
* applies safe defaults (private network, position sharing off, sensible announce timing);
* creates a private channel and gives you an invite to share.

If you are using demo mode, you can still walk the wizard — it exercises the same flow you will
use with real radios.

### 4. Invite someone and say hello (2 minutes)

Copy the **channel invite** from the setup screen (it looks like
`openmesh://channel?name=…`) and send it to a friend over any channel you already trust — chat,
email, a note. When they import it in their app, you are on the same channel.

Type a message in the chat box and press send. A single tick means "handed to the mesh", a double
tick means "delivered". In demo mode the bridge simulates delivery so you can see both states.

### 5. Optional: add a real radio

When you are ready, plug a supported LoRa radio into USB (or pair it over Bluetooth), then restart
the bridge without `--demo`. The app's Nodes screen will show the radio as connected. The Radio
and region topic explains which settings to pick.

## Advanced notes

* **What "secure my node" changes** — the preset sets region/profile for your country, disables
  store-and-forward on a plain client, turns position sharing off, sets the announce interval to
  30 minutes, and creates a private channel. It never changes your identity and never enables
  open/backbone mode. You can see and change every value later in Settings.
* **Where your data lives** — by default `~/.local/share/openmesh` (Linux/macOS) contains
  `identity.key`, the message database and attachments; the bridge API token is at
  `~/.config/openmesh/bridge-token`. Back up those files (see the Security and privacy topic).
* **Demo vs real** — `--demo` uses an in-process virtual mesh; `--sim` is the same transport with
  an empty state; neither touches hardware. Remove `--demo` to use UDP on your LAN or a connected
  radio. Use `--serve-web <dir>` if the web client is not installed in the default location.
* **Command-line people** — the browser UI is optional. `openmesh-cli` can send, listen, announce
  and show nodes, and `GET /api/status` returns the same data the UI uses.
* **Beginner vs advanced mode** — the app starts in beginner mode (fewer dials, guided setup).
  Switch to advanced in Settings or the top bar when you want airtime maths, raw config, backbone
  controls and agent access. Switching modes never changes a setting.

## Troubleshooting

| Symptom | Try this |
| --- | --- |
| `openmesh-bridge: command not found` | The installer put it in `~/.local/bin`; add that to your `PATH` or re-open the terminal. |
| Browser says "connection refused" | The bridge is not running, or it is listening elsewhere. Check the terminal output and `curl http://127.0.0.1:8787/api/health`. |
| The page loads but asks for a token | This is the bridge API token. Copy it from `~/.config/openmesh/bridge-token` into Settings → Bridge. |
| "Secure my node" is missing | You may be on an old release or in advanced mode. Update the bridge and check Settings. |
| Message stays on one tick | The recipient may be offline. OpenMesh can store messages at a repeater for later delivery; see Messages, delivery and offline mail. |
| You cannot invite anyone yet | Create or join a channel first, then invite from the channel's share screen. |

## Related topics

[Install OpenMesh](install.md) · [Build a tiny mesh](first-mesh.md) ·
[Radios, regions and airtime](radio-and-region.md) · [Security and privacy](security.md) ·
[Troubleshooting](troubleshooting.md)
