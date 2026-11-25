/**
 * Registry of user-facing controls and stats that must carry a HelpTip.
 * The unit test in `src/lib/__tests__/controlRegistry.test.ts` asserts every entry
 * has a resolvable helpKey, topic and (for settings/backbone/agents) visible copy.
 */
import type { HelpAudience } from '../types/help';
import { HELP_TIPS } from './help-tips';

export type ControlScreen = 'chats' | 'map' | 'nodes' | 'contacts' | 'settings' | 'backbone' | 'agents' | 'onboarding';

export interface ControlEntry {
  /** Stable id used by UI code/tests. */
  id: string;
  screen: ControlScreen;
  label: string;
  helpKey: string;
  audience: HelpAudience;
}

export const CONTROL_REGISTRY: ControlEntry[] = [
  /* onboarding */
  { id: 'onboarding.start', screen: 'onboarding', label: 'Start guided setup', helpKey: 'onboarding.start', audience: 'beginner' },
  { id: 'onboarding.role', screen: 'onboarding', label: 'Device role', helpKey: 'onboarding.role', audience: 'beginner' },
  { id: 'onboarding.country', screen: 'onboarding', label: 'Country / region', helpKey: 'onboarding.country', audience: 'beginner' },
  { id: 'onboarding.name', screen: 'onboarding', label: 'Node name', helpKey: 'onboarding.name', audience: 'beginner' },
  { id: 'onboarding.privacy', screen: 'onboarding', label: 'Keep location private', helpKey: 'onboarding.privacy', audience: 'beginner' },
  { id: 'onboarding.secure', screen: 'onboarding', label: 'Secure my node', helpKey: 'onboarding.secure', audience: 'beginner' },
  { id: 'onboarding.checklist', screen: 'onboarding', label: 'Setup checklist', helpKey: 'onboarding.checklist', audience: 'beginner' },
  { id: 'onboarding.invite', screen: 'onboarding', label: 'Channel invite', helpKey: 'onboarding.invite', audience: 'beginner' },

  /* settings — node & radio */
  { id: 'settings.node.name', screen: 'settings', label: 'Node name', helpKey: 'settings.node.name', audience: 'beginner' },
  { id: 'settings.node.class', screen: 'settings', label: 'Device role', helpKey: 'settings.node.class', audience: 'beginner' },
  { id: 'settings.radio.region', screen: 'settings', label: 'Region', helpKey: 'settings.radio.region', audience: 'beginner' },
  { id: 'settings.radio.profile', screen: 'settings', label: 'Radio profile', helpKey: 'settings.radio.profile', audience: 'beginner' },
  { id: 'settings.radio.txpower', screen: 'settings', label: 'Transmit power', helpKey: 'settings.radio.txpower', audience: 'advanced' },
  { id: 'settings.announce.interval', screen: 'settings', label: 'Announce interval', helpKey: 'settings.announce.interval', audience: 'advanced' },
  { id: 'settings.auto.announce', screen: 'settings', label: 'Auto announce', helpKey: 'settings.auto.announce', audience: 'advanced' },
  { id: 'settings.position.sharing', screen: 'settings', label: 'Position sharing', helpKey: 'settings.position.sharing', audience: 'beginner' },
  { id: 'settings.store.forward', screen: 'settings', label: 'Store-and-forward', helpKey: 'settings.store.forward', audience: 'beginner' },

  /* settings — identity, bridge, danger zone */
  { id: 'settings.identity.public', screen: 'settings', label: 'Public identity', helpKey: 'settings.identity.public', audience: 'advanced' },
  { id: 'settings.identity.reset', screen: 'settings', label: 'Reset identity', helpKey: 'settings.identity.reset', audience: 'advanced' },
  { id: 'settings.bridge.url', screen: 'settings', label: 'Bridge URL', helpKey: 'settings.bridge.url', audience: 'beginner' },
  { id: 'settings.bridge.token', screen: 'settings', label: 'API token', helpKey: 'settings.bridge.token', audience: 'beginner' },
  { id: 'settings.bridge.demo', screen: 'settings', label: 'Demo mode', helpKey: 'settings.bridge.demo', audience: 'beginner' },
  { id: 'settings.api.token', screen: 'settings', label: 'API token', helpKey: 'settings.api.token', audience: 'advanced' },
  { id: 'settings.api.allowlist', screen: 'settings', label: 'Backbone allowlist', helpKey: 'settings.api.allowlist', audience: 'advanced' },
  { id: 'settings.backup.identity', screen: 'settings', label: 'Identity backup', helpKey: 'settings.backup.identity', audience: 'advanced' },
  { id: 'settings.advanced.show', screen: 'settings', label: 'Show advanced', helpKey: 'settings.advanced.show', audience: 'beginner' },

  /* nodes */
  { id: 'nodes.stat.known', screen: 'nodes', label: 'Known nodes', helpKey: 'nodes.stat.known', audience: 'both' },
  { id: 'nodes.stat.positioned', screen: 'nodes', label: 'With position', helpKey: 'nodes.stat.positioned', audience: 'both' },
  { id: 'nodes.stat.infrastructure', screen: 'nodes', label: 'Infrastructure', helpKey: 'nodes.stat.infrastructure', audience: 'both' },
  { id: 'nodes.stat.contacts', screen: 'nodes', label: 'Contacts', helpKey: 'nodes.stat.contacts', audience: 'both' },
  { id: 'nodes.detail.signal', screen: 'nodes', label: 'Signal', helpKey: 'nodes.detail.signal', audience: 'advanced' },
  { id: 'nodes.detail.hops', screen: 'nodes', label: 'Hops', helpKey: 'nodes.detail.hops', audience: 'both' },
  { id: 'nodes.detail.battery', screen: 'nodes', label: 'Battery', helpKey: 'nodes.detail.battery', audience: 'both' },
  { id: 'nodes.action.contact', screen: 'nodes', label: 'Add contact', helpKey: 'nodes.action.contact', audience: 'both' },
  { id: 'nodes.action.message', screen: 'nodes', label: 'Message node', helpKey: 'nodes.action.message', audience: 'both' },

  /* chats */
  { id: 'chats.composer.airtime', screen: 'chats', label: 'Airtime estimate', helpKey: 'chats.composer.airtime', audience: 'both' },
  { id: 'chats.composer.encryption', screen: 'chats', label: 'Encryption', helpKey: 'chats.composer.encryption', audience: 'both' },
  { id: 'chats.attachment.image', screen: 'chats', label: 'Send image', helpKey: 'chats.attachment.image', audience: 'both' },
  { id: 'chats.voice.note', screen: 'chats', label: 'Voice note', helpKey: 'chats.voice.note', audience: 'both' },
  { id: 'chats.thread.delivery', screen: 'chats', label: 'Delivery status', helpKey: 'chats.thread.delivery', audience: 'both' },
  { id: 'chats.invite.copy', screen: 'chats', label: 'Copy channel invite', helpKey: 'chats.invite.copy', audience: 'both' },

  /* map */
  { id: 'map.offline', screen: 'map', label: 'Offline map', helpKey: 'map.offline', audience: 'both' },
  { id: 'map.share.position', screen: 'map', label: 'Share position', helpKey: 'map.share.position', audience: 'both' },
  { id: 'map.accuracy', screen: 'map', label: 'Accuracy circle', helpKey: 'map.accuracy', audience: 'both' },
  { id: 'map.history', screen: 'map', label: 'Position history', helpKey: 'map.history', audience: 'both' },

  /* contacts */
  { id: 'contacts.invite', screen: 'contacts', label: 'Add contact', helpKey: 'contacts.invite', audience: 'both' },
  { id: 'contacts.verify', screen: 'contacts', label: 'Verify contact', helpKey: 'contacts.verify', audience: 'both' },

  /* backbone */
  { id: 'backbone.enabled', screen: 'backbone', label: 'Enable backbone', helpKey: 'backbone.enabled', audience: 'advanced' },
  { id: 'backbone.bind', screen: 'backbone', label: 'Bind address', helpKey: 'backbone.bind', audience: 'advanced' },
  { id: 'backbone.port', screen: 'backbone', label: 'TCP port', helpKey: 'backbone.port', audience: 'advanced' },
  { id: 'backbone.maxPeers', screen: 'backbone', label: 'Max peers', helpKey: 'backbone.maxPeers', audience: 'advanced' },
  { id: 'backbone.name', screen: 'backbone', label: 'Backbone name', helpKey: 'backbone.name', audience: 'advanced' },
  { id: 'backbone.publicHost', screen: 'backbone', label: 'Public host', helpKey: 'backbone.publicHost', audience: 'advanced' },
  { id: 'backbone.ping', screen: 'backbone', label: 'Peer ping interval', helpKey: 'backbone.ping', audience: 'advanced' },
  { id: 'backbone.storeForward', screen: 'backbone', label: 'Store-and-forward', helpKey: 'backbone.storeForward', audience: 'advanced' },
  { id: 'backbone.auth', screen: 'backbone', label: 'Peer authentication', helpKey: 'backbone.auth', audience: 'advanced' },
  { id: 'backbone.allowlist', screen: 'backbone', label: 'Peer allowlist', helpKey: 'backbone.allowlist', audience: 'advanced' },
  { id: 'backbone.token', screen: 'backbone', label: 'Backbone token', helpKey: 'backbone.token', audience: 'advanced' },
  { id: 'backbone.rotate', screen: 'backbone', label: 'Rotate token', helpKey: 'backbone.rotate', audience: 'advanced' },
  { id: 'backbone.connection', screen: 'backbone', label: 'Connection string', helpKey: 'backbone.connection', audience: 'advanced' },
  { id: 'backbone.qr', screen: 'backbone', label: 'Invite QR', helpKey: 'backbone.qr', audience: 'advanced' },
  { id: 'backbone.stat.peers', screen: 'backbone', label: 'Peers stat', helpKey: 'backbone.stat.peers', audience: 'advanced' },
  { id: 'backbone.stat.frames', screen: 'backbone', label: 'Frames stat', helpKey: 'backbone.stat.frames', audience: 'advanced' },
  { id: 'backbone.stat.throughput', screen: 'backbone', label: 'Throughput stat', helpKey: 'backbone.stat.throughput', audience: 'advanced' },
  { id: 'backbone.stat.bytes', screen: 'backbone', label: 'Bytes stat', helpKey: 'backbone.stat.bytes', audience: 'advanced' },
  { id: 'backbone.stat.storeForward', screen: 'backbone', label: 'Store-forward stat', helpKey: 'backbone.stat.storeForward', audience: 'advanced' },
  { id: 'backbone.stat.authFailures', screen: 'backbone', label: 'Auth failures stat', helpKey: 'backbone.stat.authFailures', audience: 'advanced' },
  { id: 'backbone.peer.kick', screen: 'backbone', label: 'Kick peer', helpKey: 'backbone.peer.kick', audience: 'advanced' },
  { id: 'backbone.setup', screen: 'backbone', label: 'Setup wizard', helpKey: 'backbone.setup', audience: 'both' },
  { id: 'backbone.exposure', screen: 'backbone', label: 'Public exposure', helpKey: 'backbone.exposure', audience: 'both' },

  /* agents */
  { id: 'agents.intro', screen: 'agents', label: 'What is an agent', helpKey: 'agents.intro', audience: 'both' },
  { id: 'agents.mcp.claude', screen: 'agents', label: 'Claude MCP config', helpKey: 'agents.mcp.claude', audience: 'advanced' },
  { id: 'agents.mcp.cursor', screen: 'agents', label: 'Cursor MCP config', helpKey: 'agents.mcp.cursor', audience: 'advanced' },
  { id: 'agents.mcp.dsh', screen: 'agents', label: 'DSH MCP config', helpKey: 'agents.mcp.dsh', audience: 'advanced' },
  { id: 'agents.mcp.generic', screen: 'agents', label: 'Generic MCP config', helpKey: 'agents.mcp.generic', audience: 'advanced' },
  { id: 'agents.readonly', screen: 'agents', label: 'Read-only mode', helpKey: 'agents.readonly', audience: 'both' },
  { id: 'agents.prompt', screen: 'agents', label: 'Agent prompt', helpKey: 'agents.prompt', audience: 'both' },
  { id: 'agents.activity', screen: 'agents', label: 'Activity log', helpKey: 'agents.activity', audience: 'both' },
  { id: 'agents.revoke', screen: 'agents', label: 'Revoke API token', helpKey: 'agents.revoke', audience: 'both' },
  { id: 'agents.security', screen: 'agents', label: 'Agent security rules', helpKey: 'agents.security', audience: 'both' },
];

