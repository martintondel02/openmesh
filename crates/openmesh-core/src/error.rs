//! Error types for the OpenMesh protocol core.

use thiserror::Error;

use crate::wire::NodeId;

#[derive(Debug, Error)]
pub enum Error {
    #[error("buffer too small: need {need} bytes, have {have}")]
    BufferTooSmall { need: usize, have: usize },

    #[error("malformed frame: {0}")]
    Malformed(&'static str),

    #[error("unsupported protocol version {0}")]
    UnsupportedVersion(u8),

    #[error("decryption failed")]
    Decrypt,

    #[error("signature verification failed")]
    Signature,

    #[error("unknown node {0}")]
    UnknownNode(NodeId),

    #[error("payload too large: {size} > {max}")]
    PayloadTooLarge { size: usize, max: usize },

    #[error("resource transfer error: {0}")]
    Resource(&'static str),

    #[error("codec error: {0}")]
    Codec(String),

    #[error("i/o error: {0}")]
    Io(String),

    #[error("invalid configuration: {0}")]
    Config(&'static str),

    #[error("not implemented: {0}")]
    Unimplemented(&'static str),
}

pub type Result<T> = core::result::Result<T, Error>;
