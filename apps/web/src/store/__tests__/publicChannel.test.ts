import { describe, expect, it } from 'vitest';
import type { Channel } from '../../types/api';
import { selectConversations, selectThreadHeader } from '../selectors';
import {
  PUBLIC_CHANNEL_ID,
  PUBLIC_CHANNEL_NAME,
  PUBLIC_CHANNEL_SUBTITLE,
} from '../../lib/public-channel';

const FAMILY_ID = 'f0e1d2c3b4a59687';
const NOW = 1_800_000_000_000;

const channels: Record<string, Channel> = {
  [FAMILY_ID]: { channel_id: FAMILY_ID, name: 'familien', psk_set: true },
  [PUBLIC_CHANNEL_ID]: {
    channel_id: PUBLIC_CHANNEL_ID,
    name: PUBLIC_CHANNEL_NAME,
    psk_set: true,
    public: true,
  },
};

function state(overrides: Partial<Parameters<typeof selectConversations>[0]> = {}) {
  return {
    messages: [],
    groupMessages: [],
    nodes: {},
    contacts: {},
    channels,
    readCursors: {},
    activeThread: null,
    ownNodeId: null,
    ...overrides,
  };
}

describe('selectors: Public Mesh pinning and header', () => {
  it('pins the public channel above newer user channels and marks the row public', () => {
    const conversations = selectConversations(
      state({
        groupMessages: [
          {
            msg_id: 1,
            channel_id: FAMILY_ID,
            channel_name: 'familien',
            from: 'anne',
            from_name: 'Anne',
            sender_key: '',
            kind: 'text',
            text: 'newer family message',
            sent_at_ms: NOW,
            outgoing: false,
            signature_valid: true,
            attachment: null,
            position: null,
          },
        ],
      }),
      NOW,
    );
    expect(conversations[0]!.id).toBe(PUBLIC_CHANNEL_ID);
    expect(conversations[0]!.public).toBe(true);
    expect(conversations[0]!.pinned).toBe(true);
    expect(conversations[0]!.subtitle).toBe(PUBLIC_CHANNEL_SUBTITLE);
    expect(conversations[0]!.lastText).toBe(PUBLIC_CHANNEL_SUBTITLE);
    expect(conversations.find((row) => row.id === FAMILY_ID)?.public).toBeUndefined();
  });

  it('marks the thread header public with the anyone-can-join subtitle', () => {
    const header = selectThreadHeader(
      {
        ...state(),
        activeThread: { kind: 'group', id: PUBLIC_CHANNEL_ID },
      },
      NOW,
    );
    expect(header?.public).toBe(true);
    expect(header?.name).toBe(PUBLIC_CHANNEL_NAME);
    expect(header?.subtitle).toContain('Public · anyone can join');
    expect(header?.subtitle).toContain('signed');

    const privateHeader = selectThreadHeader(
      {
        ...state(),
        activeThread: { kind: 'group', id: FAMILY_ID },
      },
      NOW,
    );
    expect(privateHeader?.public).toBe(false);
    expect(privateHeader?.subtitle).toContain('Channel ·');
  });
});
