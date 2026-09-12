//! Per-session transcript state machine (entity layer, PLAN §4 T4).
//!
//! One [`Reducer`] per open chat (PLAN decision 10). Its output is a
//! routable change stream: every [`TranscriptChange`] is wrapped as
//! [`SessionChange { key, change }`] so the UI can route it to the session it
//! belongs to. `reset_from_resume` takes the key it belongs to. Session-less
//! events (`gateway.ready`, connection state, skin, …) are not reducer input
//! and never create rows — the T5 session registry is out of scope here.
//!
//! Pure and deterministic: no clocks, no randomness, no I/O; ordering comes
//! from the event stream. Unknown event types, missing fields and garbage
//! payloads are ignored without panicking (`json::*_at` helpers).

use serde_json::Value;

use crate::json;
use crate::protocol::EventParams;
use crate::transcript::model::{
    ApprovalCard, ClarifyCard, ClarifyQuestion, Row, RowKind, SessionHeader, StatusKind, ToolCard,
    Transcript,
};

/// The session a change belongs to, plus the change itself (PLAN §4 T4:
/// routable change stream, one shape used everywhere).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionChange {
    /// The session key the change belongs to (the live `session_id` of the
    /// event, as delivered on the wire).
    pub key: String,
    pub change: TranscriptChange,
}

/// Every observable transcript change a UI needs to render incrementally.
#[derive(Debug, Clone, PartialEq)]
pub enum TranscriptChange {
    /// A new row was appended at `index` (a new assistant/thinking/tool/
    /// approval/clarify/status/header/error row). Never emitted for a
    /// session-less event.
    RowAppended { index: usize },
    /// The row at `index` was updated in place (assistant deltas, tool
    /// progress, card resolution).
    RowUpdated { index: usize },
    /// The whole transcript was reset (resume / rebuild).
    Reset,
    /// Only the header was refreshed (no row change).
    HeaderUpdated,
}

/// Per-session reducer: `apply` the decoded event, get the routable changes.
#[derive(Debug, Clone, Default)]
pub struct Reducer {
    state: Transcript,
    /// Index of the currently streaming assistant row, if one is open.
    streaming: Option<usize>,
    /// Index of the currently streaming thinking row, if one is open.
    thinking: Option<usize>,
    /// tool_id → row index, so tool events update one card in place.
    tools: std::collections::HashMap<String, usize>,
}

