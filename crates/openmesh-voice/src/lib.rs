//! # openmesh-voice — tiny/medium voice codecs for OpenMesh
//!
//! Pure-Rust [Codec2](https://crates.io/crates/codec2) raw-frame voice, shared by the bridge,
//! the WASM web client and the Android FFI so all clients agree on wire bytes, MIME types and
//! duration/size math. Normative contract: `docs/design/media-tiers.md` §2.
//!
//! * **tiny** — Codec2 1300: 320 samples (40 ms) → 52 bits = 7 bytes, 175 B/s, `audio/c2-1300`.
//! * **medium** — Codec2 3200: 160 samples (20 ms) → 64 bits = 8 bytes, 400 B/s, `audio/c2-3200`.
//! * **high** — platform Opus/AAC (not encoded here): `UnsupportedTier`.
//!
//! Payloads are **raw codec frames**, no container: encode consumes whole 8 kHz mono s16le PCM
//! frames only, decode ignores a trailing partial payload frame. There is no IO in this crate.

use codec2::{Codec2, Codec2Mode};

/// Voice quality tier. `High` is recorded by platform codecs and is not handled by this crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VoiceTier {
    /// Codec2 1300 bps: smallest payload, most robust to bad channels.
    Tiny,
    /// Codec2 3200 bps: better intelligibility, twice the airtime of tiny.
    Medium,
    /// Platform Opus/AAC (`audio/ogg; codecs=opus`); not encoded/decoded here.
    High,
}

impl VoiceTier {
    /// Stable lowercase identifier used by clients, FFI and the JSON tier table.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tiny => "tiny",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Tiny => 0,
            Self::Medium => 1,
            Self::High => 2,
        }
    }

    fn codec2_mode(self) -> Result<Codec2Mode, VoiceError> {
        match self {
            Self::Tiny => Ok(Codec2Mode::MODE_1300),
            Self::Medium => Ok(Codec2Mode::MODE_3200),
            Self::High => Err(VoiceError::UnsupportedTier),
        }
    }
}

/// Errors returned by encode/decode and tier helpers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoiceError {
    /// The selected tier is not handled by this crate (`high` = platform Opus/AAC).
    UnsupportedTier,
    /// The input contained no complete frame/sample to process.
    EmptyAudio,
    /// The payload/audio exceeded the configured limit.
    TooLarge,
    /// The codec rejected the frames.
    Decode(String),
}

impl core::fmt::Display for VoiceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnsupportedTier => {
                write!(f, "tier 'high' uses the platform Opus/AAC recorder")
            }
            Self::EmptyAudio => write!(f, "no audio frames to process"),
            Self::TooLarge => write!(f, "audio payload is too large"),
            Self::Decode(message) => write!(f, "voice decode failed: {message}"),
        }
    }
}

impl std::error::Error for VoiceError {}

/// One voice tier's wire/format metadata (matches `content/presets/media.json`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceFormat {
    /// Quality tier.
    pub tier: VoiceTier,
    /// Codec name (`codec2` or `opus`).
    pub codec: &'static str,
    /// Codec mode (`1300`, `3200` or `opus`).
    pub mode: &'static str,
    /// MIME type used on the attachment.
    pub mime: &'static str,
    /// Sample rate of the PCM/codec stream.
    pub sample_rate: u32,
    /// Channel count (always 1 today).
    pub channels: u8,
    /// One codec frame in milliseconds.
    pub frame_ms: u32,
    /// Codec bit rate in bits per second.
    pub bits_per_second: u32,
    /// Hard duration cap for this tier in seconds.
    pub max_duration_s: u32,
}

impl VoiceFormat {
    /// Encoded payload bytes per second (frame-rounded; e.g. tiny 7 B/40 ms = 175 B/s).
    pub fn bytes_per_second(&self) -> u32 {
        match self.tier {
            VoiceTier::Tiny => 175,
            VoiceTier::Medium => 400,
            VoiceTier::High => self.bits_per_second / 8,
        }
    }
}

const FORMATS: [VoiceFormat; 3] = [
    VoiceFormat {
        tier: VoiceTier::Tiny,
        codec: "codec2",
        mode: "1300",
        mime: "audio/c2-1300",
        sample_rate: 8000,
        channels: 1,
        frame_ms: 40,
        bits_per_second: 1300,
        max_duration_s: 30,
    },
    VoiceFormat {
        tier: VoiceTier::Medium,
        codec: "codec2",
        mode: "3200",
        mime: "audio/c2-3200",
        sample_rate: 8000,
        channels: 1,
        frame_ms: 20,
        bits_per_second: 3200,
        max_duration_s: 20,
    },
    VoiceFormat {
        tier: VoiceTier::High,
        codec: "opus",
        mode: "opus",
        mime: "audio/ogg; codecs=opus",
        sample_rate: 48000,
        channels: 1,
        frame_ms: 20,
        bits_per_second: 12000,
        max_duration_s: 15,
    },
];