export function controlEntriesFor(screen: ControlScreen): ControlEntry[] {
  return CONTROL_REGISTRY.filter((entry) => entry.screen === screen);
}

export interface RegistryProblem {
  controlId: string;
  problem: 'missing-help-key' | 'missing-tip' | 'missing-topic';
}

/** Structural validation used by tests and (defensively) by screens in dev. */
export function validateControlRegistry(topicSlugs: Iterable<string>): RegistryProblem[] {
  const slugs = new Set(topicSlugs);
  const problems: RegistryProblem[] = [];
  for (const entry of CONTROL_REGISTRY) {
    if (!entry.helpKey) problems.push({ controlId: entry.id, problem: 'missing-help-key' });
    const tip = entry.helpKey ? HELP_TIPS[entry.helpKey] : null;
    if (!tip) {
      problems.push({ controlId: entry.id, problem: 'missing-tip' });
      continue;
    }
    if (!slugs.has(tip.topic)) problems.push({ controlId: entry.id, problem: 'missing-topic' });
  }
  return problems;
}

/** Tips whose topic is not referenced by any control (helps keep the map tidy). */
export function unusedHelpTips(): string[] {
  const used = new Set(CONTROL_REGISTRY.map((entry) => entry.helpKey));
  return Object.keys(HELP_TIPS).filter((key) => !used.has(key) && key !== 'common.help' && key !== 'mode.toggle');
}
