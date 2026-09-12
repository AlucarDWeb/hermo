//! Transcript entity layer (PLAN §4 T4): the pure model + reducer that turn
//! decoded gateway events into a renderable, incrementally-updatable
//! transcript, multi-session from birth (PLAN decision 10).
//!
//! Clean Architecture: this is the ENTITY layer — plain data and rules only.
//! No I/O, no tokio, no clocks, no randomness. Reducer input is the decoded
//! [`EventParams`](crate::protocol::EventParams) DTO, never the raw frame —
//! and never the codec module: `rpc::frames` is an adapter, so the DTO lives
//! in the neutral [`crate::protocol`] module both sides import.

pub mod markdown;
pub mod model;
pub mod reducer;
