import { describe, expect, it } from 'vitest';
import { GROUP_CANCEL_WINDOW_MS, cancelAvailability, isMediaTransferInFlight } from '../message-cancel';

function input(overrides: Partial<Parameters<typeof cancelAvailability>[0]> = {}) {
  return {
    outgoing: true,
    msgId: 42,
    delivery: 'sent' as const,
    ...overrides,
  };
}

describe('cancel send: availability', () => {
  it('offers Cancel for any own send that is not delivered/cancelled yet', () => {
    expect(cancelAvailability(input()).state).toBe('available');
    expect(cancelAvailability(input({ delivery: 'pending' })).state).toBe('available');
    const info = cancelAvailability(input());
    expect(info.label).toBe('Cancel');
    expect(info.canCancel).toBe(true);
    expect(info.title).toContain('airtime');
  });

  it('hides Cancel for incoming, id-less or already-settled messages', () => {
    expect(cancelAvailability(input({ outgoing: false })).state).toBe('hidden');
    expect(cancelAvailability(input({ msgId: 0, delivery: 'pending' })).state).toBe('hidden');
    for (const delivery of ['delivered', 'read', 'failed', 'cancelled'] as const) {
      expect(cancelAvailability(input({ delivery })).state).toBe('hidden');
    }
    expect(cancelAvailability(input({ error: 'boom' })).state).toBe('hidden');
  });

  it('keeps channel Cancel for in-flight media or a short window only', () => {
    const nowMs = 1_800_000_000_000;
    // Recent channel text: cancellable.
    expect(cancelAvailability(input({ group: true, sentAtMs: nowMs - 10_000, nowMs })).state).toBe('available');
    // Old channel text: hidden (no per-recipient ack would otherwise keep Cancel forever).
    expect(cancelAvailability(input({ group: true, sentAtMs: nowMs - GROUP_CANCEL_WINDOW_MS - 1, nowMs })).state).toBe(
      'hidden',
    );
    // Old channel media still transferring: cancellable.
    expect(
      cancelAvailability(
        input({ group: true, sentAtMs: nowMs - 10 * GROUP_CANCEL_WINDOW_MS, nowMs, hasAttachment: true, attachmentProgress: 0.4 }),
      ).state,
    ).toBe('available');
    // Direct sends do not use the channel window.
    expect(cancelAvailability(input({ sentAtMs: nowMs - 10 * GROUP_CANCEL_WINDOW_MS, nowMs })).state).toBe('available');
  });

  it('classifies media in flight from progress', () => {
    expect(isMediaTransferInFlight({ hasAttachment: true, attachmentProgress: 0.4 })).toBe(true);
    expect(isMediaTransferInFlight({ hasAttachment: true, attachmentProgress: 1 })).toBe(false);
    expect(isMediaTransferInFlight({ hasAttachment: true, attachmentProgress: null })).toBe(true);
    expect(isMediaTransferInFlight({ hasAttachment: false, attachmentProgress: 0.4 })).toBe(false);
  });

  it('shows a disabled Cancelling… state while the request is in flight', () => {
    const info = cancelAvailability(input({ cancelRequested: true }));
    expect(info.state).toBe('requesting');
    expect(info.label).toBe('Cancelling…');
    expect(info.canCancel).toBe(false);
  });

  it('reports unsupported bridges instead of repeatedly offering a dead action', () => {
    const info = cancelAvailability(input({ bridgeSupportsCancel: false }));
    expect(info.state).toBe('unavailable');
    expect(info.bridgeUnsupported).toBe(true);
    expect(info.canCancel).toBe(false);
    // Unknown support (default) keeps the action.
    expect(cancelAvailability(input({ bridgeSupportsCancel: null })).state).toBe('available');
  });
});
