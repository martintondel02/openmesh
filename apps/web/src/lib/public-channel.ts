/**
 * Built-in "Public Mesh" channel — constants and helpers.
 *
 * Everyone with OpenMesh can read and post here; messages are signed so senders can be verified,
 * but there is no privacy. The canonical channel id is protocol-derived from the published PSK
 * (`a16d63142854cef4`); the bridge also marks the channel `public: true`. Clients must never
 * offer leave/delete for it and must re-create/keep it after a reset.
 *
 * The join URL intentionally carries only `name` + `psk` (the id is derived and may be shown in
 * the UI, but the parseable invite does not depend on it).
 */

export const PUBLIC_CHANNEL_NAME = 'Public Mesh';
export const PUBLIC_CHANNEL_PSK = '-w-KmLkWsRmxrtQqcPT2sXrlcTHTiaCNu1F-Qp_YqZI';
export const PUBLIC_CHANNEL_ID = 'a16d63142854cef4';
export const PUBLIC_CHANNEL_JOIN_URL = `openmesh://channel?name=${encodeURIComponent(
  PUBLIC_CHANNEL_NAME,
)}&psk=${encodeURIComponent(PUBLIC_CHANNEL_PSK)}`;

export const PUBLIC_CHANNEL_WARNING =
  'This is a public channel: anyone with OpenMesh can read and post here. Messages are signed, so senders are verifiable, but there is no privacy.';

export const PUBLIC_CHANNEL_SUBTITLE = 'Public · anyone can join';

export interface PublicChannelIdentity {
  channel_id?: string | null;
  name?: string | null;
  public?: boolean | null;
}

/** True when the API object is the built-in Public Mesh channel (flag, canonical id or name). */
export function isPublicChannel(channel: PublicChannelIdentity | null | undefined): boolean {
  if (!channel) return false;
  if (channel.public === true) return true;
  if ((channel.channel_id ?? '').toLowerCase() === PUBLIC_CHANNEL_ID) return true;
  return (channel.name ?? '').trim().toLowerCase() === PUBLIC_CHANNEL_NAME.toLowerCase();
}

export function isPublicChannelId(channelId: string | null | undefined): boolean {
  return (channelId ?? '').toLowerCase() === PUBLIC_CHANNEL_ID;
}

/** First Public Mesh channel in a `channels` map, if present. */
export function findPublicChannel<T extends PublicChannelIdentity>(
  channels: Record<string, T>,
): T | undefined {
  return Object.values(channels).find((channel) => isPublicChannel(channel));
}

/** Public Mesh can never be left/deleted, even if an old bridge sends no `public` flag. */
export function publicChannelLeaveBlocked(
  channelId: string | null | undefined,
  channels: Record<string, PublicChannelIdentity>,
): boolean {
  return isPublicChannelId(channelId) || isPublicChannel(channels[channelId ?? '']);
}

export interface PublicChannelAutoSelectInput {
  mode: 'live' | 'demo';
  hasActiveThread: boolean;
  hasThreadDeepLink: boolean;
  messageCount: number;
  channels: Record<string, PublicChannelIdentity>;
}

/**
 * Auto-open Public Mesh only on a fresh/empty live client (first run or after a full reset),
 * never over a deep link or an active conversation, and never in demo mode (seeded history).
 */
export function shouldAutoSelectPublicChannel(input: PublicChannelAutoSelectInput): boolean {
  if (input.mode !== 'live' || input.hasActiveThread || input.hasThreadDeepLink) return false;
  if (input.messageCount > 0) return false;
  return Boolean(findPublicChannel(input.channels));
}
