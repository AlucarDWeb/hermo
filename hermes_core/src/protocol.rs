//! Protocol value objects shared across the layering boundary (PLAN §3:
//! "cross-boundary data = plain DTOs/value objects").
//!
//! These types carry **no behaviour and no I/O**: they are the decoded shapes
//! the codec produces and the transcript entity consumes. They live in their
//! own module on purpose — with `EventParams` inside `rpc::frames` (the
//! adapter/codec module) the entity had to `use crate::rpc::frames::…`, i.e.
//! an inward-pointing edge from entity to adapter, which is exactly the class
//! of violation PR #3 flagged for `CoreError` and `ClientError`.
//!
//! Dependency direction after the move:
//! `rpc::frames` (codec) -> `protocol` (DTO) <- `transcript` (entity).
//! The codec still builds the DTO and re-exports it, so the decode path keeps
//! a single name for it.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `params` of a server `event` frame (PLAN §1.2).
///
/// Tolerant on purpose: a mistyped `seq`, a missing `type` or a missing
/// `params` degrades to defaults in the codec instead of dropping the frame,
/// and unknown event types are for the consumer to ignore.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventParams {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub session_id: String,
    /// Present only on session-bound events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<i64>,
    #[serde(default)]
    pub payload: Value,
}
