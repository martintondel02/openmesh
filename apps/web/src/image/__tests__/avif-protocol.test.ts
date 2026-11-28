import { describe, expect, it } from 'vitest';
import { avifEncodeSettings, calibratedAvifQuality, isAvifFileHeader } from '../avif-protocol';

function header(brand = 'avif'): Uint8Array {
  const bytes = new Uint8Array(16);
  bytes.set([0x00, 0x00, 0x00, 0x10], 0);
  bytes.set([...'ftyp'].map((character) => character.charCodeAt(0)), 4);
  bytes.set([...brand].map((character) => character.charCodeAt(0)), 8);
  return bytes;
}

describe('avif protocol: quality-calibrated preset mapping', () => {
  it('calibrates the AVIF quality to 1.25x the preset WebP quality (integer floor, cap 100)', () => {
    expect(calibratedAvifQuality(28)).toBe(35);
    expect(calibratedAvifQuality(40)).toBe(50);
    expect(calibratedAvifQuality(50)).toBe(62);
    expect(calibratedAvifQuality(86)).toBe(100);
    expect(calibratedAvifQuality(1)).toBe(1);
  });

  it('maps the shared preset fractions to WebP and AVIF qualities plus the speed cap', () => {
    expect(avifEncodeSettings(0.28)).toEqual({ webpQuality: 28, avifQuality: 35, speed: 10 });
    expect(avifEncodeSettings(0.4)).toEqual({ webpQuality: 40, avifQuality: 50, speed: 8 });
    expect(avifEncodeSettings(0.5)).toEqual({ webpQuality: 50, avifQuality: 62, speed: 8 });
    expect(avifEncodeSettings(0.86)).toEqual({ webpQuality: 86, avifQuality: 100, speed: 8 });
  });

  it('clamps bad fractions instead of producing an invalid quality', () => {
    expect(avifEncodeSettings(0)).toEqual({ webpQuality: 1, avifQuality: 1, speed: 10 });
    expect(avifEncodeSettings(2)).toEqual({ webpQuality: 100, avifQuality: 100, speed: 8 });
    expect(avifEncodeSettings(Number.NaN)).toEqual({ webpQuality: 50, avifQuality: 62, speed: 8 });
    expect(avifEncodeSettings(Number.POSITIVE_INFINITY)).toEqual({ webpQuality: 50, avifQuality: 62, speed: 8 });
  });
});

describe('avif protocol: output header check', () => {
  it('accepts only complete AVIF (ftyp avif/avis) headers', () => {
    expect(isAvifFileHeader(header('avif'))).toBe(true);
    expect(isAvifFileHeader(header('avis'))).toBe(true);
    // PNG magic ("\x89PNG") and truncated inputs are rejected.
    expect(isAvifFileHeader(new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0]))).toBe(
      false,
    );
    expect(isAvifFileHeader(header('avif').slice(0, 8))).toBe(false);
    expect(isAvifFileHeader(new Uint8Array(0))).toBe(false);
  });

  it('rejects ftyp files with a non-AVIF brand', () => {
    expect(isAvifFileHeader(header('webp'))).toBe(false);
  });
});
