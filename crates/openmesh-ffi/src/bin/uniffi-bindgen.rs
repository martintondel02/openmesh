//! Generates Kotlin/Swift bindings from the compiled `openmesh-ffi` library.
//!
//! ```text
//! cargo run -p openmesh-ffi --bin uniffi-bindgen -- \
//!     generate --library target/debug/libopenmesh_ffi.so \
//!     --language kotlin --out-dir /tmp/ffi-kotlin
//! ```

fn main() {
    uniffi::uniffi_bindgen_main();
}
