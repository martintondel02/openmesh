import { describe, expect, it } from 'vitest';
import {
  batteryTone,
  bytesToBase64,
  bytesToHex,
  cn,
  decodeBase64OfLength,
  fingerprintHex,
  formatBytes,
  formatBytesShort,
  formatClockTime,
  formatDayLabel,
  formatDuration,
  formatFrequency,
  formatLastSeen,
  formatNodeId,
  formatPct,
  formatRelativeTime,
  formatRole,
  formatSeconds,
  formatUptime,
  dayKey,
  graphemeLength,
  hexToBytes,
  initials,
  isHex,
  isValidNodeId,
  messagePreview,
  normalizeHex,
  shortenNodeId,
  signalBars,
  signalLabel,
  snrLabel,
  spacedHex,
  truncateGraphemes,
} from '../utils';

const LOCAL_NOW = new Date(2026, 2, 15, 12, 0, 0).getTime(); // 15 March 2026, local noon

describe('utils: byte formatting', () => {
  it('formats bytes with KiB/MiB boundaries', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(1023)).toBe('1023 B');
    expect(formatBytes(1024)).toBe('1 KiB');
    expect(formatBytes(1536)).toBe('1.5 KiB');
    expect(formatBytes(12 * 1024)).toBe('12 KiB');
    expect(formatBytes(1.5 * 1024 * 1024)).toBe('1.5 MiB');
    expect(formatBytes(-5)).toBe('—');
  });

  it('formats compact sizes', () => {
    expect(formatBytesShort(900)).toBe('900B');
    expect(formatBytesShort(12_800)).toBe('12.5K');
    expect(formatBytesShort(2.5 * 1024 * 1024)).toBe('2.5M');
  });
});

describe('utils: durations and clocks', () => {
  it('formats durations for voice bubbles', () => {
    expect(formatDuration(0)).toBe('0:00');
    expect(formatDuration(7400)).toBe('0:07');
    expect(formatDuration(59_000)).toBe('0:59');
    expect(formatDuration(3_723_000)).toBe('1:02:03');
    expect(formatDuration(-1)).toBe('0:00');
  });

  it('formats seconds with an honest ms boundary', () => {
    expect(formatSeconds(610)).toBe('610 ms');
    expect(formatSeconds(7400)).toBe('7.4 s');
    expect(formatSeconds(20_000)).toBe('20 s');
  });

  it('formats wall-clock times in local time', () => {
    expect(formatClockTime(new Date(2026, 0, 5, 9, 4).getTime())).toBe('09:04');
    expect(formatClockTime(new Date(2026, 0, 5, 23, 59).getTime())).toBe('23:59');
  });

  it('formats uptime', () => {
    expect(formatUptime(48)).toBe('48s');
    expect(formatUptime(3720)).toBe('1h 2m');
    expect(formatUptime(90_000)).toBe('1d 1h');
  });

  it('formats relative timestamps deterministically', () => {
    expect(formatRelativeTime(LOCAL_NOW - 10_000, LOCAL_NOW)).toBe('now');
    expect(formatRelativeTime(LOCAL_NOW - 5 * 60_000, LOCAL_NOW)).toBe('5m');
    expect(formatRelativeTime(LOCAL_NOW - 3 * 3_600_000, LOCAL_NOW)).toBe('3h');
    expect(formatRelativeTime(LOCAL_NOW - 26 * 3_600_000, LOCAL_NOW)).toBe('Yesterday');
    expect(formatRelativeTime(LOCAL_NOW - 12 * 86_400_000, LOCAL_NOW)).toBe('3 Mar');
  });

  it('formats day separators', () => {
    expect(formatDayLabel(LOCAL_NOW, LOCAL_NOW)).toBe('Today');
    expect(formatDayLabel(LOCAL_NOW - 86_400_000, LOCAL_NOW)).toBe('Yesterday');
    expect(formatDayLabel(LOCAL_NOW - 3 * 86_400_000, LOCAL_NOW)).toMatch(/^(Mon|Tue|Wed|Thu|Fri|Sat|Sun)$/);
    expect(formatDayLabel(new Date(2025, 11, 24, 12).getTime(), LOCAL_NOW)).toBe('24 Dec 2025');
  });

  it('keys days for grouping', () => {
    expect(dayKey(new Date(2026, 2, 5, 23, 59).getTime())).toBe('2026-03-05');
    expect(dayKey(new Date(2026, 0, 1, 0, 1).getTime())).toBe('2026-01-01');
  });

  it('formats last-seen ages', () => {
    expect(formatLastSeen(null, LOCAL_NOW)).toBe('never');
    expect(formatLastSeen(LOCAL_NOW - 20_000, LOCAL_NOW)).toBe('just now');
    expect(formatLastSeen(LOCAL_NOW - 5 * 60_000, LOCAL_NOW)).toBe('5 min ago');
    expect(formatLastSeen(LOCAL_NOW - 2 * 3_600_000, LOCAL_NOW)).toBe('2 h ago');
    expect(formatLastSeen(LOCAL_NOW - 3 * 86_400_000, LOCAL_NOW)).toBe('3 days ago');
  });
});

