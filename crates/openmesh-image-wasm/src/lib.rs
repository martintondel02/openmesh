//! # openmesh-image-wasm — C ABI for the web client's bundled AVIF encoder
//!
//! Mirrors the `openmesh-voice-wasm` pattern (`docs/design/media-tiers.md` §3): the JS loader
//! copies RGBA bytes into wasm memory, calls [`om_image_encode`], reads `out_len` bytes back and
//! calls [`om_image_free`]. Errors return a NULL pointer with `*out_len = 0`.
//!
//! The module is loaded lazily by the web client on the first image send (it is ~1.5 MB raw /
//! ~415 KB gzip-9) and encoding runs in a Worker; the existing canvas WebP/JPEG path remains the
//! fallback when the module cannot be loaded or encoding fails. Never label fallback bytes as
//! `image/avif`.
//!
//! The wasm build has four tiny `wasm-bindgen` externref stubs required by rav1e's wasm support;
//! the loader can provide no-op imports (see `test-wasm.mjs`):
//! `__wbindgen_externref_xform__::{__wbindgen_externref_statics_end,
//! __wbindgen_externref_table_set_null, __wbindgen_externref_table_grow}` and
//! `__wbindgen_placeholder__::__wbindgen_describe`.

use std::slice;

use openmesh_image::{encode_avif_rgba_with_speed, quality_from_fraction, speed_for_quality};

/// Host-testable encode wrapper (same path as the C ABI).
pub fn encode_bytes(
    rgba: &[u8],
    width: u32,
    height: u32,
    quality: u8,
    speed: u8,
) -> Result<Vec<u8>, openmesh_image::ImageError> {
    encode_avif_rgba_with_speed(rgba, width, height, quality, speed)
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

/// Allocate `len` bytes in the wasm heap (the JS loader writes RGBA input here).
///
/// # Safety
/// The caller owns the returned buffer until [`om_image_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn om_image_alloc(len: u32) -> *mut u8 {
    let mut buffer = vec![0u8; len as usize].into_boxed_slice();
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr
}

/// Free a buffer returned by [`om_image_alloc`] or [`om_image_encode`].
///
/// # Safety
/// `ptr`/`len` must come from this module.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn om_image_free(ptr: *mut u8, len: u32) {
    if !ptr.is_null() {
        drop(unsafe { Vec::from_raw_parts(ptr, len as usize, len as usize) });
    }
}

/// Quality mapping helper for JS (normalized preset fraction → ravif 1–100).
#[unsafe(no_mangle)]
pub extern "C" fn om_image_quality_from_fraction(fraction: f32) -> u8 {
    quality_from_fraction(fraction)
}

/// Speed mapping helper for JS (ravif quality → default speed 7/8/10).
#[unsafe(no_mangle)]
pub extern "C" fn om_image_speed_for_quality(quality: u8) -> u8 {
    speed_for_quality(quality)
}

/// Encode RGBA8 pixels to AVIF. Returns a heap buffer and writes its length to `out_len`.
///
/// `quality` is 1–100 (preset tiers 28/40/50); `speed` is 1–10 (preset tiers 10/8/7). Returns NULL
/// (and `*out_len = 0`) for invalid pointers/buffers or encoder failure.
///
/// # Safety
/// `rgba` must point to `width * height * 4` readable bytes; `out_len` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn om_image_encode(
    rgba: *const u8,
    width: u32,
    height: u32,
    quality: u8,
    speed: u8,
    out_len: *mut u32,
) -> *mut u8 {
    if out_len.is_null() || rgba.is_null() || width == 0 || height == 0 {
        return error_buffer(out_len);
    }
    unsafe { *out_len = 0 };
    let Some(pixel_count) = (width as usize).checked_mul(height as usize) else {
        return error_buffer(out_len);
    };
    let Some(byte_len) = pixel_count.checked_mul(4) else {
        return error_buffer(out_len);
    };
    let pixels = unsafe { slice::from_raw_parts(rgba, byte_len) };
    match encode_bytes(pixels, width, height, quality, speed) {
        Ok(encoded) => into_c_buffer(encoded, out_len),
        Err(_) => error_buffer(out_len),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(width: u32, height: u32) -> Vec<u8> {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                rgba.extend_from_slice(&[x as u8, y as u8, (x ^ y) as u8, 255]);
            }
        }
        rgba
    }

    #[test]
    fn encode_bytes_matches_the_wasm_path_and_is_deterministic() {
        let rgba = gradient(48, 32);
        let first = encode_bytes(&rgba, 48, 32, 40, 8).expect("encode");
        let second = encode_bytes(&rgba, 48, 32, 40, 8).expect("encode");
        assert_eq!(first, second);
        assert_eq!(&first[4..12], b"ftypavif");
        assert_eq!(om_image_quality_from_fraction(0.28), 28);
        assert_eq!(om_image_speed_for_quality(40), 8);
    }

    #[test]
    fn encode_bytes_rejects_bad_buffers() {
        assert!(encode_bytes(&[], 0, 4, 40, 8).is_err());
        assert!(encode_bytes(&[0; 8], 2, 2, 40, 8).is_err());
    }
}