/// Format metadata for one tier.
pub fn format(tier: VoiceTier) -> &'static VoiceFormat {
    &FORMATS[tier.index()]
}

/// All supported tiers in wire order (`tiny`, `medium`, `high`).
pub fn formats() -> &'static [VoiceFormat] {
    &FORMATS
}

/// Encoded payload bytes per second for one tier.
pub fn bytes_per_second(tier: VoiceTier) -> u32 {
    format(tier).bytes_per_second()
}

/// Estimated encoded payload size for a recording duration (frames rounded up).
pub fn estimate_bytes(tier: VoiceTier, duration_ms: u64) -> usize {
    if duration_ms == 0 {
        return 0;
    }
    let frame_ms = u64::from(format(tier).frame_ms.max(1));
    let frames = duration_ms.div_ceil(frame_ms);
    (frames as usize).saturating_mul(frame_bytes(tier))
}

/// PCM bytes for a recording duration at the tier's sample rate/channels (8 kHz mono s16le for
/// Codec2 tiers).
pub fn pcm_bytes_for(tier: VoiceTier, duration_ms: u64) -> usize {
    let format = format(tier);
    let bytes_per_sample = 2u64 * u64::from(format.channels);
    (u64::from(format.sample_rate) * bytes_per_sample * duration_ms / 1000) as usize
}

/// Effective duration cap in seconds: `min(tier.max_duration_s, floor(cap / bytes_per_second))`.
pub fn max_duration_s(tier: VoiceTier, profile_cap_bytes: usize) -> u32 {
    let by_cap = (profile_cap_bytes as u64) / u64::from(bytes_per_second(tier).max(1));
    u32::try_from(by_cap)
        .unwrap_or(u32::MAX)
        .min(format(tier).max_duration_s)
}

/// Encoded frame size in bytes for a tier (7 tiny, 8 medium; 0 for high).
pub fn frame_bytes(tier: VoiceTier) -> usize {
    match tier {
        VoiceTier::Tiny => 7,
        VoiceTier::Medium => 8,
        VoiceTier::High => 0,
    }
}

fn pcm_frame_bytes(tier: VoiceTier) -> usize {
    let format = format(tier);
    let samples = match tier {
        VoiceTier::Tiny => 320,
        VoiceTier::Medium => 160,
        VoiceTier::High => 0,
    };
    samples * usize::from(format.channels) * 2
}

/// Encode 8 kHz mono s16le PCM into the tier's raw Codec2 frame stream.
///
/// * `high` → [`VoiceError::UnsupportedTier`] (clients use the platform recorder).
/// * empty/no whole frame → [`VoiceError::EmptyAudio`].
/// * a trailing partial sample/frame is dropped (whole-frame truncation).
pub fn encode(tier: VoiceTier, pcm_s16le: &[u8]) -> Result<Vec<u8>, VoiceError> {
    let mode = tier.codec2_mode()?;
    if pcm_s16le.is_empty() {
        return Err(VoiceError::EmptyAudio);
    }
    let sample_bytes = pcm_s16le.len() - (pcm_s16le.len() % 2);
    let frame_pcm_bytes = pcm_frame_bytes(tier);
    let frames = sample_bytes / frame_pcm_bytes;
    if frames == 0 {
        return Err(VoiceError::EmptyAudio);
    }
    let mut codec = Codec2::new(mode);
    let frame_bytes = codec.bits_per_frame().div_ceil(8);
    let mut out = Vec::with_capacity(frames * frame_bytes);
    let mut bits = vec![0u8; frame_bytes];
    let mut samples = Vec::with_capacity(frame_pcm_bytes / 2);
    for frame in 0..frames {
        let start = frame * frame_pcm_bytes;
        samples.clear();
        samples.extend(
            pcm_s16le[start..start + frame_pcm_bytes]
                .chunks_exact(2)
                .map(|pair| i16::from_le_bytes([pair[0], pair[1]])),
        );
        bits.fill(0);
        codec.encode(&mut bits, &samples);
        out.extend_from_slice(&bits);
    }
    Ok(out)
}

