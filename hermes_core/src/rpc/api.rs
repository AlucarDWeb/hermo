//! Typed RPC wrappers over [`GatewayClient::call`] (adapter layer).
//!
//! Every wrapper degrades a missing/unknown field to a default (the
//! `json::*_at` accessors never panic); none of them re-implements policy —
//! they only shape the wire data of PLAN.md §1.3 into structs.

use serde_json::{json, Value};

use crate::json;
use crate::rpc::client::{ClientError, GatewayClient};

// ── session.create ──────────────────────────────────────────────────────

/// Result of `session.create` (PLAN §1.3): live sid + durable stored id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CreatedSession {
    /// The live session id — use for every subsequent RPC.
    pub session_id: String,
    /// The durable id — persist this; it is what `session.list` returns.
    pub stored_session_id: String,
    pub message_count: i64,
}

impl CreatedSession {
    fn from_value(value: &Value) -> Self {
        Self {
            session_id: json::str_at(value, "session_id").to_string(),
            stored_session_id: json::str_at(value, "stored_session_id").to_string(),
            message_count: json::i64_at(value, "message_count"),
        }
    }
}

// ── session.list ────────────────────────────────────────────────────────

/// One entry of `session.list`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionEntry {
    /// The durable (stored) id.
    pub id: String,
    /// Present on some builds; live id when it differs from `id`.
    pub resolved_id: String,
    pub title: String,
    pub preview: String,
    pub started_at: String,
    pub message_count: i64,
    pub source: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionList {
    pub sessions: Vec<SessionEntry>,
}

// ── session.resume ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResumedSession {
    /// Live sid after resume (may differ from the stored id passed in).
    pub session_id: String,
    /// True when the server reattached a parked live session.
    pub resumed: bool,
    pub message_count: i64,
    /// Raw transcript messages (typed properly in T4).
    pub messages: Vec<Value>,
    /// Whether a turn was already running when we reattached.
    pub running: bool,
    /// Un-acked events existed server-side (`messages_omitted` / inflight).
    pub inflight: bool,
}

impl ResumedSession {
    fn from_value(value: &Value) -> Self {
        Self {
            session_id: json::str_at(value, "session_id").to_string(),
            resumed: json::bool_at(value, "resumed"),
            message_count: json::i64_at(value, "message_count"),
            messages: value
                .get("messages")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            running: json::bool_at(value, "running"),
            inflight: json::bool_at(value, "inflight"),
        }
    }
}

// ── prompt.submit / busy ────────────────────────────────────────────────

/// Result of `prompt.submit`: `streaming` on a fresh turn, `redirected` when
/// a turn was already running (verified live: an ordinary result, not an
/// error — PLAN §1.3, PROVENANCE busy-submit entry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitStatus {
    Streaming,
    Redirected,
    Other,
}

impl SubmitStatus {
    fn parse(value: &Value) -> Self {
        match json::str_at(value, "status") {
            "streaming" => SubmitStatus::Streaming,
            "redirected" => SubmitStatus::Redirected,
            _ => SubmitStatus::Other,
        }
    }
}

/// Reply of `clarify.respond`: `ok` or `expired`, plus remaining question ids
/// for the batch shape.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClarifyAck {
    pub status: String,
    pub remaining: Vec<String>,
}

