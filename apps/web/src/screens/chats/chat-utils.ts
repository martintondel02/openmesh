/** Small chat-specific helpers shared by the thread, list and modals. */

import { dayKey, formatDayLabel, truncateGraphemes } from '../../lib/utils';

export { formatDayLabel, dayKey };

export interface DayGroups<T> {
  key: string;
  label: string;
  items: T[];
}

/** Split a message list into day sections, preserving order. */
export function groupMessagesByDay<T extends { sent_at_ms: number }>(messages: T[], nowMs = Date.now()): DayGroups<T>[] {
  const groups: DayGroups<T>[] = [];
  for (const message of messages) {
    const key = dayKey(message.sent_at_ms);
    const last = groups[groups.length - 1];
    if (last && last.key === key) {
      last.items.push(message);
    } else {
      groups.push({ key, label: formatDayLabel(message.sent_at_ms, nowMs), items: [message] });
    }
  }
  return groups;
}

/** Stable short form of a channel id for dense headers. */
export function shortChannelId(channelId: string): string {
  return channelId.length > 9 ? `${channelId.slice(0, 6)}…${channelId.slice(-3)}` : channelId;
}

/** Reply quote text for the composer/thread preview. */
export function replyPreviewText(text: string | null, kind: string): string {
  if (text) return truncateGraphemes(text.replace(/\s+/g, ' ').trim(), 90);
  if (kind === 'attachment') return 'Attachment';
  if (kind === 'position') return 'Shared position';
  if (kind === 'telemetry') return 'Telemetry';
  return 'Message';
}
