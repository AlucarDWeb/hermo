//! hermes_core — the shared Rust core of the hermo Hermes mobile client.
//!
//! Clean Architecture layering (Dependency Rule points inward):
//!   framework/driver: tokio, reqwest, tokio-tungstenite, uniffi
//!   adapter:          `rpc::client` (WebSocket), `rpc::frames` (codec),
//!                     `auth::client` (HTTP + cookie jar)
//!   neutral:          `json` (serde_json helpers), `protocol` (DTOs)
//!   use case:         `core` (T5, not started in this stream)
//!   entity:           `transcript` (model + reducer + markdown),
//!                     `auth::endpoint`, `error`
//!
//! `json` and `protocol` are shared value-object/helper modules both the
//! adapters and the entity import (PLAN §3: cross-boundary data is plain
//! DTOs). They hold no I/O and no framework types, and `tests/layering_guard.rs`
//! keeps them that way — as it keeps the entity from importing `rpc`/`auth::client`
//! (review #4, should 1).

pub mod json;
pub mod protocol;
pub mod rpc;
pub mod auth;
pub mod error;
pub mod transcript;

uniffi::setup_scaffolding!();
