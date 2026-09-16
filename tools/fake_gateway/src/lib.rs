//! Fake Hermes gateway: HTTP auth routes and a WebSocket JSON-RPC endpoint that
//! replays recorded fixture turns, so the app can be driven end to end without a
//! live gateway. Exposed as a library so the smoke test drives the same server
//! the binary runs.

pub mod fixtures;
pub mod server;
