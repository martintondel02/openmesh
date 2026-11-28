import { describe, expect, it } from 'vitest';
import {
  MEDIA_STALL_MS,
  SLOW_MEDIA_WALL_CLOCK_MS,
  clampMediaProgress,
  describeOutgoingMedia,
  formatMediaExpectation,
  mediaIdleMs,
  mediaWallClockMs,
} from '../media-transfer';
import type { Attachment, DeliveryState } from '../../types/api';

const NOW = 1_800_000_000_000;

function attachment(overrides: Partial<Attachment> = {}): Attachment {
  return {
    id: 'att_1',
    mime: 'image/jpeg',
    size: 12 * 1024,
    sha256: '00',
    duration_ms: null,
    name: 'trail.jpg',
    progress: 0,
    width: 800,
    height: 600,
    ...overrides,
  };
}

function input(overrides: Partial<Parameters<typeof describeOutgoingMedia>[0]> = {}) {
  return {
    attachment: attachment(),
    delivery: 'sent' as DeliveryState,
    error: undefined,
    msgId: 42,
    group: false,
    sentAtMs: NOW - 5_000,
    activityAtMs: NOW - 5_000,
    nowMs: NOW,
    wallClockMs: 180_000,
    ...overrides,
  };
}

describe('outgoing media: progress wording', () => {
  it('reports preparing while the browser still uploads to the bridge', () => {
    const media = describeOutgoingMedia(input({ msgId: 0, delivery: 'pending', attachment: attachment({ progress: 0 }) }));
    expect(media?.phase).toBe('preparing');
    expect(media?.label).toBe('Preparing the upload…');
    expect(media?.canRetry).toBe(false);
  });

  it('waits honestly when the bridge has not reported any radio progress yet', () => {
    const media = describeOutgoingMedia(input({ attachment: attachment({ progress: 0 }) }));
    expect(media?.phase).toBe('transferring');
    expect(media?.label).toBe('Waiting for the radio…');
    expect(media?.progress).toBe(0);
  });

  it('shows the real bridge progress percentage once chunks move', () => {
    const media = describeOutgoingMedia(input({ attachment: attachment({ progress: 0.12 }) }));
    expect(media?.phase).toBe('transferring');
    expect(media?.label).toBe('Sending over the radio… 12%');
    expect(media?.progress).toBeCloseTo(0.12);
  });

  it('never claims success before an ack: progress 1 without delivery is waiting', () => {
    const media = describeOutgoingMedia(input({ attachment: attachment({ progress: 1 }), delivery: 'sent' }));
    expect(media?.phase).toBe('sent');
    expect(media?.label).toBe('Sent over the radio — waiting for delivery confirmation');
  });

  it('settles cancelled media without offering retry or progress', () => {
    const media = describeOutgoingMedia(
      input({ attachment: attachment({ progress: 0.4 }), delivery: 'cancelled' }),
    );
    expect(media?.phase).toBe('cancelled');
    expect(media?.canRetry).toBe(false);
    expect(media?.label).toContain('Cancelled');
  });

  it('reports delivered for direct messages and keeps channel semantics', () => {
    expect(
      describeOutgoingMedia(input({ attachment: attachment({ progress: 1 }), delivery: 'delivered' }))?.phase,
    ).toBe('delivered');

    const channel = describeOutgoingMedia(
      input({ attachment: attachment({ progress: 1 }), delivery: 'sent', group: true }),
    );
    expect(channel?.phase).toBe('sent');
    expect(channel?.label).toBe('Sent to the channel — broadcast, no per-person receipt');
    expect(
      describeOutgoingMedia(input({ attachment: attachment({ progress: 1 }), delivery: 'delivered', group: true }))?.label,
    ).toBe('Sent to the channel');
  });
});

describe('outgoing media: stall detection', () => {
  it('switches to stalled after the stall window with no progress, echo or ack', () => {
    const justUnder = describeOutgoingMedia(
      input({ attachment: attachment({ progress: 0.2 }), sentAtMs: NOW - MEDIA_STALL_MS + 1_000, activityAtMs: NOW - MEDIA_STALL_MS + 1_000 }),
    );
    expect(justUnder?.phase).toBe('transferring');

    const stalled = describeOutgoingMedia(
      input({ attachment: attachment({ progress: 0.2 }), sentAtMs: NOW - MEDIA_STALL_MS, activityAtMs: NOW - MEDIA_STALL_MS }),
    );
    expect(stalled?.phase).toBe('stalled');
    expect(stalled?.label).toBe('Upload stalled — check the radio link; retry');
    expect(stalled?.canRetry).toBe(true);
    expect(stalled?.hint).toContain('does not upload from this browser again');
  });

  it('uses the last progress event, not the send time, as the stall clock', () => {
    const media = describeOutgoingMedia(
      input({
        attachment: attachment({ progress: 0.5 }),
        sentAtMs: NOW - 10 * MEDIA_STALL_MS,
        activityAtMs: NOW - 2_000,
      }),
    );
    expect(media?.phase).toBe('transferring');
  });

  it('falls back to sent_at_ms for messages restored from the bridge store', () => {
    const media = describeOutgoingMedia(
      input({ attachment: attachment({ progress: 0 }), sentAtMs: NOW - MEDIA_STALL_MS - 5_000, activityAtMs: null }),
    );
    expect(media?.phase).toBe('stalled');
  });

  it('never stalls while the send command is still in flight (watchdog owns that)', () => {
    const media = describeOutgoingMedia(
      input({ msgId: 0, delivery: 'pending', sentAtMs: NOW - 10 * MEDIA_STALL_MS, activityAtMs: NOW - 10 * MEDIA_STALL_MS }),
    );
    expect(media?.phase).toBe('preparing');
  });

  it('keeps stalled non-actionable without a bridge message id', () => {
    const media = describeOutgoingMedia(
      input({ msgId: 0, attachment: attachment({ progress: 0.4 }), sentAtMs: NOW - MEDIA_STALL_MS - 1 }),
    );
    // msg_id 0 + pending is "preparing"; an id-less stall cannot happen in practice but never offers a no-op retry.
    expect(media?.canRetry).toBe(false);
  });
});

