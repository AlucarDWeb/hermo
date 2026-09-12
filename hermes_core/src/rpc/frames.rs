//! JSON-RPC frame codec (adapter layer).
//!
//! Transport facts (PLAN.md §1.1-1.2, verified against Hermes v0.21.1): text
//! WebSocket frames, newline-delimited JSON-RPC 2.0 both ways; the server
//! coalesces streaming events, so one frame can carry several lines. Parse
//! defensively: never panic, ignore unknown fields and unknown event types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::json;
pub use crate::protocol::EventParams;

/// An outgoing client request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Request {
    pub jsonrpc: String,
    /// String or numeric per JSON-RPC; hermo always sends strings.
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// A decoded inbound line: result, error or event.
#[derive(Debug, Clone, PartialEq)]
pub enum Decoded {
    /// Response carrying a result for `id`.
    Result { id: String, result: Value },
    /// Response carrying an RPC error for `id`.
    Error { id: String, error: RpcError },
    /// Server event (no id).
    Event(EventParams),
    /// Valid JSON-RPC we do not understand — ignored, per the defensive rule.
    Ignored,
}

/// JSON-RPC error object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Value,
}

// PR #3 finding 6 (Dependency Rule): the entity (`error.rs`) must not
// import adapter types, so the `From<…> for CoreError` impls live next to
// the types that define them — an impl may live on either side of the
// `From` arrow. Conversion behaviour unchanged from the previous home.
impl From<RpcError> for crate::error::CoreError {
    fn from(e: RpcError) -> Self {
        crate::error::CoreError::Rpc {
            code: e.code,
            message: e.message,
        }
    }
}

/// Split one WebSocket text frame into JSON lines. Empty lines are dropped;
/// the returned strings keep no trailing newline.
pub fn split_lines(frame: &str) -> Vec<&str> {
    frame
        .split('\n')
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.trim().is_empty())
        .collect()
}

/// Decode one JSON-RPC line into a [`Decoded`].
///
/// `None` vs `Ignored` policy (pinned by tests): invalid JSON → `None`, the
/// line carries nothing routable. Valid JSON that is not JSON-RPC 2.0 we
/// care about → [`Decoded::Ignored`]. A notification without an `id` and
/// without the known `event` method → [`Decoded::Ignored`]. `event` frames
/// always decode (tolerantly, field by field) even when `params`, `type` or
/// `seq` are missing or mistyped.
///
/// The `id` may be a string or a number (PLAN.md §1.2: the server answers
/// string ids with the same string, but the protocol allows numbers).
pub fn decode(line: &str) -> Option<Decoded> {
    let mut value: Value = serde_json::from_str(line.trim()).ok()?;
    if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(Decoded::Ignored);
    }
    if value.get("method").and_then(Value::as_str) == Some("event") {
        // Tolerant event decoding, like the error path below: build
        // `EventParams` field by field so a mistyped `seq`, a missing `type`
        // or a missing `params` degrades to defaults instead of dropping
        // the frame.
        //
        // PR #1 review carry-over (PLAN §4 T2 item 6): `message.delta`
        // arrives coalesced at ~30 fps, so the payload is *moved* out of the
        // parsed value instead of cloning the subtree per frame.
        let mut params = value.get_mut("params").map(Value::take).unwrap_or(Value::Null);
        let seq = match params.get("seq") {
            Some(Value::Number(_)) => Some(json::i64_at(&params, "seq")),
            _ => None,
        };
        let payload = params
            .as_object_mut()
            .and_then(|o| o.remove("payload"))
            .unwrap_or(Value::Null);
        return Some(Decoded::Event(EventParams {
            event_type: json::str_at(&params, "type").to_string(),
            session_id: json::str_at(&params, "session_id").to_string(),
            seq,
            payload,
        }));
    }
    let id = match id_to_string(value.get("id")) {
        Some(id) => id,
        // Notification without an id and without a known method: nothing to
        // route, ignore it.
        None => return Some(Decoded::Ignored),
    };
    if let Some(err) = value.get_mut("error") {
        let code = json::i64_at(err, "code");
        let message = json::str_at(err, "message").to_string();
        let data = err
            .as_object_mut()
            .and_then(|o| o.remove("data"))
            .unwrap_or(Value::Null);
        return Some(Decoded::Error {
            id,
            error: RpcError { code, message, data },
        });
    }
    if let Some(result) = value.get_mut("result") {
        return Some(Decoded::Result {
            id,
            result: Value::take(result),
        });
    }
    Some(Decoded::Ignored)
}

