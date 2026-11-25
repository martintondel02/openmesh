import { describe, expect, it } from 'vitest';
import {
  ANNOUNCE_FRAME_BYTES,
  DEFAULT_PROFILE_ID,
  FRAME_MTU_BYTES,
  RADIO_PROFILES,
  RESOURCE_CHUNK_BYTES,
  assessDutyCycle,
  attachmentBytesPerHour,
  chunkCount,
  chunkFrameBytes,
  estimateAttachment,
  estimateText,
  getProfile,
  hourlyAirtimeBudgetMs,
  loraAirtimeMs,
  maxAttachmentBytes,
  voiceCapMs,
} from '../airtime';

describe('airtime: resources', () => {
  it('counts 180-byte chunks like PROTOCOL §9 (5 KiB → 29 chunks)', () => {
    expect(chunkCount(5120)).toBe(29);
    expect(chunkCount(0)).toBe(0);
    expect(chunkCount(1)).toBe(1);
    expect(chunkCount(RESOURCE_CHUNK_BYTES)).toBe(1);
    expect(chunkCount(RESOURCE_CHUNK_BYTES + 1)).toBe(2);
  });

  it('keeps resource frames inside the 240-byte MTU', () => {
    expect(FRAME_MTU_BYTES).toBe(240);
    expect(chunkFrameBytes()).toBeLessThanOrEqual(FRAME_MTU_BYTES);
    expect(chunkFrameBytes()).toBe(209);
  });

  it('matches the worked example: 5 KiB ≈ 20 s at MEDIUM_FAST', () => {
    const estimate = estimateAttachment(5 * 1024, 'MEDIUM_FAST', 'EU868');
    expect(estimate.chunks).toBe(29);
    expect(estimate.airtimeMs).toBeGreaterThan(14_000);
    expect(estimate.airtimeMs).toBeLessThan(24_000);
    expect(estimate.withinCap).toBe(true);
  });

  it('matches the worked example: 8 s voice (~7 KiB) ≈ 27 s at MEDIUM_FAST', () => {
    const estimate = estimateAttachment(7168, 'MEDIUM_FAST', 'EU868');
    expect(estimate.airtimeMs).toBeGreaterThan(20_000);
    expect(estimate.airtimeMs).toBeLessThan(33_000);
  });

  it('flags attachments over the profile cap', () => {
    const estimate = estimateAttachment(maxAttachmentBytes('LONG_SLOW') + 1, 'LONG_SLOW');
    expect(estimate.withinCap).toBe(false);
    expect(estimate.percentOfCap).toBeGreaterThanOrEqual(99);
  });

  it('models wall-clock time from the region duty cycle', () => {
    const eu = estimateAttachment(4096, 'MEDIUM_FAST', 'EU868');
    const us = estimateAttachment(4096, 'MEDIUM_FAST', 'US915');
    expect(eu.wallClockMs).toBeGreaterThan(eu.airtimeMs * 50); // 1 % duty ≈ 100× airtime
    expect(us.wallClockMs).toBe(us.transferMs); // no duty cycle
  });
});

