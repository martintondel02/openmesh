/** One-line definitions reused by Term popovers, HelpTips, onboarding and empty states. */

export interface GlossaryEntry {
  term: string;
  short: string;
  definition: string;
  helpTopic: string;
}

export const GLOSSARY_TERMS: GlossaryEntry[] = [
  { term: 'LoRa', short: 'Long-range radio', definition: 'A low-power radio technique that carries small messages for kilometres without Wi-Fi or mobile data.', helpTopic: 'radio-and-region' },
  { term: 'Mesh', short: 'Phones and radios passing messages along', definition: 'A network where each device can relay for the others, so a message can reach further than one radio alone.', helpTopic: 'first-mesh' },
  { term: 'Node', short: 'One OpenMesh device', definition: 'A phone, laptop, repeater or gateway running OpenMesh with its own identity.', helpTopic: 'nodes-telemetry' },
  { term: 'Node ID', short: 'A short, public name for your node', definition: 'A 16-character public identifier derived from your node key. Safe to share; it is how others address you.', helpTopic: 'contacts-identity' },
  { term: 'Announce', short: 'A short “I am here” broadcast', definition: 'A periodic radio message that introduces your node and its capabilities. More announcing uses more airtime and battery.', helpTopic: 'nodes-telemetry' },
  { term: 'Airtime', short: 'The shared speaking time on the radio', definition: 'How long your messages occupy the radio. Every node shares it, so long or frequent messages slow neighbours down.', helpTopic: 'radio-and-region' },
  { term: 'Duty cycle', short: 'The legal limit on radio time', definition: 'The percentage of time a radio may transmit. Some regions enforce a 1% limit; OpenMesh warns before you exceed it.', helpTopic: 'radio-and-region' },
  { term: 'Region', short: 'Your country’s radio band', definition: 'The frequency plan set by your country’s regulator. Picking the wrong region is both illegal and unreliable.', helpTopic: 'radio-and-region' },
  { term: 'Radio profile', short: 'A speed-versus-range preset', definition: 'A bundle of radio settings. “Long fast” reaches further; “medium fast” sends faster. Most people never need to change it.', helpTopic: 'radio-and-region' },
  { term: 'Hop', short: 'One relay step', definition: 'Each time a repeater passes a message along counts as one hop. More hops means more reach but more delay.', helpTopic: 'first-mesh' },
  { term: 'Repeater', short: 'A radio that passes messages along', definition: 'A fixed or solar-powered node that relays traffic, usually placed high up to extend the mesh.', helpTopic: 'nodes-telemetry' },
  { term: 'Gateway', short: 'A bridge to the internet', definition: 'A node that connects your local radio mesh to other meshes or the internet, often the machine that runs a backbone.', helpTopic: 'backbone' },
  { term: 'Tracker', short: 'A small battery-powered sender', definition: 'A low-power node that mostly reports its position and battery, with strict airtime limits.', helpTopic: 'nodes-telemetry' },
  { term: 'Client', short: 'Your phone or laptop', definition: 'A node carried by a person. It sends and receives messages and may share its position if you allow it.', helpTopic: 'nodes-telemetry' },
  { term: 'PSK', short: 'A shared private key for a channel', definition: 'A secret key that encrypts a channel. Everyone in the channel holds it; never post it publicly.', helpTopic: 'channels' },
  { term: 'Channel', short: 'A private group chat', definition: 'A named group conversation protected by a shared key. You join by opening an invite link or entering its key.', helpTopic: 'channels' },
  { term: 'Invite', short: 'A link that adds someone', definition: 'A shareable link that carries a channel key or contact identity. Treat it like a password.', helpTopic: 'channels' },
  { term: 'Contact', short: 'Someone you have verified', definition: 'A person or node you added after checking their identity, so you can message them directly and trust their key.', helpTopic: 'contacts-identity' },
  { term: 'Identity', short: 'Your node’s key pair', definition: 'The public/private key pair that proves messages are from you. Back it up before resetting a device.', helpTopic: 'security' },
  { term: 'Public key', short: 'The shareable half of your identity', definition: 'A long code others use to verify you. It is safe to share; the matching private key never leaves your device.', helpTopic: 'contacts-identity' },
  { term: 'End-to-end encryption', short: 'Only the endpoints can read it', definition: 'Messages are locked on your device and unlocked by the recipient. Relays see only ciphertext.', helpTopic: 'security' },
  { term: 'Store-and-forward', short: 'Holding mail for offline people', definition: 'A relay keeps encrypted messages for someone who is offline and delivers them when they return.', helpTopic: 'messaging' },
  { term: 'Offline mail', short: 'Messages that wait for you', definition: 'Encrypted messages queued by your own node or a store-and-forward relay until a radio path is available.', helpTopic: 'messaging' },
  { term: 'Backbone', short: 'An always-on relay for distant meshes', definition: 'A server or small box that relays between meshes over the internet, so distant radios behave like one network.', helpTopic: 'backbone' },
  { term: 'Peer', short: 'Another connected backbone or gateway', definition: 'A remote node currently linked to your backbone. You can see its traffic and disconnect it from the panel.', helpTopic: 'backbone' },
  { term: 'MCP', short: 'A standard way for AI agents to use tools', definition: 'Model Context Protocol lets an AI client call OpenMesh tools such as “list nodes” in a typed, permissioned way.', helpTopic: 'agents' },
  { term: 'Agent', short: 'An AI assistant connected to your node', definition: 'An AI program using the OpenMesh MCP server. Its write actions appear in the activity log as “agent:name”.', helpTopic: 'agents' },
  { term: 'Read-only mode', short: 'An agent that can look but not touch', definition: 'MCP runs with only read tools; write actions are refused. Recommended until you trust an agent.', helpTopic: 'agents' },
  { term: 'Activity log', short: 'A list of changes made by people and agents', definition: 'Every write action, who made it and whether it worked. Message contents and secrets are never included.', helpTopic: 'agents' },
  { term: 'Token', short: 'The key the browser uses to talk to the bridge', definition: 'A secret the local bridge API checks. Keep it in Settings; rotate it if a device is lost or an agent is revoked.', helpTopic: 'security' },
  { term: 'Bridge', short: 'The program that links radio and apps', definition: 'A local service that owns the radio connection and exposes the API used by the web panel, CLI and agents.', helpTopic: 'install' },
  { term: 'Telemetry', short: 'Battery, signal and sensor readings', definition: 'Small status values nodes report alongside messages, such as battery voltage, signal strength and temperature.', helpTopic: 'nodes-telemetry' },
  { term: 'Signal strength', short: 'How strongly a radio is heard', definition: 'Usually shown in dBm. Values closer to zero are stronger; around −120 dBm is the edge of usable range.', helpTopic: 'nodes-telemetry' },
  { term: 'Hop limit', short: 'How far a message may travel', definition: 'The maximum number of relays a message may pass before it stops, so stale messages do not circle forever.', helpTopic: 'first-mesh' },
  { term: 'Position accuracy', short: 'How precise a location is', definition: 'The radius within which your reported location is likely correct. Coarse positions protect privacy.', helpTopic: 'map-and-position' },
];

export function normalizeTermKey(value: string): string {
  return value.trim().toLowerCase().replace(/[\s_]+/g, ' ');
}

export function glossaryEntry(term: string): GlossaryEntry | null {
  const key = normalizeTermKey(term);
  return (
    GLOSSARY_TERMS.find((entry) => normalizeTermKey(entry.term) === key) ??
    GLOSSARY_TERMS.find((entry) => normalizeTermKey(entry.short) === key) ??
    null
  );
}

export function glossaryDefinitions(terms: string[]): GlossaryEntry[] {
  return terms.map(glossaryEntry).filter((entry): entry is GlossaryEntry => Boolean(entry));
}
