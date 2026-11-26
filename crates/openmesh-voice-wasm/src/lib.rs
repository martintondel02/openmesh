//! # openmesh-voice-wasm — C ABI for the web client
//!
//! Minimal, allocation-aware wrapper around [`openmesh_voice`] for `wasm32-unknown-unknown`
//! (`docs/design/media-tiers.md` §3). The JS loader copies PCM/payload bytes into wasm memory,
//! calls `om_voice_encode`/`om_voice_decode`, reads `out_len` bytes back and calls
//! `om_voice_free(ptr, len)`. Errors return a NULL pointer with `*out_len = 0`.
//!
//! `tier`: 0 = tiny (Codec2 1300), 1 = medium (Codec2 3200), 2 = high (rejected; platform Opus).

use std::slice;

use openmesh_voice::{VoiceError, VoiceTier, decode, encode, frame_bytes};

/// Map the C ABI tier integer to a [`VoiceTier`].
pub fn tier_from_u32(tier: u32) -> Option<VoiceTier> {
    match tier {
        0 => Some(VoiceTier::Tiny),
        1 => Some(VoiceTier::Medium),
        2 => Some(VoiceTier::High),
        _ => None,
    }
}

/// Host-testable encode wrapper (same path as the C ABI).
pub fn encode_bytes(tier: u32, pcm_s16le: &[u8]) -> Result<Vec<u8>, VoiceError> {
    let tier = tier_from_u32(tier).ok_or(VoiceError::UnsupportedTier)?;
    encode(tier, pcm_s16le)
}

/// Host-testable decode wrapper (same path as the C ABI).
pub fn decode_bytes(tier: u32, payload: &[u8]) -> Result<Vec<u8>, VoiceError> {
    let tier = tier_from_u32(tier).ok_or(VoiceError::UnsupportedTier)?;
    decode(tier, payload)
}

fn into_c_buffer(mut data: Vec<u8>, out_len: *mut u32) -> *mut u8 {
    if data.is_empty() {
        return std::ptr::null_mut();
    }
    let len = data.len();
    let ptr = data.as_mut_ptr();
    std::mem::forget(data);
    unsafe {
        *out_len = len as u32;
    }
    ptr
}

fn error_buffer(out_len: *mut u32) -> *mut u8 {
    if !out_len.is_null() {
        unsafe {
            *out_len = 0;
        }
    }
    std::ptr::null_mut()
}

/// Encode 8 kHz mono s16le PCM into raw Codec2 frames. Returns NULL on error.
///
/// # Safety
/// `pcm` must point to `len` readable bytes (or be NULL when `len == 0`); `out_len` must be a
/// valid writable `u32` pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn om_voice_encode(
    tier: u32,
    pcm: *const u8,
    len: u32,
    out_len: *mut u32,
) -> *mut u8 {
    if out_len.is_null() {
        return std::ptr::null_mut();
    }
    if pcm.is_null() {
        return error_buffer(out_len);
    }
    let input = unsafe { slice::from_raw_parts(pcm, len as usize) };
    match encode_bytes(tier, input) {
        Ok(encoded) => into_c_buffer(encoded, out_len),
        Err(_) => error_buffer(out_len),
    }
}

/// Decode raw Codec2 frames to 8 kHz mono s16le PCM. Returns NULL on error.
///
/// # Safety
/// `payload` must point to `len` readable bytes (or be NULL when `len == 0`); `out_len` must be a
/// valid writable `u32` pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn om_voice_decode(
    tier: u32,
    payload: *const u8,
    len: u32,
    out_len: *mut u32,
) -> *mut u8 {
    if out_len.is_null() {
        return std::ptr::null_mut();
    }
    if payload.is_null() {
        return error_buffer(out_len);
    }
    let input = unsafe { slice::from_raw_parts(payload, len as usize) };
    match decode_bytes(tier, input) {
        Ok(pcm) => into_c_buffer(pcm, out_len),
        Err(_) => error_buffer(out_len),
    }
}

/// Free a buffer previously returned by `om_voice_encode`/`om_voice_decode`.
///
/// # Safety
/// `ptr`/`len` must be exactly the values returned by this module (and not freed twice).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn om_voice_free(ptr: *mut u8, len: u32) {
    if ptr.is_null() || len == 0 {
        return;
    }
    let slice = unsafe { slice::from_raw_parts_mut(ptr, len as usize) };
    unsafe {
        drop(Box::from_raw(slice as *mut [u8]));
    }
}

/// Encoded frame size in bytes for a tier (7 tiny, 8 medium, 0 high/invalid).
#[unsafe(no_mangle)]
pub extern "C" fn om_voice_frame_bytes(tier: u32) -> u32 {
    tier_from_u32(tier)
        .map(|tier| frame_bytes(tier) as u32)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(duration_ms: u64) -> Vec<u8> {
        let samples = (8000 * duration_ms / 1000) as usize;
        let mut pcm = Vec::with_capacity(samples * 2);
        for index in 0..samples {
            let t = index as f64 / 8000.0;
            let value = (2.0 * std::f64::consts::PI * 440.0 * t).sin() * 12_000.0;
            pcm.extend_from_slice(&(value as i16).to_le_bytes());
        }
        pcm
    }

    #[test]
    fn host_round_trip_via_c_abi() {
        for (tier, bytes_per_second) in [(0u32, 175u32), (1, 400)] {
            let pcm = sine(1_000);
            let mut encoded_len = 0u32;
            let encoded_ptr =
                unsafe { om_voice_encode(tier, pcm.as_ptr(), pcm.len() as u32, &mut encoded_len) };
            assert!(!encoded_ptr.is_null());
            assert_eq!(encoded_len, bytes_per_second);
            let encoded =
                unsafe { slice::from_raw_parts(encoded_ptr, encoded_len as usize) }.to_vec();
            unsafe { om_voice_free(encoded_ptr, encoded_len) };

            let mut pcm_len = 0u32;
            let pcm_ptr = unsafe {
                om_voice_decode(tier, encoded.as_ptr(), encoded.len() as u32, &mut pcm_len)
            };
            assert!(!pcm_ptr.is_null());
            assert_eq!(pcm_len as usize, pcm.len());
            let decoded = unsafe { slice::from_raw_parts(pcm_ptr, pcm_len as usize) }.to_vec();
            unsafe { om_voice_free(pcm_ptr, pcm_len) };
            assert!(decoded.iter().any(|byte| *byte != 0));
        }
        assert_eq!(om_voice_frame_bytes(0), 7);
        assert_eq!(om_voice_frame_bytes(1), 8);
        assert_eq!(om_voice_frame_bytes(2), 0);
        assert_eq!(om_voice_frame_bytes(9), 0);
    }

    #[test]
    fn errors_return_null_and_zero_length() {
        let mut out_len = 99u32;
        let ptr = unsafe { om_voice_encode(9, std::ptr::null(), 0, &mut out_len) };
        assert!(ptr.is_null());
        assert_eq!(out_len, 0);
        let pcm = sine(1_000);
        out_len = 99;
        // High tier is rejected (platform Opus recorder).
        let ptr = unsafe { om_voice_encode(2, pcm.as_ptr(), pcm.len() as u32, &mut out_len) };
        assert!(ptr.is_null());
        assert_eq!(out_len, 0);
    }
}