fn id_to_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip() {
        let req = Request {
            jsonrpc: "2.0".into(),
            id: "r12".into(),
            method: "prompt.submit".into(),
            params: serde_json::json!({"session_id": "abc", "text": "hi"}),
        };
        let line = crate::json::to_compact_string(&serde_json::to_value(&req).unwrap());
        let back: Request = serde_json::from_str(&line).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn numeric_id_response_decode() {
        let line = r#"{"jsonrpc":"2.0","id":7,"result":{"ok":true}}"#;
        match decode(line).expect("decodes") {
            Decoded::Result { id, result } => {
                assert_eq!(id, "7");
                assert!(crate::json::bool_at(&result, "ok"));
            }
            other => panic!("expected Result, got {:?}", other),
        }
    }

    #[test]
    fn two_lines_in_one_frame() {
        let frame = "{\"jsonrpc\":\"2.0\",\"method\":\"event\",\"params\":{\"type\":\"message.delta\",\"session_id\":\"s1\",\"seq\":1,\"payload\":{\"text\":\"Hel\"}}}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"ok\":true}}\n";
        let lines = split_lines(frame);
        assert_eq!(lines.len(), 2);
        match decode(lines[0]).unwrap() {
            Decoded::Event(ev) => {
                assert_eq!(ev.event_type, "message.delta");
                assert_eq!(ev.session_id, "s1");
                assert_eq!(ev.seq, Some(1));
                assert_eq!(crate::json::str_at(&ev.payload, "text"), "Hel");
            }
            other => panic!("expected Event, got {:?}", other),
        }
        assert!(matches!(decode(lines[1]).unwrap(), Decoded::Result { .. }));
    }

    #[test]
    fn error_response_decode() {
        let line = r#"{"jsonrpc":"2.0","id":"r12","error":{"code":4009,"message":"session busy","data":null}}"#;
        match decode(line).unwrap() {
            Decoded::Error { id, error } => {
                assert_eq!(id, "r12");
                assert_eq!(error.code, 4009);
                assert_eq!(error.message, "session busy");
            }
            other => panic!("expected Error, got {:?}", other),
        }
    }

    #[test]
    fn event_without_seq_and_empty_payload_decode() {
        let line = r#"{"jsonrpc":"2.0","method":"event","params":{"type":"message.start","session_id":"s1"}}"#;
        match decode(line).unwrap() {
            Decoded::Event(ev) => {
                assert_eq!(ev.event_type, "message.start");
                assert!(ev.seq.is_none());
                assert!(ev.payload.is_null());
            }
            other => panic!("expected Event, got {:?}", other),
        }
    }

    #[test]
    fn never_panics_on_garbage_or_unknown_shapes() {
        assert!(decode("not json").is_none());
        // unknown jsonrpc version -> Ignored, not a panic
        assert!(matches!(
            decode(r#"{"jsonrpc":"1.0","id":1,"result":{}}"#).unwrap(),
            Decoded::Ignored
        ));
        // event with unknown extra fields and unknown type is kept as Event
        let unknown = decode(
            r#"{"jsonrpc":"2.0","method":"event","params":{"type":"future.thing","x":1,"payload":{"a":2}}}"#,
        )
        .unwrap();
        match unknown {
            Decoded::Event(ev) => assert_eq!(ev.event_type, "future.thing"),
            other => panic!("expected Event, got {:?}", other),
        }
        // missing params on an event -> still an Event, with empty type
        match decode(r#"{"jsonrpc":"2.0","method":"event"}"#).unwrap() {
            Decoded::Event(ev) => {
                assert_eq!(ev.event_type, "");
                assert!(ev.seq.is_none());
            }
            other => panic!("expected Event, got {:?}", other),
        }
    }

    #[test]
    fn event_with_float_seq_and_missing_params_still_decode() {
        // mistyped `seq` (float): kept, truncated to i64
        let float_seq = decode(
            r#"{"jsonrpc":"2.0","method":"event","params":{"type":"message.delta","session_id":"s1","seq":1.0,"payload":{}}}"#,
        )
        .expect("float seq must not drop the frame");
        match float_seq {
            Decoded::Event(ev) => assert_eq!(ev.seq, Some(1)),
            other => panic!("expected Event, got {:?}", other),
        }
        // missing `params`: the event itself must not vanish
        let no_params =
            decode(r#"{"jsonrpc":"2.0","method":"event"}"#).expect("no params must not drop the frame");
        match no_params {
            Decoded::Event(ev) => {
                assert_eq!(ev.event_type, "");
                assert_eq!(ev.session_id, "");
                assert!(ev.seq.is_none());
                assert!(ev.payload.is_null());
            }
            other => panic!("expected Event, got {:?}", other),
        }
        // missing `type` (and `session_id`): degrades to defaults, not dropped
        let no_type = decode(
            r#"{"jsonrpc":"2.0","method":"event","params":{"seq":7,"payload":{"a":1}}}"#,
        )
        .expect("no type must not drop the frame");
        match no_type {
            Decoded::Event(ev) => {
                assert_eq!(ev.event_type, "");
                assert_eq!(ev.seq, Some(7));
                assert_eq!(crate::json::i64_at(&ev.payload, "a"), 1);
            }
            other => panic!("expected Event, got {:?}", other),
        }
    }

    #[test]
    fn decode_none_vs_ignored_policy() {
        // invalid JSON -> None
        assert!(decode("not json").is_none());
        // valid JSON that is not JSON-RPC 2.0 -> Ignored
        assert!(matches!(decode("[]").unwrap(), Decoded::Ignored));
        // notification without an id and without the known `event` method -> Ignored
        assert!(matches!(
            decode(r#"{"jsonrpc":"2.0","method":"ping"}"#).unwrap(),
            Decoded::Ignored
        ));
    }

    #[test]
    fn whole_fixture_decodes_and_records_wire_facts() {
        let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/events.jsonl");
        let data = std::fs::read_to_string(fixture).expect("fixture exists");
        let lines: Vec<&str> = data.lines().collect();
        assert!(!lines.is_empty(), "fixture must not be empty");
        let mut events = 0;
        let mut redirected = false;
        let mut clarify_request = false;
        for line in lines {
            match decode(line) {
                Some(Decoded::Event(ev)) => {
                    events += 1;
                    if ev.event_type == "clarify.request" {
                        clarify_request = true;
                    }
                }
                Some(Decoded::Result { id, result }) => {
                    // Mid-turn busy submit: verified status == "redirected" (PROVENANCE).
                    if id == "r-busy" {
                        assert_eq!(
                            crate::json::str_at(&result, "status"),
                            "redirected",
                            "busy submit must carry status=redirected"
                        );
                        redirected = true;
                    }
                }
                Some(Decoded::Error { .. }) | Some(Decoded::Ignored) => {}
                None => panic!("line failed to decode defensively: {}", line),
            }
        }
        assert!(events > 0, "fixture lines are mostly events");
        assert!(redirected, "the r-busy result must be present");
        assert!(clarify_request, "a clarify.request event must be present");
    }

    #[test]
    fn synthetic_approval_fixture_decodes() {
        let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/events_synthetic.jsonl");
        let data = std::fs::read_to_string(fixture).expect("fixture exists");
        for line in data.lines() {
            match decode(line).unwrap() {
                Decoded::Event(ev) => assert_eq!(ev.event_type, "approval.request"),
                other => panic!("expected Event, got {:?}", other),
            }
        }
    }

    /// PR #3 finding 6: this test moved here with the `From<RpcError>`
    /// impl it pins (it lived in `error.rs`, which must not know adapter
    /// types). Behaviour unchanged: an RPC error carries its code through.
    #[test]
    fn rpc_error_converts_to_core_error() {
        let e: crate::error::CoreError = RpcError {
            code: 4009,
            message: "session busy".into(),
            data: Value::Null,
        }
        .into();
        assert!(matches!(e, crate::error::CoreError::Rpc { code: 4009, .. }));
    }
}