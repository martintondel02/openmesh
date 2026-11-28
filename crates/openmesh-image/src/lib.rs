//! # openmesh-image — bundled AVIF encoder
//!
//! Shared, pure-Rust AVIF encoder for the web (WASM) and Android (UniFFI) clients. Neither
//! Chromium canvas nor the Android platform codecs can *encode* AVIF (both can decode it), so the
//! clients bundle this module and keep WebP/JPEG only as a fallback on encoder failure.
//!
//! ## Quality / speed policy
//!
//! `content/presets/media.json` stores tier quality as a normalized fraction (`0.28`, `0.40`,
//! `0.50`). [`quality_from_fraction`] maps that to the ravif/rav1e 1–100 quality scale used by
//! [`encode_avif_rgba`]. Speed is chosen by quality unless the caller overrides it:
//!
//! | Tier | max_dim | quality | ravif quality | ravif speed |
//! | --- | --- | --- | --- | --- |
//! | tiny | 256 | 0.28 | 28 | 10 |
//! | medium | 512 | 0.40 | 40 | 8 |
//! | high | 1024 | 0.50 | 50 | 8 |
//!
//! Measured on the fixture/real photos in task-33: medium/high are 21–31 % smaller than WebP at
//! the same nominal quality; tiny is ~16–18 % smaller. Speed 10 is for the tiny tier (smallest
//! payload, fastest); speed 7–8 for medium/high keeps phone encode < ~1 s in practice.
//!
//! The encoder is deterministic for a fixed input/quality/speed, so clients can cache encoded
//! bytes and tests can assert byte-for-byte stability. No wire-format impact: AVIF bytes are just
//! an attachment MIME (`image/avif`) like any other image.

use ravif::{Encoder, Img, RGBA8};

/// MIME type of the encoded output (never mislabel fallback formats as AVIF).
pub const AVIF_MIME: &str = "image/avif";
/// Maximum accepted input dimensions (4096 × 4096) to avoid accidental huge allocations.
pub const MAX_PIXELS: u64 = 4096 * 4096;
/// Minimum ravif speed value (slowest/best compression).
pub const MIN_SPEED: u8 = 1;
/// Maximum ravif speed value (fastest/worst compression).
pub const MAX_SPEED: u8 = 10;

/// Errors returned by the encoder.
#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    /// Width or height was zero.
    #[error("image dimensions must be non-zero")]
    ZeroDimensions,
    /// The buffer length did not match `width * height * 4` RGBA bytes.
    #[error("expected {expected} RGBA bytes, got {actual}")]
    BufferLength {
        /// Expected number of bytes.
        expected: usize,
        /// Actual buffer length.
        actual: usize,
    },
    /// The image exceeds [`MAX_PIXELS`].
    #[error("image is too large ({pixels} pixels; max {max})")]
    TooLarge {
        /// Requested pixel count.
        pixels: u64,
        /// Maximum accepted pixel count.
        max: u64,
    },
    /// The underlying encoder rejected the image.
    #[error("AVIF encoder failed: {0}")]
    Encode(#[from] ravif::Error),
}

/// Map a normalized tier quality (`0.0..=1.0`, as stored in `content/presets/media.json`) to the
/// ravif/rav1e 1–100 quality scale. Values are clamped; `0.28 -> 28`, `0.4 -> 40`, `0.5 -> 50`.
pub fn quality_from_fraction(fraction: f32) -> u8 {
    if !fraction.is_finite() {
        return 50;
    }
    (fraction * 100.0).round().clamp(1.0, 100.0) as u8
}

/// Default ravif speed for a quality setting: tiny tiers (quality ≤30) use speed 10, everything
/// else (medium/high) speed 8, keeping phone/browser encode under ~1–2 s. Callers may override with
/// [`encode_avif_rgba_with_speed`].
pub fn speed_for_quality(quality: u8) -> u8 {
    if quality <= 30 { 10 } else { 8 }
}

/// Encode an RGBA8 buffer to an AVIF file.
///
/// `quality` is ravif's 1–100 scale (see [`quality_from_fraction`]); speed is chosen by
/// [`speed_for_quality`]. The returned bytes are a complete `ftypavif` file, ready to be uploaded
/// as an `image/avif` attachment. The result is deterministic for the same input/quality/speed.
pub fn encode_avif_rgba(
    rgba: &[u8],
    width: u32,
    height: u32,
    quality: u8,
) -> Result<Vec<u8>, ImageError> {
    encode_avif_rgba_with_speed(rgba, width, height, quality, speed_for_quality(quality))
}

