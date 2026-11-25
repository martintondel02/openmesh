//! OpenMesh FFI surface for Android/iOS clients.
//!
//! The final interface is generated with UniFFI; this placeholder keeps the crate buildable
//! while the Android client is scaffolded against `docs/architecture/COMPANION-API.md`.

/// Returns the version string of the linked protocol core.
pub fn version() -> String {
    format!("openmesh-core {}", openmesh_core::CRATE_VERSION)
}