describe('airtime: model presets and regions', () => {
  it('ships the protocol attachment and voice caps', () => {
    expect(maxAttachmentBytes('SHORT_TURBO')).toBe(24 * 1024);
    expect(maxAttachmentBytes('SHORT_FAST')).toBe(24 * 1024);
    expect(maxAttachmentBytes('MEDIUM_FAST')).toBe(12 * 1024);
    expect(maxAttachmentBytes('LONG_FAST')).toBe(6 * 1024);
    expect(maxAttachmentBytes('LONG_SLOW')).toBe(2 * 1024);
    expect(voiceCapMs('SHORT_TURBO')).toBe(12_000);
    expect(voiceCapMs('MEDIUM_FAST')).toBe(10_000);
    expect(voiceCapMs('LONG_FAST')).toBe(8_000);
    expect(voiceCapMs('LONG_SLOW')).toBe(5_000);
  });

  it('falls back to MEDIUM_FAST for unknown profiles', () => {
    expect(getProfile('NOPE').id).toBe(DEFAULT_PROFILE_ID);
    expect(getProfile(null).id).toBe(DEFAULT_PROFILE_ID);
  });

  it('orders presets by airtime for the same payload', () => {
    const fast = loraAirtimeMs(120, 'SHORT_TURBO');
    const medium = loraAirtimeMs(120, 'MEDIUM_FAST');
    const slow = loraAirtimeMs(120, 'LONG_SLOW');
    expect(fast).toBeGreaterThan(0);
    expect(medium).toBeGreaterThan(fast);
    expect(slow).toBeGreaterThan(medium);
  });

  it('reports region duty budgets', () => {
    expect(hourlyAirtimeBudgetMs('EU868')).toBe(36_000);
    expect(hourlyAirtimeBudgetMs('US915')).toBeNull();
    expect(attachmentBytesPerHour('MEDIUM_FAST', 'EU868')).toBeGreaterThan(1024);
    expect(attachmentBytesPerHour('MEDIUM_FAST', 'US915')).toBeNull();
  });

  it('warns when the announce interval eats the duty cycle', () => {
    const tight = assessDutyCycle('MEDIUM_FAST', 'EU868', 300);
    expect(tight.level).toBe('warn');
    expect(tight.announceBudgetPct).toBeGreaterThan(10);
    const sane = assessDutyCycle('MEDIUM_FAST', 'EU868', 1800);
    expect(sane.level).toBe('ok');
    const noLimit = assessDutyCycle('MEDIUM_FAST', 'US915', 300);
    expect(noLimit.level).toBe('ok');
    expect(noLimit.budgetMs).toBeNull();
  });
});

describe('airtime: text estimate', () => {
  it('keeps a 100-char text to one frame and ~0.6 s', () => {
    const estimate = estimateText('a'.repeat(100), 'MEDIUM_FAST');
    expect(estimate.fitsSingleFrame).toBe(true);
    expect(estimate.frames).toBe(1);
    expect(estimate.airtimeMs).toBeGreaterThan(450);
    expect(estimate.airtimeMs).toBeLessThan(1000);
  });

  it('counts UTF-8 bytes, not JavaScript code units', () => {
    const ascii = estimateText('abcd', 'MEDIUM_FAST');
    const emoji = estimateText('😀😀', 'MEDIUM_FAST');
    expect(emoji.bytes).toBeGreaterThan(ascii.bytes);
  });

  it('marks long text as a multi-frame resource', () => {
    const estimate = estimateText('x'.repeat(400), 'MEDIUM_FAST');
    expect(estimate.fitsSingleFrame).toBe(false);
    expect(estimate.frames).toBeGreaterThan(1);
  });

  it('includes the identity overhead for first contact', () => {
    const without = estimateText('hello', 'MEDIUM_FAST');
    const withIdentity = estimateText('hello', 'MEDIUM_FAST', { identityIncluded: true });
    expect(withIdentity.bytes - without.bytes).toBe(72);
  });
});

describe('airtime: profile table invariants', () => {
  it('every preset has sane parameters and media caps', () => {
    for (const profile of Object.values(RADIO_PROFILES)) {
      expect(profile.bwKHz).toBeGreaterThan(0);
      expect(profile.spreadingFactor).toBeGreaterThanOrEqual(7);
      expect(profile.spreadingFactor).toBeLessThanOrEqual(12);
      expect(profile.codingRate).toBeGreaterThanOrEqual(1);
      expect(profile.codingRate).toBeLessThanOrEqual(4);
      expect(profile.attachmentCapBytes).toBeGreaterThanOrEqual(2 * 1024);
      expect(profile.voiceCapMs).toBeGreaterThanOrEqual(5000);
      expect(profile.approxKbps).toBeGreaterThan(0);
    }
  });

  it('estimates an announce at a plausible ~500 ms', () => {
    expect(ANNOUNCE_FRAME_BYTES).toBe(200);
    const airtime = loraAirtimeMs(ANNOUNCE_FRAME_BYTES, 'MEDIUM_FAST');
    expect(airtime).toBeGreaterThan(400);
    expect(airtime).toBeLessThan(800);
  });
});