/// Same as [`encode_avif_rgba`] with an explicit ravif speed (`1..=10`, clamped).
pub fn encode_avif_rgba_with_speed(
    rgba: &[u8],
    width: u32,
    height: u32,
    quality: u8,
    speed: u8,
) -> Result<Vec<u8>, ImageError> {
    if width == 0 || height == 0 {
        return Err(ImageError::ZeroDimensions);
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_PIXELS {
        return Err(ImageError::TooLarge {
            pixels,
            max: MAX_PIXELS,
        });
    }
    let expected = (pixels * 4) as usize;
    if rgba.len() != expected {
        return Err(ImageError::BufferLength {
            expected,
            actual: rgba.len(),
        });
    }
    // ravif takes `Img<&[RGBA8]>`; copy into a typed buffer to avoid unsafe/aligned casts. The
    // allocation is bounded by MAX_PIXELS (~64 MiB at 4096²) and is short-lived.
    let pixels: Vec<RGBA8> = rgba
        .chunks_exact(4)
        .map(|px| RGBA8::new(px[0], px[1], px[2], px[3]))
        .collect();
    let image = Img::new(pixels.as_slice(), width as usize, height as usize);
    let encoded = Encoder::new()
        .with_quality(f32::from(quality.clamp(1, 100)))
        .with_speed(speed.clamp(MIN_SPEED, MAX_SPEED))
        .with_bit_depth(ravif::BitDepth::Eight)
        .encode_rgba(image)?;
    Ok(encoded.avif_file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(width: u32, height: u32) -> Vec<u8> {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                rgba.extend_from_slice(&[
                    (x * 255 / width.max(1)) as u8,
                    (y * 255 / height.max(1)) as u8,
                    ((x + y) * 127 / (width + height).max(1)) as u8,
                    if (x + y) % 7 == 0 { 128 } else { 255 },
                ]);
            }
        }
        rgba
    }

    #[test]
    fn quality_and_speed_mapping_matches_presets() {
        assert_eq!(quality_from_fraction(0.28), 28);
        assert_eq!(quality_from_fraction(0.40), 40);
        assert_eq!(quality_from_fraction(0.50), 50);
        assert_eq!(quality_from_fraction(1.0), 100);
        assert_eq!(quality_from_fraction(0.0), 1);
        assert_eq!(quality_from_fraction(f32::NAN), 50);
        assert_eq!(speed_for_quality(28), 10);
        assert_eq!(speed_for_quality(40), 8);
        assert_eq!(speed_for_quality(50), 8);
    }

    #[test]
    fn encodes_a_valid_deterministic_avif() {
        let rgba = gradient(96, 64);
        let first = encode_avif_rgba(&rgba, 96, 64, 40).expect("encode");
        let second = encode_avif_rgba(&rgba, 96, 64, 40).expect("encode");
        assert_eq!(first, second, "AVIF encoding must be deterministic");
        assert!(first.len() > 100);
        assert_eq!(&first[4..12], b"ftypavif", "must be an AVIF container");
        // Structural container checks (no decoder dependency in the crate): ISO-BMFF `ftyp` with
        // the `avif` brand and non-trivial `meta`/`mdat` boxes. Pixel decoding is exercised by the
        // Chromium smoke test in the task report and by the native clients.
        assert!(
            first.windows(4).any(|window| window == b"meta"),
            "AVIF must contain a meta box"
        );
        assert!(
            first.windows(4).any(|window| window == b"mdat"),
            "AVIF must contain a mdat box"
        );
    }

    #[test]
    fn lower_quality_produces_smaller_files() {
        let rgba = gradient(128, 96);
        let low = encode_avif_rgba_with_speed(&rgba, 128, 96, 20, 10).expect("low");
        let high = encode_avif_rgba_with_speed(&rgba, 128, 96, 80, 10).expect("high");
        assert!(
            low.len() < high.len(),
            "quality 20 ({}) must be smaller than quality 80 ({})",
            low.len(),
            high.len()
        );
    }

    #[test]
    fn rejects_invalid_buffers_and_dimensions() {
        assert!(matches!(
            encode_avif_rgba(&[], 0, 10, 40),
            Err(ImageError::ZeroDimensions)
        ));
        assert!(matches!(
            encode_avif_rgba(&[0; 8], 2, 2, 40),
            Err(ImageError::BufferLength {
                expected: 16,
                actual: 8
            })
        ));
        assert!(matches!(
            encode_avif_rgba(&[0; 4], 4096, 4096, 40),
            Err(ImageError::BufferLength { .. } | ImageError::TooLarge { .. })
        ));
    }
}
