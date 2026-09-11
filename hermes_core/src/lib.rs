//! hermes_core — the shared Rust core of the hermo Hermes mobile client.
//!
//! Clean Architecture layering (Dependency Rule points inward):
//!   framework/driver: tokio, reqwest, tokio-tungstenite, uniffi
//!   adapter:          `json`, `rpc::frames` (wire DTOs and framing)
//!   use case:         `core` (T5, not started in this stream)
//!   entity:           transcript model, session registry, endpoint (T2+)
//!
//! This stream only builds the adapter layer (JSON helpers, frame codec) and
//! the UniFFI scaffolding; nothing below the adapters exists yet.

pub mod json;
pub mod rpc;

uniffi::setup_scaffolding!();