impl Reducer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Read-only view of the transcript state.
    pub fn transcript(&self) -> &Transcript {
        &self.state
    }

    /// Apply one decoded event; empty `session_id` (a session-less event
    /// such as `gateway.ready`) yields no change and no row. Unknown event
    /// types, missing fields and garbage payloads are ignored without
    /// panicking.
    pub fn apply(&mut self, event: &EventParams) -> Vec<SessionChange> {
        if event.session_id.is_empty() {
            // Session-less events are not reducer input: emit nothing, add
            // no row (PLAN §4 T4 multi-session rule).
            return Vec::new();
        }
        let key = event.session_id.clone();
        let changes = match event.event_type.as_str() {
            "message.start" => self.on_message_start(),
            "message.delta" => self.on_message_delta(&event.payload),
            "message.interim" => self.on_message_interim(&event.payload),
            "message.complete" => self.on_message_complete(&event.payload),
            "thinking.delta" | "reasoning.delta" => self.on_thinking_delta(&event.payload),
            "tool.generating" => self.on_tool_generating(&event.payload),
            "tool.start" => self.on_tool_start(&event.payload),
            "tool.progress" => self.on_tool_progress(&event.payload),
            "tool.complete" => self.on_tool_complete(&event.payload),
            "status.update" => self.on_status_update(&event.payload),
            "approval.request" => self.on_approval_request(&event.payload),
            "clarify.request" => self.on_clarify_request(&event.payload),
            "session.info" => self.on_session_info(&event.payload),
            "session.title" => self.on_session_title(&event.payload),
            "error" => self.on_error(&event.payload),
            // Unknown event types are ignored (PLAN §1.4 "ignore for PoC",
            // PI_ROLE defensive rule) — no panic, no row.
            _ => Vec::new(),
        };
        changes
            .into_iter()
            .map(|change| SessionChange { key: key.clone(), change })
            .collect()
    }

    /// Resolve the approval card carrying `request_id` (after the local
    /// `approval.respond` RPC succeeded).
    pub fn resolve_approval(&mut self, key: &str, request_id: &str) -> Vec<SessionChange> {
        let index = self.find(|kind| match kind {
            RowKind::Approval(card) => card.request_id == request_id && !card.resolved,
            _ => false,
        });
        let mut changes = Vec::new();
        if let Some(index) = index {
            if let RowKind::Approval(card) = &mut self.state.rows[index].kind {
                card.resolved = true;
            }
            changes.push(TranscriptChange::RowUpdated { index });
        }
        wrap(key, changes)
    }

    /// Resolve the clarify card carrying `request_id` (after the local
    /// `clarify.respond` RPC succeeded): the pending question is cleared
    /// (marked resolved; both wire shapes).
    pub fn resolve_clarify(&mut self, key: &str, request_id: &str) -> Vec<SessionChange> {
        let index = self.find(|kind| match kind {
            RowKind::Clarify(card) => card.request_id == request_id && !card.resolved,
            _ => false,
        });
        let mut changes = Vec::new();
        if let Some(index) = index {
            if let RowKind::Clarify(card) = &mut self.state.rows[index].kind {
                card.resolved = true;
            }
            changes.push(TranscriptChange::RowUpdated { index });
        }
        wrap(key, changes)
    }

    /// Reset the transcript of the session `key` on resume (PLAN §1.5: on a
    /// truncated replay or changed epoch the transcript is rebuilt from the
    /// resume `messages`). Takes the key it belongs to; emits exactly one
    /// [`TranscriptChange::Reset`] for that key.
    pub fn reset_from_resume(&mut self, key: &str) -> Vec<SessionChange> {
        self.state = Transcript::default();
        self.streaming = None;
        self.thinking = None;
        self.tools.clear();
        vec![SessionChange { key: key.to_string(), change: TranscriptChange::Reset }]
    }

    // ── handlers ───────────────────────────────────────────────────────

    fn on_message_start(&mut self) -> Vec<TranscriptChange> {
        // A new turn closes the previous thinking streak: the next thinking
        // delta opens a fresh collapsible row.
        self.thinking = None;
        let index = self.state.rows.len();
        self.state.rows.push(Row {
            index,
            kind: RowKind::Assistant { text: String::new(), streaming: true, usage_json: String::new(), warning: String::new() },
        });
        self.streaming = Some(index);
        vec![TranscriptChange::RowAppended { index }]
    }

    fn on_message_delta(&mut self, payload: &Value) -> Vec<TranscriptChange> {
        let text = json::str_at(payload, "text");
        self.append_streaming_text(text)
    }

    fn on_message_interim(&mut self, payload: &Value) -> Vec<TranscriptChange> {
        // Verified wire shape (PLAN §4 T4): `{text, already_streamed}` — the
        // current streaming text is sealed as its own row and the streaming
        // continues in a fresh row. `already_streamed` says the text was
        // already delivered via deltas, so it is NOT appended again.
        let already_streamed = json::bool_at(payload, "already_streamed");
        let sealed = self.streaming.take();
        let mut changes = Vec::new();
        if let Some(index) = sealed {
            if let Some(row) = self.state.rows.get_mut(index) {
                if let RowKind::Assistant { streaming, .. } = &mut row.kind {
                    *streaming = false;
                }
            }
            changes.push(TranscriptChange::RowUpdated { index });
        }
        let _ = already_streamed;
        changes
    }

    fn on_message_complete(&mut self, payload: &Value) -> Vec<TranscriptChange> {
        // PLAN §1.4: replace streaming text with `text`, close the row.
        // A garbage (non-object) payload must not blank the row.
        if !payload.is_object() {
            return Vec::new();
        }
        let text = json::str_at(payload, "text");
        let usage = payload
            .get("usage")
            .map(json::to_compact_string)
            .unwrap_or_default();
        let warning = json::str_at(payload, "warning").to_string();
        let target = match self.streaming.take() {
            Some(index) => index,
            None => return Vec::new(),
        };
        if let RowKind::Assistant { text: slot, streaming, usage_json, warning: warn } =
            &mut self.state.rows[target].kind
        {
            slot.clear();
            slot.push_str(text);
            *streaming = false;
            *usage_json = usage;
            *warn = warning;
        }
        vec![TranscriptChange::RowUpdated { index: target }]
    }

    fn on_thinking_delta(&mut self, payload: &Value) -> Vec<TranscriptChange> {
        let text = json::str_at(payload, "text");
        if let Some(index) = self.thinking {
            if let RowKind::Thinking { text: slot } = &mut self.state.rows[index].kind {
                slot.push_str(text);
                return vec![TranscriptChange::RowUpdated { index }];
            }
        }
        let index = self.state.rows.len();
        self.state.rows.push(Row { index, kind: RowKind::Thinking { text: text.to_string() } });
        self.thinking = Some(index);
        vec![TranscriptChange::RowAppended { index }]
    }

    fn on_tool_generating(&mut self, payload: &Value) -> Vec<TranscriptChange> {
        // Verified wire shape: `{name}` — a placeholder card before
        // `tool.start` assigns the real tool_id.
        let name = json::str_at(payload, "name").to_string();
        let index = self.state.rows.len();
        self.state.rows.push(Row {
            index,
            kind: RowKind::Tool(ToolCard {
                tool_id: String::new(),
                name,
                complete: false,
                context: String::new(),
                args_json: String::new(),
                result_json: String::new(),
                duration_s: 0.0,
            }),
        });
        vec![TranscriptChange::RowAppended { index }]
    }

    fn on_tool_start(&mut self, payload: &Value) -> Vec<TranscriptChange> {
        // The wire always carries a tool_id here; without one (garbage or a
        // truncated frame) there is nothing to pair — ignore.
        let tool_id = json::str_at(payload, "tool_id").to_string();
        if tool_id.is_empty() {
            return Vec::new();
        }
        let name = json::str_at(payload, "name").to_string();
        let context = json::str_at(payload, "context").to_string();
        let args_json = payload.get("args").map(json::to_compact_string).unwrap_or_default();
        if let Some(&index) = self.tools.get(&tool_id) {
            // Re-start of a known id: update in place, never a second card.
            if let RowKind::Tool(card) = &mut self.state.rows[index].kind {
                card.name = name;
                card.context = context;
                card.args_json = args_json;
                card.complete = false;
            }
            return vec![TranscriptChange::RowUpdated { index }];
        }
        // Pair with the pending placeholder card from `tool.generating`
        // (same tool name, not yet paired): exactly ONE card per tool.
        if let Some(index) = self.pending_generating_row(&name) {
            if let RowKind::Tool(card) = &mut self.state.rows[index].kind {
                card.tool_id = tool_id.clone();
                card.context = context;
                card.args_json = args_json;
            }
            self.tools.insert(tool_id, index);
            return vec![TranscriptChange::RowUpdated { index }];
        }
        let index = self.state.rows.len();
        self.state.rows.push(Row {
            index,
            kind: RowKind::Tool(ToolCard {
                tool_id: tool_id.clone(),
                name,
                complete: false,
                context,
                args_json,
                result_json: String::new(),
                duration_s: 0.0,
            }),
        });
        self.tools.insert(tool_id, index);
        vec![TranscriptChange::RowAppended { index }]
    }

    fn on_tool_progress(&mut self, payload: &Value) -> Vec<TranscriptChange> {
        // PLAN §1.4: `{event_type, name?, preview?, ...}` — update the card
        // subtitle when a preview is present; no card is created or dropped.
        let preview = json::str_at(payload, "preview");
        if preview.is_empty() {
            return Vec::new();
        }
        let tool_id = json::str_at(payload, "tool_id").to_string();
        let index = self.tools.get(&tool_id).copied();
        let Some(index) = index else {
            return Vec::new();
        };
        match self.state.rows.get_mut(index).map(|r| &mut r.kind) {
            Some(RowKind::Tool(card)) => {
                card.context = preview.to_string();
                vec![TranscriptChange::RowUpdated { index }]
            }
            _ => Vec::new(),
        }
    }

    fn on_tool_complete(&mut self, payload: &Value) -> Vec<TranscriptChange> {
        let tool_id = json::str_at(payload, "tool_id").to_string();
        let result_json =
            payload.get("result").map(json::to_compact_string).unwrap_or_default();
        let duration_s = json::f64_at(payload, "duration_s");
        let index = self.tools.get(&tool_id).copied();
        let mut changes = Vec::new();
        if let Some(index) = index {
            if let RowKind::Tool(card) = &mut self.state.rows[index].kind {
                card.result_json = result_json;
                card.duration_s = duration_s;
                card.complete = true;
            }
            changes.push(TranscriptChange::RowUpdated { index });
        }
        changes
    }

    fn on_status_update(&mut self, payload: &Value) -> Vec<TranscriptChange> {
        // The wire always carries a `kind`; garbage (non-object payload or a
        // missing kind) is ignored instead of rendering an empty status.
        let kind_str = json::str_at(payload, "kind");
        if kind_str.is_empty() {
            return Vec::new();
        }
        let kind = StatusKind::parse(kind_str);
        let text = json::str_at(payload, "text").to_string();
        let index = self.state.rows.len();
        self.state.rows.push(Row { index, kind: RowKind::Status { kind, text } });
        vec![TranscriptChange::RowAppended { index }]
    }

    fn on_approval_request(&mut self, payload: &Value) -> Vec<TranscriptChange> {
        // `command` is already redacted and `choices` pre-computed
        // server-side — both are taken as-is, never recomputed (PLAN §4 T4).
        // Without a request_id the card could never be answered: ignore.
        let request_id = json::str_at(payload, "request_id").to_string();
        if request_id.is_empty() {
            return Vec::new();
        }
        let card = ApprovalCard {
            request_id,
            command: json::str_at(payload, "command").to_string(),
            description: json::str_at(payload, "description").to_string(),
            choices: str_list(payload.get("choices")),
            resolved: false,
        };
        let index = self.state.rows.len();
        self.state.rows.push(Row { index, kind: RowKind::Approval(card) });
        vec![TranscriptChange::RowAppended { index }]
    }

    fn on_clarify_request(&mut self, payload: &Value) -> Vec<TranscriptChange> {
        // Both shapes (PLAN §1.4): single-question `{question, choices,
        // multi_select?}` and batch `{questions:[{qid, question, ...}]}`.
        let request_id = json::str_at(payload, "request_id").to_string();
        let questions: Vec<ClarifyQuestion> = match payload.get("questions").and_then(Value::as_array) {
            Some(list) => list
                .iter()
                .map(|q| ClarifyQuestion {
                    qid: {
                        let qid = json::str_at(q, "qid");
                        if qid.is_empty() { "q0".to_string() } else { qid.to_string() }
                    },
                    question: json::str_at(q, "question").to_string(),
                    choices: str_list(q.get("choices")),
                    multi_select: json::bool_at(q, "multi_select"),
                })
                .collect(),
            None => {
                let question = json::str_at(payload, "question");
                if question.is_empty() {
                    return Vec::new();
                }
                vec![ClarifyQuestion {
                    qid: "q0".to_string(),
                    question: question.to_string(),
                    choices: str_list(payload.get("choices")),
                    multi_select: json::bool_at(payload, "multi_select"),
                }]
            }
        };
        if questions.is_empty() {
            return Vec::new();
        }
        let card = ClarifyCard { request_id, questions, resolved: false };
        let index = self.state.rows.len();
        self.state.rows.push(Row { index, kind: RowKind::Clarify(card) });
        vec![TranscriptChange::RowAppended { index }]
    }

    fn on_session_info(&mut self, payload: &Value) -> Vec<TranscriptChange> {
        let info = SessionHeader {
            title: json::str_at(payload, "title").to_string(),
            model: json::str_at(payload, "model").to_string(),
            cwd: json::str_at(payload, "cwd").to_string(),
            branch: json::str_at(payload, "branch").to_string(),
            usage_json: payload.get("usage").map(json::to_compact_string).unwrap_or_default(),
        };
        self.merge_header(info)
    }

    fn on_session_title(&mut self, payload: &Value) -> Vec<TranscriptChange> {
        let info = SessionHeader {
            title: json::str_at(payload, "title").to_string(),
            ..SessionHeader::default()
        };
        self.merge_header(info)
    }

    fn on_error(&mut self, payload: &Value) -> Vec<TranscriptChange> {
        let message = json::str_at(payload, "message").to_string();
        let index = self.state.rows.len();
        self.state.rows.push(Row { index, kind: RowKind::Error { message } });
        vec![TranscriptChange::RowAppended { index }]
    }

    // ── internals ───────────────────────────────────────────────────────

    fn append_streaming_text(&mut self, text: &str) -> Vec<TranscriptChange> {
        if text.is_empty() {
            // Coalesced frames can carry an empty delta — nothing to append.
            return Vec::new();
        }
        if let Some(index) = self.streaming {
            if let Some(row) = self.state.rows.get_mut(index) {
                if let RowKind::Assistant { text: slot, .. } = &mut row.kind {
                    slot.push_str(text);
                    return vec![TranscriptChange::RowUpdated { index }];
                }
            }
        }
        // Deltas without a `message.start` (defensive): degrade by opening
        // an assistant row rather than dropping the text.
        let index = self.state.rows.len();
        self.state.rows.push(Row {
            index,
            kind: RowKind::Assistant { text: text.to_string(), streaming: true, usage_json: String::new(), warning: String::new() },
        });
        self.streaming = Some(index);
        vec![TranscriptChange::RowAppended { index }]
    }

    fn pending_generating_row(&self, name: &str) -> Option<usize> {
        self.state.rows.iter().position(|row| {
            matches!(&row.kind, RowKind::Tool(card) if card.tool_id.is_empty() && card.name == name)
        })
    }

    fn find(&self, pred: impl Fn(&RowKind) -> bool) -> Option<usize> {
        self.state.rows.iter().position(|row| pred(&row.kind))
    }

    fn merge_header(&mut self, info: SessionHeader) -> Vec<TranscriptChange> {
        if info.title.is_empty() && info.model.is_empty() && info.cwd.is_empty() && info.branch.is_empty() && info.usage_json.is_empty() {
            return Vec::new();
        }
        if !info.title.is_empty() {
            self.state.header.title = info.title;
        }
        if !info.model.is_empty() {
            self.state.header.model = info.model;
        }
        if !info.cwd.is_empty() {
            self.state.header.cwd = info.cwd;
        }
        if !info.branch.is_empty() {
            self.state.header.branch = info.branch;
        }
        if !info.usage_json.is_empty() {
            self.state.header.usage_json = info.usage_json;
        }
        vec![TranscriptChange::HeaderUpdated]
    }
}