impl ClarifyAck {
    /// Tolerant wire parser: a missing or mistyped `remaining` degrades to
    /// an empty vec, a missing `status` to "" — never a panic.
    fn from_value(value: &Value) -> Self {
        Self {
            status: json::str_at(value, "status").to_string(),
            remaining: value
                .get("remaining")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .map(|v| v.as_str().unwrap_or_default().to_string())
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

// ── command.dispatch ────────────────────────────────────────────────────

/// One of the five dispatch result shapes (PLAN §1.3).
#[derive(Debug, Clone, PartialEq)]
pub enum DispatchOutcome {
    /// `{type:"exec"|"plugin", output?}`.
    Output(String),
    /// `{type:"alias", target}`.
    Alias(String),
    /// `{type:"skill", name, message?, display?}` — `display` is kept for
    /// the T10 slash-command surface.
    Skill {
        name: String,
        message: String,
        display: String,
    },
    /// `{type:"send", message, notice?}`.
    Send(String),
    /// `{type:"prefill", message}`.
    Prefill(String),
    /// Unknown `type` — degrade, never panic.
    Unknown,
}

impl DispatchOutcome {
    fn from_value(value: &Value) -> Self {
        match json::str_at(value, "type") {
            "exec" | "plugin" => DispatchOutcome::Output(json::str_at(value, "output").to_string()),
            "alias" => DispatchOutcome::Alias(json::str_at(value, "target").to_string()),
            "skill" => DispatchOutcome::Skill {
                name: json::str_at(value, "name").to_string(),
                message: json::str_at(value, "message").to_string(),
                display: json::str_at(value, "display").to_string(),
            },
            "send" => DispatchOutcome::Send(json::str_at(value, "message").to_string()),
            "prefill" => DispatchOutcome::Prefill(json::str_at(value, "message").to_string()),
            _ => DispatchOutcome::Unknown,
        }
    }
}

/// Item of `complete.slash`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SlashCompletionItem {
    pub display: String,
    pub text: String,
    pub kind: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SlashCompletions {
    pub items: Vec<SlashCompletionItem>,
    /// Offset in the typed text where the replacement starts (-1 = absent).
    pub replace_from: i64,
}

// ── session.events.since ────────────────────────────────────────────────

/// One replayed event of `session.events.since`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReplayedEvent {
    pub event_type: String,
    pub session_id: String,
    pub seq: i64,
    pub payload: Value,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct EventsSince {
    pub events: Vec<ReplayedEvent>,
    pub latest_seq: i64,
    /// True when the server dropped older events — rebuild from resume.
    pub truncated: bool,
    /// Replay epoch at replay time; compare with the stored epoch.
    pub epoch: String,
}

// ── wrappers ────────────────────────────────────────────────────────────

/// `session.create {cols}` — mint a fresh chat session.
pub async fn create_session(client: &GatewayClient, cols: i64) -> Result<CreatedSession, ClientError> {
    let result = client
        .call("session.create", json!({ "cols": cols }))
        .await?;
    Ok(CreatedSession::from_value(&result))
}

/// `session.list {limit}` — durable sessions for the picker.
pub async fn list_sessions(client: &GatewayClient, limit: i64) -> Result<SessionList, ClientError> {
    let result = client
        .call("session.list", json!({ "limit": limit }))
        .await?;
    let sessions = result
        .get("sessions")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|s| SessionEntry {
                    id: json::str_at(s, "id").to_string(),
                    resolved_id: json::str_at(s, "resolved_id").to_string(),
                    title: json::str_at(s, "title").to_string(),
                    preview: json::str_at(s, "preview").to_string(),
                    started_at: json::str_at(s, "started_at").to_string(),
                    message_count: json::i64_at(s, "message_count"),
                    source: json::str_at(s, "source").to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(SessionList { sessions })
}

/// `session.resume {session_id}` — stored id in, live sid out.
pub async fn resume_session(
    client: &GatewayClient,
    stored_id: &str,
) -> Result<ResumedSession, ClientError> {
    let result = client
        .call("session.resume", json!({ "session_id": stored_id }))
        .await?;
    Ok(ResumedSession::from_value(&result))
}

/// `prompt.submit` — returns the parsed status so callers can distinguish
/// `streaming` from a busy `redirected` (an ordinary result on the wire).
pub async fn submit(
    client: &GatewayClient,
    session_id: &str,
    text: &str,
) -> Result<SubmitStatus, ClientError> {
    let result = client
        .call(
            "prompt.submit",
            json!({ "session_id": session_id, "text": text }),
        )
        .await?;
    Ok(SubmitStatus::parse(&result))
}

/// `session.interrupt`.
pub async fn interrupt(client: &GatewayClient, session_id: &str) -> Result<Value, ClientError> {
    client
        .call(
            "session.interrupt",
            json!({ "session_id": session_id }),
        )
        .await
}

/// `approval.respond {choice: once|session|always|deny}`.
pub async fn respond_approval(
    client: &GatewayClient,
    session_id: &str,
    request_id: &str,
    choice: &str,
) -> Result<Value, ClientError> {
    client
        .call(
            "approval.respond",
            json!({
                "session_id": session_id,
                "request_id": request_id,
                "choice": choice,
            }),
        )
        .await
}

/// `clarify.respond` — `question_id` optional (single-question shape).
pub async fn respond_clarify(
    client: &GatewayClient,
    session_id: &str,
    request_id: &str,
    answer: &str,
    question_id: Option<&str>,
) -> Result<ClarifyAck, ClientError> {
    let mut params = json!({
        "session_id": session_id,
        "request_id": request_id,
        "answer": answer,
    });
    if let Some(qid) = question_id {
        params["question_id"] = json!(qid);
    }
    let result = client.call("clarify.respond", params).await?;
    Ok(ClarifyAck::from_value(&result))
}

/// `approval.pending` — re-emitted cards after a reconnect.
pub async fn pending_approvals(
    client: &GatewayClient,
    session_id: &str,
) -> Result<Value, ClientError> {
    client
        .call("approval.pending", json!({ "session_id": session_id }))
        .await
}

/// `slash.exec` — `command` carries its leading `/`.
pub async fn slash_exec(
    client: &GatewayClient,
    session_id: &str,
    command: &str,
) -> Result<Value, ClientError> {
    client
        .call(
            "slash.exec",
            json!({ "session_id": session_id, "command": command }),
        )
        .await
}

/// `command.dispatch {name, arg}` — parse with [`DispatchOutcome`].
pub async fn command_dispatch(
    client: &GatewayClient,
    session_id: &str,
    name: &str,
    arg: &str,
) -> Result<DispatchOutcome, ClientError> {
    let result = client
        .call(
            "command.dispatch",
            json!({ "session_id": session_id, "name": name, "arg": arg }),
        )
        .await?;
    Ok(DispatchOutcome::from_value(&result))
}

/// `complete.slash` — composer completion for text starting with `/`.
pub async fn complete_slash(
    client: &GatewayClient,
    text: &str,
) -> Result<SlashCompletions, ClientError> {
    let result = client.call("complete.slash", json!({ "text": text })).await?;
    Ok(SlashCompletions {
        items: result
            .get("items")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|i| SlashCompletionItem {
                        display: json::str_at(i, "display").to_string(),
                        text: json::str_at(i, "text").to_string(),
                        kind: json::str_at(i, "kind").to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        replace_from: match result.get("replace_from") {
            Some(Value::Number(_)) => json::i64_at(&result, "replace_from"),
            _ => -1,
        },
    })
}

/// `session.events.since` — the reconnect gap fill (PLAN §1.5).
pub async fn events_since(
    client: &GatewayClient,
    session_id: &str,
    last_seen: i64,
) -> Result<EventsSince, ClientError> {
    let result = client
        .call(
            "session.events.since",
            json!({ "session_id": session_id, "last_seen": last_seen }),
        )
        .await?;
    Ok(EventsSince {
        events: result
            .get("events")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|e| ReplayedEvent {
                        event_type: json::str_at(e, "type").to_string(),
                        session_id: json::str_at(e, "session_id").to_string(),
                        seq: json::i64_at(e, "seq"),
                        payload: e.get("payload").cloned().unwrap_or(Value::Null),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        latest_seq: json::i64_at(&result, "latest_seq"),
        truncated: json::bool_at(&result, "truncated"),
        epoch: json::str_at(&result, "epoch").to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn created_session_tolerates_missing_fields() {
        let cs = CreatedSession::from_value(&json!({}));
        assert!(cs.session_id.is_empty());
        assert_eq!(cs.message_count, 0);
        let cs = CreatedSession::from_value(&json!({"session_id": "s1", "message_count": 3.0}));
        assert_eq!(cs.session_id, "s1");
        assert_eq!(cs.message_count, 3);
    }

    #[test]
    fn submit_status_parses_verified_wire_values() {
        assert_eq!(SubmitStatus::parse(&json!({"status": "streaming"})), SubmitStatus::Streaming);
        // The busy reply is an ordinary result, verified live (PROVENANCE).
        assert_eq!(SubmitStatus::parse(&json!({"status": "redirected"})), SubmitStatus::Redirected);
        assert_eq!(SubmitStatus::parse(&json!({})), SubmitStatus::Other);
        assert_eq!(SubmitStatus::parse(&json!({"status": 5})), SubmitStatus::Other);
    }

    #[test]
    fn dispatch_outcome_covers_all_verified_shapes() {
        assert_eq!(
            DispatchOutcome::from_value(&json!({"type": "exec", "output": "hi"})),
            DispatchOutcome::Output("hi".into())
        );
        assert_eq!(
            DispatchOutcome::from_value(&json!({"type": "alias", "target": "/other"})),
            DispatchOutcome::Alias("/other".into())
        );
        assert_eq!(
            DispatchOutcome::from_value(&json!({"type": "skill", "name": "n"})),
            DispatchOutcome::Skill {
                name: "n".into(),
                message: "".into(),
                display: "".into()
            }
        );
        // The optional `display` field is kept (T10 wants it).
        assert_eq!(
            DispatchOutcome::from_value(&json!(
                {"type": "skill", "name": "n", "message": "m", "display": "/n preview"}
            )),
            DispatchOutcome::Skill {
                name: "n".into(),
                message: "m".into(),
                display: "/n preview".into()
            }
        );
        assert_eq!(
            DispatchOutcome::from_value(&json!({"type": "send", "message": "m"})),
            DispatchOutcome::Send("m".into())
        );
        assert_eq!(
            DispatchOutcome::from_value(&json!({"type": "prefill", "message": "p"})),
            DispatchOutcome::Prefill("p".into())
        );
        // Unknown type degrades instead of panicking.
        assert_eq!(
            DispatchOutcome::from_value(&json!({"type": "future.thing"})),
            DispatchOutcome::Unknown
        );
        assert_eq!(DispatchOutcome::from_value(&json!({})), DispatchOutcome::Unknown);
    }

    /// Wire-shaped, through the production parser (fix pass item 7): a
    /// batch ack carries the remaining question ids.
    #[test]
    fn clarify_ack_tolerates_missing_remaining() {
        let ack = ClarifyAck::from_value(&json!({"status": "ok", "remaining": ["q2"]}));
        assert_eq!(ack.status, "ok");
        assert_eq!(ack.remaining, vec!["q2".to_string()]);
        // A completely empty object (or a mistyped remaining) degrades to
        // defaults, never panics.
        let empty = ClarifyAck::from_value(&json!({}));
        assert!(empty.status.is_empty());
        assert!(empty.remaining.is_empty());
        let mistyped = ClarifyAck::from_value(&json!({"status": "expired", "remaining": 5}));
        assert_eq!(mistyped.status, "expired");
        assert!(mistyped.remaining.is_empty());
    }

    #[test]
    fn resumed_session_degrades_on_wrong_shapes() {
        let r = ResumedSession::from_value(&json!({
            "session_id": "live",
            "resumed": true,
            "message_count": 5,
            "messages": "not-an-array",
            "running": "truthy-but-not-bool",
        }));
        assert_eq!(r.session_id, "live");
        assert!(r.resumed);
        assert_eq!(r.message_count, 5);
        assert!(r.messages.is_empty(), "mistyped messages degrade to empty");
        assert!(!r.running, "mistyped bool degrades to false");
    }
}
