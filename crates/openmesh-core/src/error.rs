//! Error types for the OpenMesh protocol core.

use thiserror::Error;

use crate::wire::NodeId;

/// Errors returned by the protocol core. All variants are non-panicking: hostile or malformed
/// input maps to [`Error::Malformed`], [`Error::BufferTooSmall`] or [`Error::Codec`].
#[derive(Debug, Error)]
pub enum Error {
    /// A buffer was shorter than the minimum length needed.
    #[error("buffer too small: need {need} bytes, have {have}")]
    BufferTooSmall {
        /// Minimum number of bytes required.
        need: usize,
        /// Number of bytes actually available.
        have: usize,
    },

    /// A frame, body or payload is structurally invalid.
    #[error("malformed frame: {0}")]
    Malformed(&'static str),

    /// A frame carried a protocol version this implementation does not support.
    #[error("unsupported protocol version {0}")]
    UnsupportedVersion(u8),

    /// AEAD authentication failed (wrong key, tampered ciphertext or wrong AAD).
    #[error("decryption failed")]
    Decrypt,

    /// An Ed25519 signature did not verify.
    #[error("signature verification failed")]
    Signature,

    /// The peer identity needed for an operation is not in the peer book.
    #[error("unknown node {0}")]
    UnknownNode(NodeId),

    /// A payload exceeds the frame or transfer size limit.
    #[error("payload too large: {size} > {max}")]
    PayloadTooLarge {
        /// Actual payload size in bytes.
        size: usize,
        /// Maximum allowed size in bytes.
        max: usize,
    },

    /// A resource transfer is invalid or incomplete.
    #[error("resource transfer error: {0}")]
    Resource(&'static str),

    /// CBOR (de)serialization failed.
    #[error("codec error: {0}")]
    Codec(String),

    /// An embedder-supplied I/O operation failed.
    #[error("i/o error: {0}")]
    Io(String),

    /// Configuration is invalid or incomplete.
    #[error("invalid configuration: {0}")]
    Config(&'static str),

    /// A feature is not implemented yet.
    #[error("not implemented: {0}")]
    Unimplemented(&'static str),
}

/// Result alias used throughout the crate.
pub type Result<T> = core::result::Result<T, Error>;
