//! The wire between the core and its clients: JSON text frames for control,
//! binary frames for terminal bytes
//! (docs/superpowers/specs/2026-11-26-local-core-design.md §3).

pub mod frame;
pub mod messages;

pub const PROTOCOL_VERSION: u32 = 1;
