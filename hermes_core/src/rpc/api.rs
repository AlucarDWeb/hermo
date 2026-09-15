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
    /// The owning profile, defensively read from `info.profile_name` when
    /// the server sent one (T16a); empty when absent — never a guess.
    pub profile_name: String,
}

impl CreatedSession {
    fn from_value(value: &Value) -> Self {
        Self {
            session_id: json::str_at(value, "session_id").to_string(),
            stored_session_id: json::str_at(value, "stored_session_id").to_string(),
            message_count: json::i64_at(value, "message_count"),
            profile_name: json::str_at(value.get("info").unwrap_or(&Value::Null), "profile_name")
                .to_string(),
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
    /// The owning profile, defensively read from `info.profile_name` (T16a);
    /// empty when absent — never a guess.
    pub profile_name: String,
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
            profile_name: json::str_at(value.get("info").unwrap_or(&Value::Null), "profile_name")
                .to_string(),
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
    /// Short description the popup shows next to the display string (T10).
    pub meta: String,
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

/// `session.create {cols[, profile][, title]}` — mint a fresh chat session.
/// `profile`/`title` are T16a additions (the Bot Chat verb); `None` (or an
/// empty string) OMITS the JSON key entirely, so a bare create is byte-for-
/// byte what it always was.
pub async fn create_session(
    client: &GatewayClient,
    cols: i64,
    profile: Option<&str>,
    title: Option<&str>,
) -> Result<CreatedSession, ClientError> {
    let mut params = json!({ "cols": cols });
    if let Some(p) = profile.filter(|s| !s.is_empty()) {
        params["profile"] = json!(p);
    }
    if let Some(t) = title.filter(|s| !s.is_empty()) {
        params["title"] = json!(t);
    }
    let result = client.call("session.create", params).await?;
    Ok(CreatedSession::from_value(&result))
}

/// `session.list {limit[, include_hidden][, title]}` — durable sessions for
/// the picker, with the T16a filters: `include_hidden`/`title` (the Bot Chat
/// lookup needs both — hidden-at-birth rows are invisible without the flag).
/// Default-valued keys are omitted.
pub async fn list_sessions(
    client: &GatewayClient,
    limit: i64,
    include_hidden: bool,
    title: Option<&str>,
) -> Result<SessionList, ClientError> {
    let mut params = json!({ "limit": limit });
    if include_hidden {
        params["include_hidden"] = json!(true);
    }
    if let Some(t) = title.filter(|s| !s.is_empty()) {
        params["title"] = json!(t);
    }
    let result = client.call("session.list", params).await?;
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

/// `command.dispatch {name, arg}` — parse with [`DispatchOutcome`]. `pub`
/// from T10: `rpc::slash` maps the outcome onto the slash-ladder plan.
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
                        meta: json::str_at(i, "meta").to_string(),
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

    /// Wire-shaped, through the production `complete_slash` wrapper (T10):
    /// the popup cannot insert without `replace_from`, and the item rows are
    /// flat `{text, display, meta, kind}` objects exactly as
    /// `methods_complete.py` writes them — not nested. A test that passed
    /// against a `Vec<item>`-only return would be tautological, so the DTO
    /// fields themselves are asserted.
    #[tokio::test]
    async fn complete_slash_keeps_replace_from_and_item_meta() {
        let gw = crate::rpc::client::GatewayClient::for_fixture_tests(&|_m, _p| async {
            Ok(json!({
                "items": [
                    {"text": "/deploy", "display": "/deploy", "meta": "ship it", "kind": "skill"},
                    {"text": "/docs", "display": "/docs", "meta": "", "kind": "command"}
                ],
                "replace_from": 8
            }))
        })
        .await;
        let c = complete_slash(&gw, "/deploy ").await.expect("complete_slash");
        assert_eq!(c.replace_from, 8, "replace_from must survive to the DTO");
        assert_eq!(c.items.len(), 2);
        assert_eq!(c.items[0].text, "/deploy");
        assert_eq!(c.items[0].display, "/deploy");
        assert_eq!(c.items[0].meta, "ship it");
        assert_eq!(c.items[0].kind, "skill");
        assert_eq!(c.items[1].meta, "", "an absent meta degrades to empty");
        assert_eq!(c.items[1].kind, "command");
    }

    /// `replace_from` missing (or mistyped) degrades to the -1 sentinel the
    /// surface already knows (`SlashCompletions::replace_from` doc) — never a
    /// panic, never a wrong offset.
    #[tokio::test]
    async fn complete_slash_degrades_to_minus_one_without_replace_from() {
        let gw = crate::rpc::client::GatewayClient::for_fixture_tests(&|_m, _p| async {
            Ok(json!({"items": []}))
        })
        .await;
        let c = complete_slash(&gw, "/d").await.expect("complete_slash");
        assert_eq!(c.replace_from, -1);
        assert!(c.items.is_empty());
    }

    // ── T16a: profile/title forwarding, wire-verified ────────────────────

    /// A `&'static` log of (method, params) the fake gateway received —
    /// server-side evidence, not client-side bookkeeping.
    fn leak_log() -> &'static std::sync::Mutex<Vec<(String, Value)>> {
        Box::leak(Box::new(std::sync::Mutex::new(Vec::new())))
    }

    /// The create wrapper must send `profile` and `title` EXACTLY when they
    /// are non-empty, and omit both keys on a bare create (T16a decision 2:
    /// `open_session(None)` stays byte-for-byte what it always was).
    #[tokio::test]
    async fn create_forwards_profile_and_title_and_omits_when_absent() {
        let log = leak_log();
        // `for_fixture_tests` wants a `&'static` handler and a capturing
        // closure cannot be static-promoted: leak the concrete closure.
        let handler = Box::leak(Box::new(move |m: String, p: Value| {
            log.lock().unwrap().push((m, p));
            async { Ok(json!({"session_id": "live", "stored_session_id": "stored"})) }
        }));
        let gw = crate::rpc::client::GatewayClient::for_fixture_tests(handler).await;

        create_session(&gw, 80, Some("jn-core"), Some("Bot Chat")).await.unwrap();
        create_session(&gw, 80, None, None).await.unwrap();
        // An empty string is treated as absent: no key, not "".
        create_session(&gw, 80, Some(""), Some("")).await.unwrap();

        let calls = log.lock().unwrap();
        assert_eq!(calls.len(), 3);
        let (m, p) = &calls[0];
        assert_eq!(m, "session.create");
        assert_eq!(p["profile"], "jn-core", "profile forwarded on the wire");
        assert_eq!(p["title"], "Bot Chat", "title forwarded on the wire");
        assert_eq!(p["cols"], 80);
        let (_, p) = &calls[1];
        assert!(p.get("profile").is_none(), "bare create omits profile: {p}");
        assert!(p.get("title").is_none(), "bare create omits title: {p}");
        let (_, p) = &calls[2];
        assert!(p.get("profile").is_none() && p.get("title").is_none(), "empty == absent: {p}");
    }

    /// The list wrapper must send `include_hidden: true` and `title` when the
    /// Bot Chat lookup asks for them, and a default list keeps working with
    /// only `limit` (T16a decision 3).
    #[tokio::test]
    async fn list_sends_include_hidden_and_title_only_when_needed() {
        let log = leak_log();
        let handler = Box::leak(Box::new(move |m: String, p: Value| {
            log.lock().unwrap().push((m, p));
            async { Ok(json!({"sessions": []})) }
        }));
        let gw = crate::rpc::client::GatewayClient::for_fixture_tests(handler).await;

        list_sessions(&gw, 200, true, Some("Bot Chat")).await.unwrap();
        list_sessions(&gw, 200, false, None).await.unwrap();

        let calls = log.lock().unwrap();
        assert_eq!(calls.len(), 2);
        let (m, p) = &calls[0];
        assert_eq!(m, "session.list");
        assert_eq!(p["include_hidden"], true, "hidden rows are invisible without the flag");
        assert_eq!(p["title"], "Bot Chat");
        assert_eq!(p["limit"], 200);
        let (_, p) = &calls[1];
        assert!(p.get("include_hidden").is_none(), "default list omits the flag: {p}");
        assert!(p.get("title").is_none(), "default list omits title: {p}");
        assert_eq!(p["limit"], 200);
    }

    /// `info.profile_name` is parsed defensively from both create and resume
    /// results (T16a decision 6): present → carried, absent/mistyped → "".
    #[test]
    fn profile_name_parsed_defensively_from_info() {
        let created = CreatedSession::from_value(&json!({
            "session_id": "s", "stored_session_id": "k",
            "info": {"profile_name": "jn-core", "unrelated": {"deep": [1]}}
        }));
        assert_eq!(created.profile_name, "jn-core");
        let bare = CreatedSession::from_value(&json!({"session_id": "s"}));
        assert!(bare.profile_name.is_empty(), "no info block: empty, not a panic");
        let mistyped = CreatedSession::from_value(&json!({"info": {"profile_name": 7}}));
        assert!(mistyped.profile_name.is_empty(), "mistyped degrades to empty");

        let resumed = ResumedSession::from_value(&json!({
            "session_id": "live", "info": {"profile_name": "jn-review"}
        }));
        assert_eq!(resumed.profile_name, "jn-review");
        let resumed_bare = ResumedSession::from_value(&json!({}));
        assert!(resumed_bare.profile_name.is_empty());
    }
}
