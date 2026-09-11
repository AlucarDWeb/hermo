//! JSON-RPC frame codec (adapter layer).
//!
//! Transport facts (PLAN.md §1.1-1.2, verified against Hermes v0.21.1): text
//! WebSocket frames, newline-delimited JSON-RPC 2.0 both ways; the server
//! coalesces streaming events, so one frame can carry several lines. Parse
//! defensively: never panic, ignore unknown fields and unknown event types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::json;

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

/// `params` of a server `event` frame.
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

/// JSON-RPC error object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub data: Value,
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
/// The `id` may be a string or a number (PLAN.md §1.2: the server answers
/// string ids with the same string, but the protocol allows numbers).
pub fn decode(line: &str) -> Option<Decoded> {
    let value: Value = serde_json::from_str(line.trim()).ok()?;
    if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(Decoded::Ignored);
    }
    if value.get("method").and_then(Value::as_str) == Some("event") {
        let params: EventParams = serde_json::from_value(value.get("params").cloned()?).ok()?;
        return Some(Decoded::Event(params));
    }
    let id = id_to_string(value.get("id"))?;
    if let Some(err) = value.get("error") {
        let code = json::i64_at(err, "code");
        let message = json::str_at(err, "message").to_string();
        let data = err.get("data").cloned().unwrap_or(Value::Null);
        return Some(Decoded::Error {
            id,
            error: RpcError { code, message, data },
        });
    }
    if let Some(result) = value.get("result") {
        return Some(Decoded::Result {
            id,
            result: result.clone(),
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
                assert_eq!(crate::json::bool_at(&result, "ok"), true);
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
        // missing params on an event -> None, no panic
        assert!(decode(r#"{"jsonrpc":"2.0","method":"event"}"#).is_none());
    }

    #[test]
    fn first_50_fixture_lines_decode_without_error() {
        let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/events.jsonl");
        let data = std::fs::read_to_string(fixture).expect("fixture exists");
        let lines: Vec<&str> = data.lines().take(50).collect();
        assert_eq!(lines.len(), 50, "fixture must have >= 50 lines");
        let mut events = 0;
        for line in lines {
            match decode(line) {
                Some(Decoded::Event(_)) => events += 1,
                Some(Decoded::Result { .. }) | Some(Decoded::Error { .. }) | Some(Decoded::Ignored) => {}
                None => panic!("line failed to decode defensively: {}", line),
            }
        }
        assert!(events > 0, "fixture lines are mostly events");
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
}