describe('outgoing media: failures', () => {
  it('maps upload errors to a failed phase with a retry when the bridge knows the message', () => {
    const media = describeOutgoingMedia(input({ delivery: 'failed', error: 'Upload failed: network down' }));
    expect(media?.phase).toBe('failed');
    expect(media?.canRetry).toBe(true);
    expect(media?.hint).toContain('network down');
    expect(media?.hint).toContain('does not upload from this browser again');
  });

  it('offers retry for failed transfers only after the bridge returns an id', () => {
    expect(describeOutgoingMedia(input({ delivery: 'failed', error: 'boom', msgId: 0 }))?.canRetry).toBe(false);
    expect(describeOutgoingMedia(input({ delivery: 'failed', error: 'boom', msgId: 7 }))?.canRetry).toBe(true);
  });
});

describe('outgoing media: slow-media honesty', () => {
  it('adds a minutes hint for slow duty-cycle transfers only', () => {
    const slow = describeOutgoingMedia(input({ attachment: attachment({ progress: 0.1 }) }));
    expect(slow?.slow).toBe(true);
    expect(slow?.hint).toContain('keeps sending in the background');
    expect(slow?.hint).toContain('about 3 minutes');

    const fast = describeOutgoingMedia(input({ wallClockMs: 15_000 }));
    expect(fast?.slow).toBe(false);
    expect(fast?.hint).toBeNull();
  });

  it('does not show the slow hint for text or incoming messages', () => {
    expect(describeOutgoingMedia(input({ attachment: null }))).toBeNull();
  });

  it('uses the bridge effective duty cycle for the wall-clock expectation', () => {
    const estimate = { airtimeMs: 6_000, wallClockMs: 600_000 };
    expect(mediaWallClockMs(estimate, 10)).toBe(60_000);
    expect(mediaWallClockMs(estimate, 1)).toBe(600_000);
    expect(mediaWallClockMs(estimate, null)).toBe(600_000);
    expect(mediaWallClockMs(estimate, 0)).toBe(600_000);
    expect(mediaWallClockMs({ airtimeMs: 6_000, wallClockMs: 7_000 }, 150)).toBe(7_000);
  });

  it('formats expectations in plain words', () => {
    expect(formatMediaExpectation(null)).toBeNull();
    expect(formatMediaExpectation(0)).toBeNull();
    expect(formatMediaExpectation(4_000)).toBe('under 10 seconds');
    expect(formatMediaExpectation(40_000)).toBe('about 40 seconds');
    expect(formatMediaExpectation(60_000)).toBe('about 1 minute');
    expect(formatMediaExpectation(185_000)).toBe('about 3 minutes');
    expect(formatMediaExpectation(35 * 60_000)).toBe('about 35 minutes');
    expect(SLOW_MEDIA_WALL_CLOCK_MS).toBe(60_000);
  });

  it('clamps bogus progress values instead of showing impossible bars', () => {
    expect(clampMediaProgress(undefined)).toBeNull();
    expect(clampMediaProgress(Number.NaN)).toBeNull();
    expect(clampMediaProgress(-2)).toBe(0);
    expect(clampMediaProgress(1.5)).toBe(1);
  });

  it('computes idle time from activity or the send timestamp', () => {
    expect(mediaIdleMs({ nowMs: NOW, sentAtMs: NOW - 10_000, activityAtMs: NOW - 4_000 })).toBe(4_000);
    expect(mediaIdleMs({ nowMs: NOW, sentAtMs: NOW - 10_000, activityAtMs: null })).toBe(10_000);
    // Browser clock before a bridge timestamp (remote clock skew) never yields a negative age.
    expect(mediaIdleMs({ nowMs: NOW, sentAtMs: NOW + 5_000, activityAtMs: null })).toBe(0);
  });
});

describe('outgoing media: labels are accessibility-safe', () => {
  it('spells out every state in text, never colour alone', () => {
    const states = [
      describeOutgoingMedia(input({ msgId: 0, delivery: 'pending' })),
      describeOutgoingMedia(input({ attachment: attachment({ progress: 0.3 }) })),
      describeOutgoingMedia(input({ sentAtMs: NOW - MEDIA_STALL_MS - 1 })),
      describeOutgoingMedia(input({ delivery: 'failed', error: 'boom' })),
      describeOutgoingMedia(input({ attachment: attachment({ progress: 1 }), delivery: 'delivered' })),
      describeOutgoingMedia(input({ attachment: attachment({ progress: 1 }), group: true })),
    ];
    for (const media of states) {
      expect(media?.label.trim().length).toBeGreaterThan(0);
    }
  });
});