describe('utils: node ids and identities', () => {
  it('validates 8-byte node ids', () => {
    expect(isValidNodeId('6b1f9a2c4d7e8031')).toBe(true);
    expect(isValidNodeId('6B1F9A2C4D7E8031')).toBe(true);
    expect(isValidNodeId('6b1f9a2c4d7e803')).toBe(false);
    expect(isValidNodeId('not-hex-at-all!!')).toBe(false);
    expect(isValidNodeId('')).toBe(false);
  });

  it('normalizes and formats ids', () => {
    expect(normalizeHex(' 0xAB12·CD34 ')).toBe('ab12cd34');
    expect(shortenNodeId('6b1f9a2c4d7e8031')).toBe('6b1f…8031');
    expect(formatNodeId('6b1f9a2c4d7e8031')).toBe('6b1f·9a2c·4d7e·8031');
    expect(spacedHex('abcd')).toBe('ab cd');
    expect(isHex('abcd', 2)).toBe(true);
    expect(isHex('abc', 2)).toBe(false);
  });

  it('round-trips bytes through base64 and hex', () => {
    const bytes = Uint8Array.from({ length: 72 }, (_, index) => index);
    const base64 = bytesToBase64(bytes);
    const decoded = decodeBase64OfLength(base64, 72);
    expect(decoded?.length).toBe(72);
    expect(decoded?.[71]).toBe(71);
    expect(decodeBase64OfLength(base64, 32)).toBeNull();
    expect(bytesToHex(Uint8Array.from([0xab, 0x01]))).toBe('ab01');
    expect(Array.from(hexToBytes('ab01'))).toEqual([0xab, 0x01]);
  });

  it('produces a stable friendly fingerprint', () => {
    const bytes = Uint8Array.from([1, 2, 3, 4]);
    expect(fingerprintHex(bytes)).toMatch(/^[0-9a-f]{4}$/);
    expect(fingerprintHex(bytes)).toBe(fingerprintHex(Uint8Array.from([1, 2, 3, 4])));
    expect(fingerprintHex(bytes)).not.toBe(fingerprintHex(Uint8Array.from([4, 3, 2, 1])));
  });
});

describe('utils: signal presentation', () => {
  it('maps RSSI to bars', () => {
    expect(signalBars(-70)).toBe(4);
    expect(signalBars(-85)).toBe(4);
    expect(signalBars(-90)).toBe(3);
    expect(signalBars(-100)).toBe(2);
    expect(signalBars(-110)).toBe(1);
    expect(signalBars(-120)).toBe(0);
    expect(signalBars(null)).toBe(0);
  });

  it('describes signal and SNR for tooltips', () => {
    expect(signalLabel(-70)).toContain('strong');
    expect(signalLabel(-113)).toContain('marginal');
    expect(signalLabel(null)).toBe('No signal');
    expect(snrLabel(6.54)).toBe('+6.5 dB');
    expect(snrLabel(-2)).toBe('-2.0 dB');
    expect(snrLabel(null)).toBe('—');
  });

  it('tints batteries and roles', () => {
    expect(batteryTone(90)).toBe('good');
    expect(batteryTone(30)).toBe('fair');
    expect(batteryTone(10)).toBe('low');
    expect(batteryTone(null)).toBe('unknown');
    expect(formatRole('room_server')).toBe('Room server');
    expect(formatRole('client')).toBe('Client');
    expect(formatPct(0.42, 1)).toBe('0.4%');
    expect(formatFrequency(869.525)).toBe('869.525 MHz');
  });
});

describe('utils: unicode-safe text helpers', () => {
  it('counts graphemes, not code units', () => {
    expect(graphemeLength('hello')).toBe(5);
    expect(graphemeLength('👨‍👩‍👧‍👦')).toBe(1);
    expect(graphemeLength('🇳🇴')).toBe(1);
    expect(graphemeLength('😀😀')).toBe(2);
  });

  it('truncates without splitting surrogate pairs or ZWJ sequences', () => {
    expect(truncateGraphemes('😀😀😀', 2)).toBe('😀😀…');
    expect(truncateGraphemes('👨‍👩‍👧‍👦abc', 2)).toBe('👨‍👩‍👧‍👦a…');
    expect(truncateGraphemes('short', 10)).toBe('short');
    expect(truncateGraphemes('anything', 0)).toBe('');
  });

  it('flattens whitespace for conversation previews', () => {
    expect(messagePreview('  hello\n\nworld  ')).toBe('hello world');
    expect(messagePreview(null)).toBe('No messages yet');
    expect(messagePreview('', 'Say hello')).toBe('Say hello');
  });

  it('derives initials from names', () => {
    expect(initials('Anne Marie')).toBe('AM');
    expect(initials('lars')).toBe('L');
    expect(initials('')).toBe('?');
  });

  it('joins class names', () => {
    expect(cn('a', false, null, undefined, 'b')).toBe('a b');
  });
});
