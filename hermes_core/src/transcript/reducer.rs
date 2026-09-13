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

    /// True while an assistant row is streaming (`message.start` seen, no
    /// `message.complete`/`interim` yet) — the "running" flag of the open
    /// tabs (PLAN §4 T5: `open_sessions()` carries running flags).
    pub fn is_streaming(&self) -> bool {
        self.streaming.is_some()
    }

    /// True when an unresolved approval card with this `request_id` is
    /// already on the transcript. The use case uses it to de-duplicate
    /// re-emitted `approval.pending` cards after a reconnect (PLAN §4 T5:
    /// replays by `request_id`) without duplicating rows.
    pub fn has_unresolved_approval(&self, request_id: &str) -> bool {
        self.state.rows.iter().any(|row| {
            matches!(
                &row.kind,
                RowKind::Approval(card) if card.request_id == request_id && !card.resolved
            )
        })
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
        Self::wrap(key, changes)
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
        Self::wrap(key, changes)
    }

    /// Reset the transcript of the session `key` on resume (PLAN §1.5: on a
    /// truncated replay or changed epoch the transcript is rebuilt from the
    /// resume `messages`). Takes the key it belongs to; emits exactly one
    /// [`TranscriptChange::Reset`] for that key.
    ///
    /// NOTE (PR #10 nit): this method has NO production caller — the shipping
    /// rebuild path is [`Reducer::ingest_resume_messages`], which emits one
    /// Reset plus one `RowAppended` per rebuilt row (the change stream is the
    /// single source of rows). The single-`Reset` contract here is still
    /// coherent for a wipe-WITHOUT-rebuild site (clear the transcript and let
    /// the following events repaint it); do not "fix" it into the ingest
    /// shape.
    pub fn reset_from_resume(&mut self, key: &str) -> Vec<SessionChange> {
        self.state = Transcript::default();
        self.streaming = None;
        self.thinking = None;
        self.tools.clear();
        vec![SessionChange { key: key.to_string(), change: TranscriptChange::Reset }]
    }

    /// Ingest the `messages: [Transcript]` array that `session.create` and
    /// `session.resume` return (review #4 carry-over): a second input shape,
    /// not an [`EventParams`]. Clears the transcript first (a resume
    /// replaces history) and then renders the messages in list order, so a
    /// resumed session shows its history without waiting for events.
    ///
    /// Emit contract (T7c): [`TranscriptChange::Reset`] FIRST (the authority
    /// that the history was replaced), then one
    /// [`TranscriptChange::RowAppended` { index }] per rebuilt row, in list
    /// order, at the same index stamped on each `Row` — the change stream is
    /// the single source of rows, so the app renders the rebuilt history
    /// from the stream and needs no local snapshot.
    ///
    /// Assumed message shape (PLAN §1.3, from `tui_gateway/session_history.py`
    /// `_history_to_messages` / `gatewayTypes.ts` `GatewayTranscriptMessage`):
    /// `{role: "user"|"assistant"|"system"|"tool", text?, name?, context?,
    /// display_kind?, display_metadata?}`. Tool entries may carry a `tool_id`
    /// in `display_metadata` — tolerated, never required (PI_ROLE defensive
    /// rule): missing/unknown fields degrade to defaults, never panic.
    pub fn ingest_resume_messages(
        &mut self,
        key: &str,
        messages: &[serde_json::Value],
    ) -> Vec<SessionChange> {
        self.state = Transcript::default();
        self.streaming = None;
        self.thinking = None;
        self.tools.clear();
        let mut changes =
            vec![SessionChange { key: key.to_string(), change: TranscriptChange::Reset }];
        for msg in messages {
            if !msg.is_object() {
                continue;
            }
            let role = json::str_at(msg, "role");
            let text = json::str_at(msg, "text").to_string();
            let name = json::str_at(msg, "name").to_string();
            let context = json::str_at(msg, "context").to_string();
            let index = self.state.rows.len();
            let row = match role {
                "tool" => {
                    let card = ToolCard {
                        // A resumed tool message re-pairs by id when the
                        // server carries one in display_metadata; absent ->
                        // empty (never re-computed).
                        tool_id: json::str_at(
                            msg.get("display_metadata").unwrap_or(&Value::Null),
                            "tool_id",
                        )
                        .to_string(),
                        name,
                        complete: true,
                        context,
                        args_json: String::new(),
                        result_json: text,
                        duration_s: 0.0,
                    };
                    if !card.tool_id.is_empty() {
                        self.tools.insert(card.tool_id.clone(), index);
                    }
                    Row { index, kind: RowKind::Tool(card) }
                }
                "user" | "system" => Row { index, kind: RowKind::User { text } },
                // "assistant" and anything unknown: degrade to an assistant
                // row (defensive: the history must render, not vanish).
                _ => Row { index, kind: RowKind::Assistant { text, streaming: false, usage_json: String::new(), warning: String::new() } },
            };
            self.state.rows.push(row);
            changes.push(SessionChange {
                key: key.to_string(),
                change: TranscriptChange::RowAppended { index },
            });
        }
        changes
    }

    /// Append the user's own row for a submitted prompt (T7c): pushes a
    /// [`RowKind::User`] at `rows.len()` and returns one
    /// [`TranscriptChange::RowAppended` { index }] for it. No other state is
    /// touched — no streaming latch, no tools map, no turn bookkeeping: the
    /// turn's own rows remain the wire events' business. The use case
    /// (`core.rs::send`) calls this after a successful `prompt.submit`,
    /// because the server never emits a row event for the user's message and
    /// the change stream is the single source of rows.
    pub fn append_user_row(&mut self, key: &str, text: &str) -> Vec<SessionChange> {
        let index = self.state.rows.len();
        self.state
            .rows
            .push(Row { index, kind: RowKind::User { text: text.to_string() } });
        Self::wrap(key, vec![TranscriptChange::RowAppended { index }])
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
        // continues in a fresh row.
        //
        // `already_streamed` decides what the sealed row holds (review #4,
        // should 2): when TRUE the text already arrived as deltas, so it must
        // NOT be applied again (appending would duplicate it). When FALSE the
        // interim text never arrived as a delta, so it IS the row's text —
        // discarding it would store an empty assistant row.
        let already_streamed = json::bool_at(payload, "already_streamed");
        let sealed = self.streaming.take();
        let mut changes = Vec::new();
        if let Some(index) = sealed {
            if let Some(row) = self.state.rows.get_mut(index) {
                if let RowKind::Assistant { text: slot, streaming, .. } = &mut row.kind {
                    *streaming = false;
                    if !already_streamed {
                        slot.clear();
                        slot.push_str(json::str_at(payload, "text"));
                    }
                }
            }
            changes.push(TranscriptChange::RowUpdated { index });
        }
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
        // `heartbeat` is a liveness ping with no user-facing text and the
        // gateway emits it every few seconds: a row per ping would flood the
        // transcript (review #4, nit). It stays parseable but is not a row.
        // User-visible status text renders as a status strip (PLAN §1.4).
        if kind == StatusKind::Heartbeat {
            return Vec::new();
        }
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

    /// Wrap changes with the session key (internal helper of the wrappers).
    fn wrap(key: &str, changes: Vec<TranscriptChange>) -> Vec<SessionChange> {
        changes
            .into_iter()
            .map(|change| SessionChange { key: key.to_string(), change })
            .collect()
    }

    /// Apply one `approval.pending` card (a JSON object of the same shape
    /// as the `approval.request` event payload) as an approval row. The use
    /// case (`core.rs`) calls this when re-emitting unresolved cards after
    /// a (re)connect; it has ALREADY deduped by `request_id`
    /// (`has_unresolved_approval` + its own emitted set). Returns the
    /// routable changes, or `None` when the card carried no usable
    /// `request_id` (defensive: garbage never becomes a row).
    pub fn apply_approval_card(
        &mut self,
        key: &str,
        request_id: &str,
        card: &serde_json::Value,
    ) -> Option<Vec<SessionChange>> {
        if request_id.is_empty() {
            return None;
        }
        // The event handler reads `request_id` from the payload; inject it
        // so both entry points share one row-building rule.
        let mut payload = card.clone();
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("request_id".to_string(), serde_json::json!(request_id));
        }
        let ev = EventParams {
            event_type: "approval.request".to_string(),
            session_id: key.to_string(),
            seq: None,
            payload,
        };
        let changes = self.apply(&ev);
        if changes.is_empty() {
            None
        } else {
            Some(changes)
        }
    }
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

    /// Rule pinned: an interim whose text did NOT arrive as deltas
    /// (`already_streamed: false`) puts that text into the sealed row. The
    /// pre-fix handler bound the field and dropped it, leaving an empty
    /// assistant row (review #4, should 2). The T0 recording has no
    /// `message.interim` frame at all, so only this unit test can catch it.
    #[test]
    fn message_interim_carries_text_when_not_already_streamed() {
        let mut r = Reducer::new();
        r.apply(&event("message.start", json!({})));
        r.apply(&event(
            "message.interim",
            json!({"text": "partial answer", "already_streamed": false}),
        ));
        let t = r.transcript();
        assert_eq!(t.rows.len(), 1, "the interim seals, it does not append a row");
        assert_eq!(assistant_text(&t.rows[0]), "partial answer");
        match &t.rows[0].kind {
            RowKind::Assistant { streaming, .. } => assert!(!streaming, "sealed row is closed"),
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
        let visible = ["status", "lifecycle", "compacting", "compacted", "weird"];
        for (i, kind) in visible.iter().enumerate() {
            r.apply(&event("status.update", json!({"kind": kind, "text": "t"})));
            match &r.transcript().rows[i].kind {
                RowKind::Status { kind: parsed, text } => {
                    let expected = match *kind {
                        "status" => StatusKind::Status,
                        "lifecycle" => StatusKind::Lifecycle,
                        "compacting" => StatusKind::Compacting,
                        "compacted" => StatusKind::Compacted,
                        _ => StatusKind::Other("weird".to_string()),
                    };
                    assert_eq!(*parsed, expected, "kind {}", kind);
                    assert_eq!(text, "t");
                }
                other => panic!("expected Status row, got {:?}", other),
            }
        }
    }

    /// Rule pinned: `heartbeat` stays parseable but is NEVER a transcript row.
    /// A ping every few seconds would otherwise flood the transcript, and the
    /// pre-fix handler appended one row per ping (review #4, nit).
    #[test]
    fn heartbeat_status_is_not_a_row() {
        assert_eq!(StatusKind::parse("heartbeat"), StatusKind::Heartbeat, "still parseable");
        let mut r = Reducer::new();
        r.apply(&event("status.update", json!({"kind": "status", "text": "working"})));
        let changes = r.apply(&event("status.update", json!({"kind": "heartbeat", "text": "ping"})));
        assert!(changes.is_empty(), "heartbeat emits no change");
        assert_eq!(r.transcript().rows.len(), 1, "heartbeat adds no row");
        assert_eq!(r.transcript().rows[0].index, 0, "existing rows untouched");
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

    /// Review #4 carry-over rule pinned: `ingest_resume_messages` renders a
    /// resumed session's history — the `messages: [Transcript]` array of
    /// `session.resume` (PLAN §1.3 shape) — into rows, without any event.
    /// Removing the constructor, or rebuilding history only from events,
    /// leaves a resumed transcript empty and this test fails.
    #[test]
    fn resume_messages_ingest_renders_history_without_events() {
        let mut r = Reducer::new();
        // Pre-existing state must be replaced, not appended to.
        r.apply(&event("message.complete", json!({"text": "stale"})));
        let messages = vec![
            json!({"role": "user", "text": "hi there"}),
            json!({"role": "assistant", "text": "hello back"}),
            json!({
                "role": "tool",
                "name": "terminal",
                "context": "echo hi",
                "text": "{\"output\": \"hi\"}",
                "display_metadata": {"tool_id": "t9"}
            }),
            // Defensive shapes: a message with no role and a non-object entry
            // must degrade, never panic.
            json!({"role": "assistant"}),
            json!("not an object"),
        ];
        let changes = r.ingest_resume_messages("s-resume", &messages);
        // T7c contract: the Reset is the authority that the history was
        // replaced, and the rebuilt rows FOLLOW it as one `RowAppended` per
        // row, in list order — the change stream is the single source of
        // rows, so the app must receive the rebuilt history through it.
        // Pre-fix this returned EXACTLY ONE Reset and the rebuilt rows were
        // visible to no one (the app replayed a stale local snapshot).
        assert_eq!(changes[0].key, "s-resume");
        assert!(matches!(changes[0].change, TranscriptChange::Reset), "Reset comes FIRST");
        let kinds: Vec<String> = changes
            .iter()
            .map(|c| match c.change {
                TranscriptChange::Reset => "reset".to_string(),
                TranscriptChange::RowAppended { index } => format!("row@{index}"),
                ref other => format!("other:{other:?}"),
            })
            .collect();
        assert_eq!(
            changes.len(),
            5,
            "one Reset + one RowAppended per rebuilt row (4 objects, garbage skipped): {kinds:?}"
        );
        assert_eq!(kinds, vec!["reset", "row@0", "row@1", "row@2", "row@3"], "in list order");
        for (i, change) in changes[1..].iter().enumerate() {
            assert_eq!(change.key, "s-resume", "every change carries the session key");
            assert!(
                matches!(change.change, TranscriptChange::RowAppended { index } if index == i),
                "index {} matches the rebuilt row's index (stamped on each Row)",
                i
            );
        }
        // The FFI mapping the resume path uses (`change_to_dto`) renders each
        // appended row: a delivered RowAppended must carry its row_json (the
        // app decodes the row from it — an empty row_json is an empty row).
        let rebuilt = r.transcript().rows.clone();
        for (i, change) in changes[1..].iter().enumerate() {
            let dto = crate::core::change_to_dto("s-resume", &change.change, &rebuilt);
            assert_eq!(dto.index as usize, i);
            assert!(!dto.row_json.is_empty(), "row {} must render its row_json", i);
        }
        // Expected-JSON assertion on the FIRST rebuilt row (PR #10 nit: the
        // previous assertion compared `change_to_dto` against ITSELF at the
        // same index and could only pass). Row 0 of the rebuilt history is
        // the user message "hi there"; its delivered payload must be exactly
        // the serialized user row — nothing else can pass.
        let first = crate::core::change_to_dto("s-resume", &changes[1].change, &rebuilt);
        assert_eq!(first.row_json, r#"{"kind":"user","text":"hi there"}"#);
        let t = r.transcript();
        // Non-object entries are skipped, the four objects become rows in
        // list order: user, assistant, tool, assistant.
        assert_eq!(t.rows.len(), 4, "user+assistant+tool+assistant, garbage skipped");
        assert_eq!(t.kind_at(0), "user");
        match &t.rows[0].kind {
            RowKind::User { text } => assert_eq!(text, "hi there"),
            other => panic!("expected User, got {:?}", other),
        }
        match &t.rows[1].kind {
            RowKind::Assistant { text, streaming, .. } => {
                assert_eq!(text, "hello back");
                assert!(!streaming, "history rows are not streaming");
            }
            other => panic!("expected Assistant, got {:?}", other),
        }
        match &t.rows[2].kind {
            RowKind::Tool(card) => {
                assert_eq!(card.name, "terminal");
                assert_eq!(card.tool_id, "t9", "tool_id from display_metadata");
                assert_eq!(card.result_json, r#"{"output": "hi"}"#);
                assert!(card.complete, "a resumed tool message is finished");
            }
            other => panic!("expected Tool, got {:?}", other),
        }
        match &t.rows[3].kind {
            RowKind::Assistant { text, .. } => assert_eq!(text, "", "missing text degrades"),
            other => panic!("expected Assistant, got {:?}", other),
        }
        // The stale pre-resume rows are gone.
        assert!(t.rows.iter().all(|row| match &row.kind {
            RowKind::Assistant { text, .. } => text != "stale",
            _ => true,
        }));
        // The reducer keeps working after the ingest: a new event applies on
        // top of the rebuilt history.
        let mut ev = event("message.start", json!({}));
        ev.session_id = "s-resume".into();
        r.apply(&ev);
        assert_eq!(r.transcript().rows.len(), 5);
    }

    /// Rule pinned: an EMPTY resume messages array still resets (a resumed
    /// session that legitimately has no history renders an empty transcript,
    /// not the stale pre-resume rows). T7c contract, N=0 case: the emit is
    /// Reset + one RowAppended per rebuilt row — an empty history is Reset
    /// with ZERO appends, never a Reset plus a phantom row.
    #[test]
    fn empty_resume_messages_still_resets() {
        let mut r = Reducer::new();
        r.apply(&event("message.complete", json!({"text": "old"})));
        let changes = r.ingest_resume_messages("s1", &[]);
        assert_eq!(changes.len(), 1, "Reset with zero RowAppended for an empty history");
        assert_eq!(changes[0].key, "s1");
        assert!(matches!(changes[0].change, TranscriptChange::Reset));
        assert!(r.transcript().rows.is_empty(), "history replaced, not merged");
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