fn wrap(key: &str, changes: Vec<TranscriptChange>) -> Vec<SessionChange> {
    changes
        .into_iter()
        .map(|change| SessionChange { key: key.to_string(), change })
        .collect()
}

/// String list from a JSON value; non-string entries and non-arrays degrade
/// to an empty list, never a panic.
fn str_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

// ── unit tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Helper: an `EventParams` for session `s1`.
    fn event(event_type: &str, payload: Value) -> EventParams {
        EventParams {
            event_type: event_type.to_string(),
            session_id: "s1".to_string(),
            seq: None,
            payload,
        }
    }

    fn assistant_text(row: &Row) -> &str {
        match &row.kind {
            RowKind::Assistant { text, .. } => text,
            other => panic!("row {} is not an assistant row: {:?}", row.index, other),
        }
    }

    /// Rule pinned: `message.complete` REPLACES the streamed text — the
    /// assistant row's text must equal exactly the `complete` text, not a
    /// concatenation of deltas + complete.
    #[test]
    fn message_complete_replaces_streamed_text() {
        let mut r = Reducer::new();
        r.apply(&event("message.start", json!({})));
        r.apply(&event("message.delta", json!({"text": "Hel"})));
        r.apply(&event("message.delta", json!({"text": "lo"})));
        r.apply(&event("message.complete", json!({"text": "Hello, world.", "status": "done"})));
        let t = r.transcript();
        assert_eq!(t.rows.len(), 1, "one assistant row, not one per delta");
        assert_eq!(assistant_text(&t.rows[0]), "Hello, world.");
        match &t.rows[0].kind {
            RowKind::Assistant { streaming, usage_json, .. } => {
                assert!(!streaming, "complete closes the row");
                assert!(usage_json.is_empty(), "no usage field -> empty");
            }
            other => panic!("expected Assistant, got {:?}", other),
        }
    }

    /// Rule pinned: `message.complete` carries `usage` through as a compact
    /// JSON string (PLAN §3: usage crosses untyped).
    #[test]
    fn message_complete_keeps_usage_json() {
        let mut r = Reducer::new();
        r.apply(&event("message.start", json!({})));
        r.apply(&event(
            "message.complete",
            json!({"text": "done", "usage": {"total": 42}}),
        ));
        match &r.transcript().rows[0].kind {
            RowKind::Assistant { usage_json, .. } => {
                assert_eq!(usage_json, r#"{"total":42}"#);
            }
            other => panic!("expected Assistant, got {:?}", other),
        }
    }

    /// Rule pinned: interim seals the streaming assistant row
    /// (`{text, already_streamed}` shape); the next delta opens a NEW row
    /// and the sealed row keeps its own text.
    #[test]
    fn message_interim_seals_current_assistant_row() {
        let mut r = Reducer::new();
        r.apply(&event("message.start", json!({})));
        r.apply(&event("message.delta", json!({"text": "part one"})));
        r.apply(&event(
            "message.interim",
            json!({"text": "part one", "already_streamed": true}),
        ));
        // Streaming continues: a new assistant row opens on the next delta.
        r.apply(&event("message.delta", json!({"text": "part two"})));
        let t = r.transcript();
        assert_eq!(t.rows.len(), 2, "sealed row + fresh streaming row");
        assert_eq!(assistant_text(&t.rows[0]), "part one");
        assert_eq!(assistant_text(&t.rows[1]), "part two");
        match &t.rows[0].kind {
            RowKind::Assistant { streaming, .. } => assert!(!streaming, "sealed row is closed"),
            other => panic!("expected Assistant, got {:?}", other),
        }
        match &t.rows[1].kind {
            RowKind::Assistant { streaming, .. } => assert!(streaming, "new row streams"),
            other => panic!("expected Assistant, got {:?}", other),
        }
    }

    /// Rule pinned: `tool.generating` → `start` → `complete` produces
    /// exactly ONE card for the tool id, updated in place; `complete` is the
    /// only step that sets the terminal state.
    #[test]
    fn one_tool_card_per_tool_id_updated_in_place() {
        let mut r = Reducer::new();
        r.apply(&event("tool.generating", json!({"name": "terminal"})));
        r.apply(&event(
            "tool.start",
            json!({"tool_id": "t1", "name": "terminal", "context": "echo hi", "args": {"command": "echo hi"}}),
        ));
        r.apply(&event(
            "tool.complete",
            json!({"tool_id": "t1", "name": "terminal", "result": {"output": "hi"}, "duration_s": 0.5}),
        ));
        let t = r.transcript();
        assert_eq!(t.rows.len(), 1, "generating+start+complete = ONE card");
        match &t.rows[0].kind {
            RowKind::Tool(card) => {
                assert_eq!(card.tool_id, "t1");
                assert_eq!(card.name, "terminal");
                assert_eq!(card.context, "echo hi");
                assert_eq!(card.args_json, r#"{"command":"echo hi"}"#);
                assert_eq!(card.result_json, r#"{"output":"hi"}"#);
                assert!((card.duration_s - 0.5).abs() < 1e-9);
                assert!(card.complete, "terminal state only via tool.complete");
            }
            other => panic!("expected Tool card, got {:?}", other),
        }
    }

    /// Rule pinned: `tool.progress` updates the existing card's subtitle and
    /// never creates a second card (no `tool.progress` frame appeared in the
    /// T0 fixture — this pins the rule from PLAN §1.4).
    #[test]
    fn tool_progress_updates_card_without_new_rows() {
        let mut r = Reducer::new();
        r.apply(&event("tool.start", json!({"tool_id": "t1", "name": "fetch"})));
        let changes = r.apply(&event(
            "tool.progress",
            json!({"tool_id": "t1", "event_type": "output", "preview": "12 lines"}),
        ));
        let t = r.transcript();
        assert_eq!(t.rows.len(), 1, "progress must not add a card");
        match &t.rows[0].kind {
            RowKind::Tool(card) => assert_eq!(card.context, "12 lines"),
            other => panic!("expected Tool card, got {:?}", other),
        }
        assert!(matches!(changes[0].change, TranscriptChange::RowUpdated { index: 0 }));
    }

    /// Rule pinned: approval `choices` and `command` are taken from the
    /// event verbatim — never recomputed (PLAN §4 T4: server pre-computes
    /// both; the synthetic fixture carries the verified subset).
    #[test]
    fn approval_choices_and_command_taken_verbatim_not_recomputed() {
        let mut r = Reducer::new();
        // allow_session/allow_permanent=false on the wire -> the server
        // computed a SUBSET; a recomputation would yield all four choices.
        r.apply(&event(
            "approval.request",
            json!({
                "request_id": "req-synth-2",
                "command": "git push --force",
                "description": "Force-push a branch",
                "choices": ["once", "deny"],
                "allow_session": false,
                "allow_permanent": false,
                "smart_denied": false
            }),
        ));
        match &r.transcript().rows[0].kind {
            RowKind::Approval(card) => {
                assert_eq!(card.request_id, "req-synth-2");
                assert_eq!(card.command, "git push --force", "redacted command as-is");
                assert_eq!(card.choices, vec!["once", "deny"], "server subset, NOT recomputed");
                assert!(!card.resolved);
            }
            other => panic!("expected Approval card, got {:?}", other),
        }
        // resolve_approval is the ONLY thing that resolves the card.
        let changes = r.resolve_approval("s1", "req-synth-2");
        assert!(matches!(changes[0].change, TranscriptChange::RowUpdated { index: 0 }));
        match &r.transcript().rows[0].kind {
            RowKind::Approval(card) => assert!(card.resolved, "resolved only via resolve_approval"),
            other => panic!("expected Approval card, got {:?}", other),
        }
        // Resolving twice must not emit anything.
        assert!(r.resolve_approval("s1", "req-synth-2").is_empty(), "no double resolve");
    }

    /// Rule pinned: both clarify payload shapes produce a card; the batch
    /// shape keeps the wire `qid`s, the single shape defaults to `q0`;
    /// resolve_clarify clears the pending question.
    #[test]
    fn clarify_both_shapes_and_resolution() {
        let mut r = Reducer::new();
        // Single-question shape.
        r.apply(&event(
            "clarify.request",
            json!({"request_id": "c1", "question": "Continue?", "choices": ["Yes", "No"], "multi_select": false}),
        ));
        // Batch shape (the fixture's shape).
        r.apply(&event(
            "clarify.request",
            json!({
                "request_id": "c2",
                "questions": [
                    {"qid": "q0", "question": "Quale colore preferisci, blu o verde?", "choices": ["Blu", "Verde"], "multi_select": false}
                ]
            }),
        ));
        let t = r.transcript();
        assert_eq!(t.rows.len(), 2);
        match &t.rows[0].kind {
            RowKind::Clarify(card) => {
                assert_eq!(card.request_id, "c1");
                assert_eq!(card.questions.len(), 1);
                assert_eq!(card.questions[0].qid, "q0", "single shape defaults to q0");
                assert_eq!(card.questions[0].choices, vec!["Yes", "No"]);
                assert!(!card.resolved);
            }
            other => panic!("expected Clarify card, got {:?}", other),
        }
        match &t.rows[1].kind {
            RowKind::Clarify(card) => {
                assert_eq!(card.request_id, "c2");
                assert_eq!(card.questions[0].qid, "q0", "batch qid kept");
                assert_eq!(card.questions[0].question, "Quale colore preferisci, blu o verde?");
            }
            other => panic!("expected Clarify card, got {:?}", other),
        }
        // resolve_clarify clears the pending question...
        let changes = r.resolve_clarify("s1", "c1");
        assert!(matches!(changes[0].change, TranscriptChange::RowUpdated { index: 0 }));
        let t = r.transcript();
        match &t.rows[0].kind {
            RowKind::Clarify(card) => assert!(card.resolved, "pending question cleared"),
            other => panic!("expected Clarify card, got {:?}", other),
        }
        // ...and leaves the OTHER card untouched.
        match &t.rows[1].kind {
            RowKind::Clarify(card) => assert!(!card.resolved),
            other => panic!("expected Clarify card, got {:?}", other),
        }
        assert!(r.resolve_clarify("s1", "c1").is_empty(), "no double resolve");
    }

    /// Rule pinned: `status.update` kinds map to the verified value set
    /// (status/lifecycle/compacting/compacted/heartbeat); unknown kinds
    /// degrade to Other without panicking.
    #[test]
    fn status_kinds_cover_verified_set() {
        let mut r = Reducer::new();
        for (i, kind) in ["status", "lifecycle", "compacting", "compacted", "heartbeat", "weird"]
            .iter()
            .enumerate()
        {
            r.apply(&event("status.update", json!({"kind": kind, "text": "t"})));
            match &r.transcript().rows[i].kind {
                RowKind::Status { kind: parsed, text } => {
                    let expected = match *kind {
                        "status" => StatusKind::Status,
                        "lifecycle" => StatusKind::Lifecycle,
                        "compacting" => StatusKind::Compacting,
                        "compacted" => StatusKind::Compacted,
                        "heartbeat" => StatusKind::Heartbeat,
                        _ => StatusKind::Other("weird".to_string()),
                    };
                    assert_eq!(*parsed, expected, "kind {}", kind);
                    assert_eq!(text, "t");
                }
                other => panic!("expected Status row, got {:?}", other),
            }
        }
    }

    /// Rule pinned: session.info and session.title merge into the session
    /// header (no per-update rows).
    #[test]
    fn session_info_and_title_update_the_header() {
        let mut r = Reducer::new();
        r.apply(&event("session.info", json!({"model": "m1", "cwd": "/c", "usage": {"total": 3}})));
        r.apply(&event("session.title", json!({"title": "my chat"})));
        let h = &r.transcript().header;
        assert_eq!(h.model, "m1");
        assert_eq!(h.cwd, "/c");
        assert_eq!(h.usage_json, r#"{"total":3}"#);
        assert_eq!(h.title, "my chat", "title merges without dropping model/cwd");
        assert!(r.transcript().rows.is_empty(), "header events never create rows");
    }

    /// Rule pinned: session-less events (`gateway.ready`, connection state,
    /// skin) are NOT reducer input — they emit nothing and add no row.
    #[test]
    fn sessionless_events_emit_nothing_and_add_no_rows() {
        let mut r = Reducer::new();
        let ready = EventParams {
            event_type: "gateway.ready".to_string(),
            session_id: String::new(),
            seq: None,
            payload: json!({"replay_epoch": "e1"}),
        };
        assert!(r.apply(&ready).is_empty());
        // Also: known event types but empty session_id (no session bound).
        assert!(r.apply(&event_no_session("message.start")).is_empty());
        assert!(r.apply(&event_no_session("message.complete")).is_empty());
        assert!(r.apply(&event_no_session("error")).is_empty());
        assert!(r.transcript().rows.is_empty(), "no rows from session-less events");
        assert!(r.transcript().header.title.is_empty());
    }

    fn event_no_session(event_type: &str) -> EventParams {
        EventParams {
            event_type: event_type.to_string(),
            session_id: String::new(),
            seq: None,
            payload: json!({"text": "x"}),
        }
    }

    /// Rule pinned: unknown event types, missing fields and garbage payloads
    /// are ignored without panicking (PI_ROLE defensive rule).
    #[test]
    fn unknown_events_and_garbage_payloads_never_panic() {
        let mut r = Reducer::new();
        assert!(r.apply(&event("subagent.spawn", json!({"x": 1}))).is_empty());
        assert!(r.apply(&event("notification.show", json!({}))).is_empty());
        // Known types with garbage payloads: degrade, no panic, no crash.
        assert!(r.apply(&event("message.delta", json!({"text": 42}))).is_empty());
        assert!(r.apply(&event("message.delta", json!(null))).is_empty());
        assert!(r.apply(&event("tool.start", json!("garbage"))).is_empty());
        assert!(r.apply(&event("approval.request", json!([1, 2, 3]))).is_empty());
        assert!(r.apply(&event("clarify.request", json!({"request_id": 9}))).is_empty());
        assert!(r.apply(&event("status.update", json!({"kind": 5}))).is_empty());
        assert!(r.apply(&event("session.info", json!("not an object"))).is_empty());
        // Missing payload entirely.
        let bare = EventParams {
            event_type: "message.delta".to_string(),
            session_id: "s1".to_string(),
            seq: Some(1),
            payload: Value::Null,
        };
        assert!(r.apply(&bare).is_empty());
        // The reducer is still usable afterwards.
        r.apply(&event("message.start", json!({})));
        assert_eq!(r.transcript().rows.len(), 1);
    }

    /// Rule pinned: every emitted change carries the session key of its
    /// event — multi-session routing from birth (PLAN decision 10).
    #[test]
    fn changes_carry_their_session_key() {
        let mut r = Reducer::new();
        let mut ev = event("message.start", json!({}));
        ev.session_id = "sess-A".to_string();
        let changes = r.apply(&ev);
        assert!(changes.iter().all(|c| c.key == "sess-A"), "key must ride every change");
        assert!(matches!(changes[0].change, TranscriptChange::RowAppended { index: 0 }));
        let mut ev = event("message.complete", json!({"text": "done"}));
        ev.session_id = "sess-A".to_string();
        let changes = r.apply(&ev);
        assert!(matches!(
            changes[0],
            SessionChange { key: ref k, change: TranscriptChange::RowUpdated { index: 0 } } if k == "sess-A"
        ));
    }

    /// Rule pinned: `reset_from_resume` wipes the transcript of its session
    /// and emits exactly one Reset for that key.
    #[test]
    fn reset_from_resume_wipes_and_emits_reset() {
        let mut r = Reducer::new();
        r.apply(&event("message.start", json!({})));
        r.apply(&event("message.complete", json!({"text": "old"})));
        let changes = r.reset_from_resume("s1");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].key, "s1");
        assert!(matches!(changes[0].change, TranscriptChange::Reset));
        assert!(r.transcript().rows.is_empty());
        // The reducer is reusable after a reset.
        r.apply(&event("message.start", json!({})));
        assert_eq!(r.transcript().rows.len(), 1);
    }

    /// Rule pinned: `thinking.delta` and `reasoning.delta` append into ONE
    /// collapsible thinking row until an assistant row takes over; a second
    /// thinking streak after an assistant row is a NEW row.
    #[test]
    fn thinking_deltas_accumulate_and_restart_after_assistant() {
        let mut r = Reducer::new();
        r.apply(&event("thinking.delta", json!({"text": "a"})));
        r.apply(&event("reasoning.delta", json!({"text": "b"})));
        r.apply(&event("message.start", json!({})));
        r.apply(&event("message.complete", json!({"text": "x"})));
        r.apply(&event("thinking.delta", json!({"text": "c"})));
        let t = r.transcript();
        assert_eq!(t.rows.len(), 3, "thinking, assistant, thinking");
        assert_eq!(t.kind_at(0), "thinking");
        match &t.rows[0].kind {
            RowKind::Thinking { text } => assert_eq!(text, "ab", "deltas accumulate"),
            other => panic!("expected Thinking, got {:?}", other),
        }
        match &t.rows[2].kind {
            RowKind::Thinking { text } => assert_eq!(text, "c", "new streak = new row"),
            other => panic!("expected Thinking, got {:?}", other),
        }
    }
}