/// Decode a raw Codec2 frame stream back to 8 kHz mono s16le PCM.
///
/// * `high` → [`VoiceError::UnsupportedTier`].
/// * empty / no complete frame → [`VoiceError::EmptyAudio`].
/// * a trailing partial frame is ignored.
pub fn decode(tier: VoiceTier, payload: &[u8]) -> Result<Vec<u8>, VoiceError> {
    let mode = tier.codec2_mode()?;
    if payload.is_empty() {
        return Err(VoiceError::EmptyAudio);
    }
    let frame_bytes = frame_bytes(tier);
    let frames = payload.len() / frame_bytes;
    if frames == 0 {
        return Err(VoiceError::EmptyAudio);
    }
    let mut codec = Codec2::new(mode);
    let samples_per_frame = codec.samples_per_frame();
    let mut pcm = vec![0i16; samples_per_frame];
    let mut out = Vec::with_capacity(frames * samples_per_frame * 2);
    for frame in 0..frames {
        let start = frame * frame_bytes;
        pcm.fill(0);
        codec.decode(&mut pcm, &payload[start..start + frame_bytes]);
        for sample in &pcm {
            out.extend_from_slice(&sample.to_le_bytes());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(duration_ms: u64, frequency: f64) -> Vec<u8> {
        let sample_rate = 8000.0;
        let samples = (sample_rate * duration_ms as f64 / 1000.0) as usize;
        let mut pcm = Vec::with_capacity(samples * 2);
        for index in 0..samples {
            let t = index as f64 / sample_rate;
            let value = (2.0 * std::f64::consts::PI * frequency * t).sin() * 12_000.0;
            pcm.extend_from_slice(&(value as i16).to_le_bytes());
        }
        pcm
    }

    fn rms(pcm_s16le: &[u8]) -> f64 {
        let samples: Vec<f64> = pcm_s16le
            .chunks_exact(2)
            .map(|pair| f64::from(i16::from_le_bytes([pair[0], pair[1]])))
            .collect();
        if samples.is_empty() {
            return 0.0;
        }
        (samples.iter().map(|sample| sample * sample).sum::<f64>() / samples.len() as f64).sqrt()
    }

    #[test]
    fn formats_match_the_media_tiers_contract() {
        assert_eq!(formats().len(), 3);
        assert_eq!(format(VoiceTier::Tiny).mime, "audio/c2-1300");
        assert_eq!(format(VoiceTier::Tiny).frame_ms, 40);
        assert_eq!(format(VoiceTier::Tiny).max_duration_s, 30);
        assert_eq!(format(VoiceTier::Medium).mime, "audio/c2-3200");
        assert_eq!(format(VoiceTier::Medium).frame_ms, 20);
        assert_eq!(format(VoiceTier::Medium).max_duration_s, 20);
        assert_eq!(format(VoiceTier::High).mime, "audio/ogg; codecs=opus");
        assert_eq!(format(VoiceTier::High).sample_rate, 48_000);
        assert_eq!(format(VoiceTier::Tiny).tier, VoiceTier::Tiny);
        assert_eq!(VoiceTier::Tiny.as_str(), "tiny");
        assert_eq!(VoiceTier::High.as_str(), "high");
    }

    #[test]
    fn tiny_and_medium_round_trip_non_silent_audio() {
        for (tier, expected_bytes) in [(VoiceTier::Tiny, 175usize), (VoiceTier::Medium, 400)] {
            let pcm = sine(1_000, 440.0);
            let encoded = encode(tier, &pcm).expect("encode");
            assert_eq!(encoded.len(), expected_bytes, "{tier:?} one second");
            assert!(encoded.len() % frame_bytes(tier) == 0);
            let decoded = decode(tier, &encoded).expect("decode");
            assert_eq!(decoded.len(), pcm.len(), "{tier:?} decoded PCM length");
            assert!(rms(&decoded) > 0.0, "{tier:?} decoded audio is silent");
        }
    }

    #[test]
    fn noise_round_trip_is_non_silent() {
        let mut pcm = Vec::with_capacity(32_000);
        let mut state = 0x1234_5678u32;
        for _ in 0..16_000 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let sample = (state >> 16) as i16 / 2;
            pcm.extend_from_slice(&sample.to_le_bytes());
        }
        for tier in [VoiceTier::Tiny, VoiceTier::Medium] {
            let encoded = encode(tier, &pcm).expect("encode noise");
            let decoded = decode(tier, &encoded).expect("decode noise");
            assert!(rms(&decoded) > 0.0, "{tier:?} noise decode silent");
        }
    }

    #[test]
    fn byte_sizes_for_one_and_eight_seconds() {
        let one_second = sine(1_000, 440.0);
        let eight_seconds = sine(8_000, 300.0);
        assert_eq!(encode(VoiceTier::Tiny, &one_second).unwrap().len(), 175);
        assert_eq!(encode(VoiceTier::Medium, &one_second).unwrap().len(), 400);
        assert_eq!(
            encode(VoiceTier::Tiny, &eight_seconds).unwrap().len(),
            1_400
        );
        assert_eq!(
            encode(VoiceTier::Medium, &eight_seconds).unwrap().len(),
            3_200
        );
        assert_eq!(bytes_per_second(VoiceTier::Tiny), 175);
        assert_eq!(bytes_per_second(VoiceTier::Medium), 400);
        assert_eq!(bytes_per_second(VoiceTier::High), 1_500);
    }

    #[test]
    fn estimate_and_pcm_math() {
        assert_eq!(estimate_bytes(VoiceTier::Tiny, 0), 0);
        assert_eq!(estimate_bytes(VoiceTier::Tiny, 1_000), 175);
        assert_eq!(estimate_bytes(VoiceTier::Medium, 1_000), 400);
        assert_eq!(estimate_bytes(VoiceTier::Tiny, 8_000), 1_400);
        assert_eq!(estimate_bytes(VoiceTier::Medium, 8_000), 3_200);
        // A duration that is not a whole frame rounds up.
        assert_eq!(estimate_bytes(VoiceTier::Tiny, 40), 7);
        assert_eq!(estimate_bytes(VoiceTier::Tiny, 41), 14);
        assert_eq!(pcm_bytes_for(VoiceTier::Tiny, 1_000), 16_000);
        assert_eq!(pcm_bytes_for(VoiceTier::Medium, 8_000), 128_000);
        assert_eq!(pcm_bytes_for(VoiceTier::High, 1_000), 96_000);
        // Duration caps: tier cap vs profile attachment cap.
        assert_eq!(max_duration_s(VoiceTier::Tiny, 1_000_000), 30);
        assert_eq!(max_duration_s(VoiceTier::Tiny, 1_750), 10);
        assert_eq!(max_duration_s(VoiceTier::Medium, 800), 2);
        assert_eq!(max_duration_s(VoiceTier::High, 100_000), 15);
    }

    #[test]
    fn high_tier_is_unsupported_but_described() {
        let pcm = sine(1_000, 440.0);
        assert_eq!(
            encode(VoiceTier::High, &pcm).unwrap_err(),
            VoiceError::UnsupportedTier
        );
        assert_eq!(
            decode(VoiceTier::High, &[0u8; 8]).unwrap_err(),
            VoiceError::UnsupportedTier
        );
        assert_eq!(format(VoiceTier::High).codec, "opus");
    }

    #[test]
    fn empty_short_and_partial_input_handling() {
        assert_eq!(
            encode(VoiceTier::Tiny, &[]).unwrap_err(),
            VoiceError::EmptyAudio
        );
        assert_eq!(
            decode(VoiceTier::Tiny, &[]).unwrap_err(),
            VoiceError::EmptyAudio
        );
        // Less than one whole PCM frame → nothing to encode.
        assert_eq!(
            encode(VoiceTier::Tiny, &[0u8; 100]).unwrap_err(),
            VoiceError::EmptyAudio
        );
        // Less than one whole payload frame → nothing to decode.
        assert_eq!(
            decode(VoiceTier::Tiny, &[0u8; 3]).unwrap_err(),
            VoiceError::EmptyAudio
        );

        // Trailing partial PCM byte is dropped: 1 s + 1 odd byte encodes exactly like 1 s.
        let pcm = sine(1_000, 440.0);
        let mut padded = pcm.clone();
        let mut odd = pcm.clone();
        odd.push(0xAA);
        let exact = encode(VoiceTier::Tiny, &pcm).unwrap();
        assert_eq!(encode(VoiceTier::Tiny, &odd).unwrap(), exact);
        padded.truncate(pcm.len() - 300); // not a whole tiny frame (640 B)
        assert_eq!(
            encode(VoiceTier::Tiny, &padded).unwrap().len(),
            (padded.len() / 640) * 7
        );

        // Trailing partial payload frame is ignored by the decoder.
        let mut payload = exact.clone();
        payload.extend_from_slice(&[0x11, 0x22, 0x33]);
        // Codec2 randomises unvoiced phase, so decodes are not bit-identical across calls; the
        // contract only requires the trailing partial frame to be ignored.
        let decoded_exact = decode(VoiceTier::Tiny, &exact).unwrap();
        let decoded_partial = decode(VoiceTier::Tiny, &payload).unwrap();
        assert_eq!(
            decoded_exact.len(),
            decoded_partial.len(),
            "length mismatch"
        );
        let exact_rms = rms(&decoded_exact);
        let partial_rms = rms(&decoded_partial);
        assert!(exact_rms > 0.0 && partial_rms > 0.0);
        assert!(
            (partial_rms / exact_rms - 1.0).abs() < 0.5,
            "partial-frame decode energy drifted: {exact_rms} vs {partial_rms}"
        );
    }
}
