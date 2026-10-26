//! pillkit-core: everything about the widget that doesn't depend on the OS.

pub mod envelope;
pub mod model;
pub mod policy;
pub mod render;
pub mod rotation;

pub const PROTOCOL_VERSION: u32 = 1;

pub use ed25519_dalek::VerifyingKey;
