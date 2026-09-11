//! Auth layer (PLAN.md §3): entity (`endpoint`) + adapter (`client`).
//!
//! `test_http_server` is compiled only under `cfg(test)` — the probe and
//! the library never carry the fake backend.

pub mod endpoint;
pub mod client;

#[cfg(test)]
pub(crate) mod test_http_server;
