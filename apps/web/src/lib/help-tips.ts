/** Central help-tip map keyed by `helpKey` (docs/design/ux-help-system.md §4.1). */
import type { HelpTipContent } from '../types/help';

export const HELP_TIPS: Record<string, HelpTipContent> = {
  /* ---------------- modes & onboarding ---------------- */
  'mode.toggle': {
    what: 'Beginner keeps the safe basics in front and hides expert knobs; Advanced shows every value.',
    why: 'Changing mode never changes your settings. It only changes how much is shown.',
    topic: 'getting-started',
  },
  'onboarding.start': {
    what: 'A three-screen guided setup that names your node, picks a legal radio region and creates a private channel.',
    why: 'It applies secure defaults so you can send a first message without reading a manual. Takes about two minutes.',
    topic: 'getting-started',
  },
  'onboarding.role': {
    what: 'Your role tells OpenMesh how this device will be used: phone, fixed relay, internet gateway or battery tracker.',
    why: 'The role sets radio speed, announcing frequency and power-saving defaults that suit that job.',
    topic: 'nodes-telemetry',
    term: 'Node',
  },
  'onboarding.country': {
    what: 'Your country decides which radio band is legal here.',
    why: 'Picking another country can be illegal and will make you hard to hear. We only store the country code.',
    topic: 'radio-and-region',
    term: 'Region',
  },
  'onboarding.name': {
    what: 'The friendly name other people see for this node.',
    why: 'Names are public on the mesh. Use a nickname, not your full legal name.',
    topic: 'contacts-identity',
  },
  'onboarding.privacy': {
    what: 'Keep my location private hides your position unless you deliberately share it.',
    why: 'Position history can reveal home and work patterns. Private is the safer default.',
    topic: 'map-and-position',
  },
  'onboarding.secure': {
    what: 'One button applies token auth, a loopback-only API, a private channel and conservative radio defaults.',
    why: 'These are the choices that keep a fresh node off the public internet and inside the duty cycle.',
    topic: 'security',
  },
  'onboarding.checklist': {
    what: 'A short checklist of the things a healthy node needs.',
    why: 'It is safe to close and come back; unfinished items stay listed on Chats until done.',
    topic: 'getting-started',
  },
  'onboarding.invite': {
    what: 'A channel invite is a link containing a secret channel key.',
    why: 'Anyone with the link can join, so share it only with the people you mean to include.',
    topic: 'channels',
    term: 'Invite',
  },

  /* ---------------- settings: node ---------------- */
  'settings.node.name': {
    what: 'The display name other nodes see.',
    why: 'It is public on the mesh; a nickname keeps personal information private.',
    topic: 'contacts-identity',
    term: 'Node ID',
  },
  'settings.node.class': {
    what: 'How this node behaves: phone, tracker, repeater or gateway.',
    why: 'The class changes power and airtime defaults so the radio is not used more than needed.',
    topic: 'nodes-telemetry',
    term: 'Node',
  },
  'settings.radio.region': {
    what: 'The legal radio band for your country.',
    why: 'The wrong band is illegal and unreliable. Changing it restarts the radio.',
    topic: 'radio-and-region',
    term: 'Region',
  },
  'settings.radio.profile': {
    what: 'A speed-versus-range preset for the radio.',
    why: 'Longer range is slower and costs more airtime per message. Most people keep the default.',
    topic: 'radio-and-region',
    term: 'Radio profile',
  },
  'settings.radio.txpower': {
    what: 'How strongly the radio transmits, in dBm.',
    why: 'Higher power reaches further but drains batteries and can break duty-cycle rules.',
    topic: 'radio-and-region',
  },
  'settings.announce.interval': {
    what: 'How often your node broadcasts “I am here”.',
    why: 'Frequent announces make you easier to find but spend airtime and battery.',
    topic: 'nodes-telemetry',
    term: 'Announce',
  },
  'settings.auto.announce': {
    what: 'Let the bridge announce after reboot or long silence.',
    why: 'It helps neighbours rediscover the node without manual steps.',
    topic: 'nodes-telemetry',
  },
  'settings.position.sharing': {
    what: 'Whether your position is shared with contacts or channels.',
    why: 'Shared positions can reveal where you live. Private keeps it on-device until you share once.',
    topic: 'map-and-position',
    term: 'Position accuracy',
  },
  'settings.store.forward': {
    what: 'Keep encrypted messages for people who are offline and deliver them later.',
    why: 'It makes chatting feel reliable, at the cost of a little storage and radio time.',
    topic: 'messaging',
    term: 'Store-and-forward',
  },
  'settings.identity.public': {
    what: 'Your node’s public key and short node id.',
    why: 'Others verify your messages with the public key; the private half never leaves the device.',
    topic: 'contacts-identity',
    term: 'Public key',
  },
  'settings.identity.reset': {
    what: 'Creates a brand-new identity and abandons the old keys.',
    why: 'Contacts must re-verify you and old encrypted messages stay unreadable. Back up first.',
    topic: 'security',
    term: 'Identity',
  },
  'settings.bridge.url': {
    what: 'Where the web panel finds the local bridge.',
    why: 'Use a loopback address unless you have deliberately secured a LAN or remote connection.',
    topic: 'install',
    term: 'Bridge',
  },
  'settings.bridge.token': {
    what: 'The shared secret the browser uses to call the bridge API.',
    why: 'Anyone with it can operate your node. Keep it private and rotate it if a device is lost.',
    topic: 'security',
    term: 'Token',
  },
  'settings.bridge.demo': {
    what: 'Runs the whole app against a seeded mesh with no radio or bridge.',
    why: 'Useful for learning and screenshots; nothing is transmitted.',
    topic: 'faq',
  },
  'settings.advanced.show': {
    what: 'Reveals raw radio, identity, backbone and API controls.',
    why: 'Nothing changes until you edit a field; you can switch back at any time.',
    topic: 'getting-started',
  },

  /* ---------------- settings: API & identity ---------------- */
  'settings.api.token': {
    what: 'The token for the local HTTP/WebSocket API.',
    why: 'Keep it out of screenshots and chats; agents should read it from the environment or token file.',
    topic: 'security',
    term: 'Token',
  },
  'settings.api.allowlist': {
    what: 'Optional list of node ids allowed to connect to your backbone.',
    why: 'It narrows access when a token leaks, but an empty list means any token holder may connect.',
    topic: 'backbone',
  },
  'settings.backup.identity': {
    what: 'Export an encrypted copy of your node identity.',
    why: 'Without a backup, a lost device means contacts must re-verify you.',
    topic: 'security',
    term: 'Identity',
  },

  /* ---------------- nodes screen ---------------- */
  'nodes.stat.known': {
    what: 'Nodes heard recently, including phones, repeaters and gateways.',
    why: 'A sudden drop can mean the radio is disconnected or the antenna changed.',
    topic: 'nodes-telemetry',
    term: 'Node',
  },
  'nodes.stat.positioned': {
    what: 'Nodes that have shared a position recently.',
    why: 'Only nodes that opted in appear on the map; zero is normal on a privacy-first mesh.',
    topic: 'map-and-position',
    term: 'Position accuracy',
  },
  'nodes.stat.infrastructure': {
    what: 'Repeaters and gateways that keep the mesh connected.',
    why: 'More infrastructure usually means more range and fewer dead spots.',
    topic: 'nodes-telemetry',
    term: 'Repeater',
  },
  'nodes.stat.contacts': {
    what: 'Nodes you have saved and verified as contacts.',
    why: 'Contacts get verified identity and direct messages; unknown nodes are still visible in the node list.',
    topic: 'contacts-identity',
    term: 'Contact',
  },
  'nodes.detail.signal': {
    what: 'How strongly this node is heard, plus signal-to-noise quality.',
    why: 'Signal drops as distance or obstacles increase; noisy signal means slower, less reliable messages.',
    topic: 'nodes-telemetry',
    term: 'Signal strength',
  },
  'nodes.detail.hops': {
    what: 'How many relays a packet used to reach you.',
    why: 'More hops means more reach but more delay; zero hops is a direct radio link.',
    topic: 'first-mesh',
    term: 'Hop',
  },
  'nodes.detail.battery': {
    what: 'The battery voltage a node last reported.',
    why: 'Trackers and repeaters may sleep; “not reported” is normal between updates.',
    topic: 'nodes-telemetry',
    term: 'Telemetry',
  },
  'nodes.action.contact': {
    what: 'Save this node as a verified contact.',
    why: 'Verification protects you from someone impersonating the node later.',
    topic: 'contacts-identity',
    term: 'Contact',
  },
  'nodes.action.message': {
    what: 'Start an end-to-end encrypted direct message.',
    why: 'Direct messages travel only to the selected node, unlike channel messages.',
    topic: 'messaging',
    term: 'End-to-end encryption',
  },

  /* ---------------- chats ---------------- */
  'chats.composer.airtime': {
    what: 'The tiny bar estimates how much radio time your draft will use.',
    why: 'Airtime is shared with everyone nearby; long messages and images slow the whole mesh.',
    topic: 'radio-and-region',
    term: 'Airtime',
  },
  'chats.composer.encryption': {
    what: 'Shows whether this conversation is end-to-end encrypted.',
    why: 'Direct messages and channels are encrypted; a relay sees only ciphertext.',
    topic: 'security',
    term: 'End-to-end encryption',
  },
  'chats.attachment.image': {
    what: 'Send a photo, compressed to fit the radio budget.',
    why: 'Images can take many seconds of airtime and may queue behind text; send small, send few.',
    topic: 'media',
  },
  'chats.voice.note': {
    what: 'Record and compress a voice message before sending.',
    why: 'Audio is expensive over radio; short clips arrive faster and cost less airtime.',
    topic: 'media',
  },
  'chats.thread.delivery': {
    what: 'One tick means sent, two means delivered over the mesh.',
    why: 'A queued message is normal when the recipient is out of range; it retries automatically.',
    topic: 'messaging',
    term: 'Offline mail',
  },
  'chats.invite.copy': {
    what: 'Share this channel link with someone you trust.',
    why: 'The link contains the channel key, so treat it like a password.',
    topic: 'channels',
    term: 'Invite',
  },

  /* ---------------- map ---------------- */
  'map.offline': {
    what: 'The bundled map works with no internet and no tiles.',
    why: 'Mesh positions still render because the basemap ships with the app.',
    topic: 'map-and-position',
  },
  'map.share.position': {
    what: 'Share where you are once, or stop sharing.',
    why: 'Position is precise; share deliberately and turn it off when done.',
    topic: 'map-and-position',
    term: 'Position accuracy',
  },
  'map.accuracy': {
    what: 'The circle shows how precisely a reported position is known.',
    why: 'A large circle means the node only knows roughly where it is; do not treat it as an address.',
    topic: 'map-and-position',
    term: 'Position accuracy',
  },
  'map.history': {
    what: 'Recent position points from nodes already heard.',
    why: 'History helps find a lost tracker; it is stored locally and can be cleared.',
    topic: 'map-and-position',
  },

  /* ---------------- contacts ---------------- */
  'contacts.invite': {
    what: 'Add someone by scanning their QR code or pasting their invite link.',
    why: 'The invite proves their identity so you are not talking to an impersonator.',
    topic: 'contacts-identity',
    term: 'Invite',
  },
  'contacts.verify': {
    what: 'Compare a short safety code out loud with your contact.',
    why: 'Verification closes the gap where someone could substitute their own key.',
    topic: 'contacts-identity',
    term: 'Identity',
  },

  /* ---------------- backbone ---------------- */
  'backbone.enabled': {
    what: 'Turns the always-on relay listener on or off.',
    why: 'Off closes the public TCP port without deleting your settings.',
    topic: 'backbone',
    term: 'Backbone',
  },
  'backbone.bind': {
    what: 'Which network interface the relay listens on.',
    why: '0.0.0.0 accepts internet peers; 127.0.0.1 keeps it local and is safer for testing.',
    topic: 'backbone',
  },
  'backbone.port': {
    what: 'The TCP port peers connect to.',
    why: '9443 is the default; changing it means gateway invites must use the new port.',
    topic: 'backbone',
  },
  'backbone.maxPeers': {
    what: 'The most peer connections the relay will hold.',
    why: 'More peers means more fan-out and bandwidth. Small home backbones need very few.',
    topic: 'backbone',
    term: 'Peer',
  },
  'backbone.name': {
    what: 'The name this backbone shows in invites and peer lists.',
    why: 'It is public to peers; a nickname avoids leaking personal details.',
    topic: 'backbone',
  },
  'backbone.publicHost': {
    what: 'The hostname or IP gateways use to reach this backbone.',
    why: 'It is baked into the connection string. Leave blank to use the detected address.',
    topic: 'backbone',
  },
  'backbone.ping': {
    what: 'How often the relay pings peers to measure round-trip time.',
    why: 'Faster pings show trouble sooner but add a little traffic; 30 s is a good default.',
    topic: 'backbone',
  },
  'backbone.storeForward': {
    what: 'Hold encrypted envelopes for nodes that are temporarily offline.',
    why: 'Improves delivery for moving gateways at the cost of a little memory and traffic.',
    topic: 'messaging',
    term: 'Store-and-forward',
  },
  'backbone.auth': {
    what: 'How peers prove they may connect: a shared token or open access.',
    why: 'Open mode accepts any peer that finds the port. Only use it on a trusted private network.',
    topic: 'backbone',
    term: 'Token',
  },
  'backbone.allowlist': {
    what: 'An optional list of node ids allowed to connect.',
    why: 'Narrow access if a token leaks. Empty means any token holder may connect.',
    topic: 'backbone',
  },
  'backbone.token': {
    what: 'The shared secret peers use to join this backbone.',
    why: 'Anyone with it can relay through you. Rotate it if a gateway is lost or removed.',
    topic: 'security',
    term: 'Token',
  },
  'backbone.rotate': {
    what: 'Creates a new token and invalidates the old one immediately.',
    why: 'Connected peers stay until they reconnect, so rotate also offers to disconnect them.',
    topic: 'backbone',
    term: 'Token',
  },
  'backbone.connection': {
    what: 'A ready-to-paste link that lets another bridge join this backbone.',
    why: 'It contains the token; share it only with the machine that should join.',
    topic: 'backbone',
    term: 'Invite',
  },
  'backbone.qr': {
    what: 'A scannable copy of the connection string.',
    why: 'Handy for phones, but the QR is just as secret as the string it carries.',
    topic: 'backbone',
  },
  'backbone.stat.peers': {
    what: 'Authenticated peers connected right now.',
    why: 'This is the real measure that the relay is reachable from outside.',
    topic: 'backbone',
    term: 'Peer',
  },
  'backbone.stat.frames': {
    what: 'Frames sent and received since the relay started.',
    why: 'Steady values mean traffic is flowing; one flat direction can mean a stale peer.',
    topic: 'backbone',
  },
  'backbone.stat.throughput': {
    what: 'Current inbound and outbound relay rate.',
    why: 'A sudden sustained spike is worth investigating; idle backbones stay near zero.',
    topic: 'backbone',
  },
  'backbone.stat.bytes': {
    what: 'Total encrypted bytes relayed since start.',
    why: 'Useful for data caps; OMP payloads remain end-to-end encrypted.',
    topic: 'backbone',
  },
  'backbone.stat.storeForward': {
    what: 'Encrypted envelopes currently held for offline nodes.',
    why: 'A large queue can mean a gateway has been offline for a while.',
    topic: 'messaging',
    term: 'Store-and-forward',
  },
  'backbone.stat.authFailures': {
    what: 'Rejected handshakes since start.',
    why: 'A few are normal scan noise; a growing number can mean a wrong token or an attack.',
    topic: 'security',
  },
  'backbone.peer.kick': {
    what: 'Disconnect this peer until it reconnects with a valid token.',
    why: 'Useful for removing a lost gateway; rotate the token if you suspect it is compromised.',
    topic: 'backbone',
    term: 'Peer',
  },
  'backbone.setup': {
    what: 'A numbered walkthrough for installing the relay on a server or Raspberry Pi.',
    why: 'It includes firewall rules, a systemd service and the gateway join command.',
    topic: 'backbone',
  },

  /* ---------------- agents ---------------- */
  'agents.intro': {
    what: 'Connect an AI assistant to your node through MCP, a typed tool protocol.',
    why: 'The agent can read mesh state and, if you allow it, make changes that appear in the activity log.',
    topic: 'agents',
    term: 'MCP',
  },
  'agents.mcp.claude': {
    what: 'Configuration for Claude Desktop or Claude Code.',
    why: 'It launches the local bridge in MCP mode; the token comes from the environment, never the prompt.',
    topic: 'agents',
    term: 'MCP',
  },
  'agents.mcp.cursor': {
    what: 'Configuration for Cursor’s MCP servers list.',
    why: 'The same stdio server works in Cursor; keep read-only on until you trust the workflow.',
    topic: 'agents',
    term: 'MCP',
  },
  'agents.mcp.dsh': {
    what: 'Configuration for DeepSeek Harness (DSH).',
    why: 'DSH reads the same stdio MCP server; `openmesh-cli agent config --client dsh` prints the exact file.',
    topic: 'agents',
    term: 'MCP',
  },
  'agents.mcp.generic': {
    what: 'A minimal snippet for any MCP client that can launch a stdio server.',
    why: 'If your client asks for a command, use `openmesh-bridge mcp` with the token in the environment.',
    topic: 'agents',
    term: 'MCP',
  },
  'agents.readonly': {
    what: 'Read-only mode exposes only tools that cannot change your node.',
    why: 'It is the safest way to try an agent. Turn it off only when you understand what it will change.',
    topic: 'agents',
    term: 'Read-only mode',
  },
  'agents.prompt': {
    what: 'A ready-made system prompt telling the agent what your node is and what it must not do.',
    why: 'It includes safety rules and a request to confirm destructive actions with you first.',
    topic: 'agents',
    term: 'Agent',
  },
  'agents.activity': {
    what: 'Every write action taken through the bridge, with who did it and whether it worked.',
    why: 'Reads are not logged. Message bodies, tokens and channel keys never appear here.',
    topic: 'agents',
    term: 'Activity log',
  },
  'agents.revoke': {
    what: 'Rotates the bridge API token so old clients and agents stop working.',
    why: 'Do this if a device is lost or an agent should no longer touch the node. Reconnect good clients afterwards.',
    topic: 'security',
    term: 'Token',
  },
  'agents.security': {
    what: 'The rules that keep an agent useful without giving away control.',
    why: 'Keep backups, prefer read-only, confirm token rotations and never paste secrets into a chat.',
    topic: 'security',
  },

  /* ---------------- generic ---------------- */
  'common.help': {
    what: 'Opens the in-app handbook, offline too.',
    why: 'Every screen has a help link so you never need to leave the app to understand a choice.',
    topic: 'getting-started',
  },
};

export function helpTipContent(helpKey: string): HelpTipContent | null {
  return HELP_TIPS[helpKey] ?? null;
}

export function missingHelpKeys(keys: string[]): string[] {
  return keys.filter((key) => !HELP_TIPS[key]);
}
