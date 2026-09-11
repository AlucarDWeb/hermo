//! hermes_core — the shared Rust core of the hermo Hermes mobile client.
//!
//! Clean Architecture layering (Dependency Rule points inward):
//!   framework/driver: tokio, reqwest, tokio-tungstenite, uniffi
//!   adapter:          `json`, `rpc::frames` (wire DTOs and framing)
//!   use case:         `core` (T5, not started in this stream)
//!   entity:           transcript model, session registry, endpoint (T2+)
//!
//! This stream builds the adapter layer (JSON helpers, frame codec, typed
//! RPC wrappers) and the T2 transport driver: the WebSocket gateway client
//! with heartbeat, sticky close reasons and replay seq watermarks, plus the
//! desktop probe binary. The use-case/entity layers (transcript model,
//! session registry) arrive with T4/T5.

pub mod json;
pub mod rpc;

uniffi::setup_scaffolding!();