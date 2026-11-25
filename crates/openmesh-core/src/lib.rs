//! # openmesh-core
//!
//! The OpenMesh protocol core: wire format, cryptography, managed flooding, resource transfer,
//! store-and-forward and the application message schemas. Everything here is deterministic and
//! IO-free so the same code powers the desktop/mobile bridge, the CLI, and the mesh simulator.
//!
//! The C++ firmware in `firmware/` implements the same wire format; `protocol/vectors/` holds
//! golden vectors that both implementations must pass.
//!
//! ```no_run
//! use openmesh_core::{Engine, EngineConfig, Identity, Target, util};
//!
//! let me = Identity::from_seed(&[42u8; 32]);
//! let mut engine = Engine::new(me, EngineConfig::client("my-node"));
//! let now = util::now_ms();
//! let _ = engine.announce(now).unwrap();
//! let _ = engine.send_text(Target::Direct(openmesh_core::NodeId([1; 8])), "hello", now).unwrap();
//! ```

#![warn(missing_docs)]
#![warn(rust_2018_idioms)]
#![forbid(unsafe_code)]

pub mod companion;
pub mod crypto;
pub mod engine;
pub mod error;
pub mod fragment;
pub mod resource;
pub mod routing;
pub mod services;
pub mod store;
pub mod util;
pub mod wire;

pub use crypto::{ChannelKey, Identity, PublicIdentity};
pub use engine::{
    engine_from_seed, Engine, EngineConfig, Event, LinkId, LinkInfo, NodeInfo, Outbound,
    Processed, ReceivedGroupMessage, ReceivedMessage, RelayConfig, Role, SendMode, StateSnapshot,
    Stats, Target,
};
pub use error::{Error, Result};
pub use services::{
    Ack, AckKind, Announce, AppMessage, AttachmentRef, MessageKind, NodeClass, Position,
    ResourceKind, ResourceManifest, Telemetry,
};
pub use wire::{Flags, Frame, FrameType, NodeId, BROADCAST, HEADER_LEN, MAX_BODY, MTU, PROTO_VERSION};

/// Protocol version string used in announces, companion handshakes and the API.
pub const VERSION_STR: &str = concat!("omp/", "1");
/// Human-readable crate version.
pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");
