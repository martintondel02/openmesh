import { describe, expect, it } from 'vitest';
import {
  compressionSummary,
  compressionTargetBytes,
  estimateEncodedBytes,
  estimateVoiceBytes,
  planCompression,
  planDimensions,
  sliderToTargetBytes,
  targetBytesToSlider,
} from '../compression';

describe('compression: dimensions', () => {
  it('fits a landscape image inside a square box without upscaling', () => {
    const plan = planDimensions(4032, 3024, 1600, 1600);
    expect(plan.width).toBe(1600);
    expect(plan.height).toBe(1200);
    expect(plan.scale).toBeLessThan(1);
    expect(plan.upscaled).toBe(false);
  });

  it('fits a portrait image and keeps its aspect ratio', () => {
    const plan = planDimensions(1200, 3600, 1600, 1600);
    expect(plan.height).toBe(1600);
    expect(plan.width).toBe(533);
    expect(Math.abs(plan.width / plan.height - 1200 / 3600)).toBeLessThan(0.01);
  });

  it('never upscales a small image', () => {
    const plan = planDimensions(320, 200, 1600, 1600);
    expect(plan.width).toBe(320);
    expect(plan.height).toBe(200);
    expect(plan.scale).toBe(1);
    expect(plan.upscaled).toBe(false);
  });

  it('handles degenerate inputs safely', () => {
    expect(planDimensions(0, 100, 1600, 1600)).toEqual({ width: 0, height: 0, scale: 0, upscaled: false });
    expect(planDimensions(-1, -1, 100, 100).width).toBe(0);
  });
});

describe('compression: encoder estimate', () => {
  it('grows with pixel count and quality', () => {
    const small = estimateEncodedBytes(200, 200, 0.6);
    const large = estimateEncodedBytes(800, 800, 0.6);
    const lowQuality = estimateEncodedBytes(800, 800, 0.3);
    const highQuality = estimateEncodedBytes(800, 800, 0.9);
    expect(large).toBeGreaterThan(small);
    expect(highQuality).toBeGreaterThan(lowQuality);
    expect(small).toBeGreaterThan(0);
  });
});

describe('compression: planning', () => {
  it('lands under the target for a 12 KiB profile cap', () => {
    const plan = planCompression(4032, 3024, 12 * 1024);
    expect(plan.estimatedBytes).toBeLessThanOrEqual(12 * 1024);
    expect(plan.width).toBeGreaterThan(0);
    expect(plan.height).toBeGreaterThan(0);
    expect(plan.width * plan.height).toBeLessThan(4032 * 3024);
    expect(plan.quality).toBeGreaterThanOrEqual(0.3);
    expect(plan.iterations).toBeGreaterThan(0);
  });

  it('keeps every compression plan above a minimum quality', () => {
    const plan = planCompression(12000, 8000, 1024, { minQuality: 0.32, minDimension: 96 });
    expect(plan.quality).toBeGreaterThanOrEqual(0.32);
    expect(plan.estimatedBytes).toBeLessThanOrEqual(12000 * 8000 * 0.32);
  });

  it('shrinks aggressively for 2 KiB caps while staying valid', () => {
    const plan = planCompression(4000, 3000, 2 * 1024);
    expect(plan.width).toBeGreaterThanOrEqual(64);
    expect(plan.height).toBeGreaterThanOrEqual(64);
    expect(plan.atFloor).toBe(true);
    expect(Number.isFinite(plan.estimatedBytes)).toBe(true);
  });

  it('uses ~85 % of the profile cap as the default target', () => {
    expect(compressionTargetBytes(12 * 1024)).toBe(Math.floor(12 * 1024 * 0.85));
    expect(compressionTargetBytes(0)).toBe(2048);
  });
});

describe('compression: slider mapping', () => {
  const cap = 12 * 1024;

  it('maps slider ends to 1 KiB and the cap', () => {
    expect(sliderToTargetBytes(0, cap)).toBe(1024);
    expect(sliderToTargetBytes(100, cap)).toBe(cap);
  });

  it('round-trips slider → bytes → slider', () => {
    for (const slider of [0, 25, 50, 75, 100]) {
      const bytes = sliderToTargetBytes(slider, cap);
      expect(targetBytesToSlider(bytes, cap)).toBe(slider);
    }
  });

  it('clamps out-of-range slider positions', () => {
    expect(sliderToTargetBytes(-20, cap)).toBe(1024);
    expect(sliderToTargetBytes(300, cap)).toBe(cap);
  });
});

describe('compression: voice size', () => {
  it('estimates Opus at ~7 kbps', () => {
    const bytes = estimateVoiceBytes(8000, 7);
    expect(bytes).toBe(7000);
    expect(estimateVoiceBytes(0)).toBe(0);
  });

  it('fits a 10 s MEDIUM_FAST voice note under the 12 KiB attachment cap', () => {
    expect(estimateVoiceBytes(10_000, 7)).toBeLessThan(12 * 1024);
  });
});

describe('compression: summary', () => {
  it('summarises dimensions and quality for the preview chip', () => {
    const text = compressionSummary(400_000, { estimatedBytes: 12_000, width: 640, height: 480, quality: 0.6 });
    expect(text).toContain('640×480');
    expect(text).toContain('q60');
    expect(text).toContain('3%');
  });
});
