//! Transcript data model (entity layer, PLAN §4 T4): plain data only.
//!
//! Mapping to PLAN §1.4 (the events the one screen must handle):
//!
//! | PLAN event / render                     | [`Row`] kind        |
//! | --------------------------------------- | ------------------- |
//! | user message (submitted locally)        | [`RowKind::User`]   |
//! | `message.start/delta/interim/complete`  | [`RowKind::Assistant`] |
//! | `thinking.delta`, `reasoning.delta`     | [`RowKind::Thinking`]  |
//! | `tool.generating/start/progress/complete` | [`RowKind::Tool`]    |
//! | `approval.request`                      | [`RowKind::Approval`]  |
//! | `clarify.request` (both shapes)         | [`RowKind::Clarify`]   |
//! | `status.update`                         | [`RowKind::Status`]    |
//! | `session.info`, `session.title`         | [`RowKind::Header`]    |
//! | `error`                                 | [`RowKind::Error`]     |
//!
//! No `serde_json::Value` outside the fields that must stay untyped (`args`,
//! `result`, `usage` cross as compact JSON strings per PLAN §3).

/// One renderable row of a session transcript, in event order.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// Position in this session's transcript (0-based, event order).
    pub index: usize,
    pub kind: RowKind,
}

/// The kind of a transcript row and its per-kind payload.
#[derive(Debug, Clone, PartialEq)]
pub enum RowKind {
    /// A locally submitted user message (UI inserts it; the fixture gateway
    /// never sends a user row itself).
    User { text: String },
    /// The streaming/received assistant message. `streaming` is true between
    /// `message.start` and `message.complete`; `complete` replaces the
    /// streamed text (PLAN §1.4).
    Assistant {
        text: String,
        streaming: bool,
        /// Compact JSON string of the `usage` object of `message.complete`
        /// (kept untyped per PLAN §3); empty when absent.
        usage_json: String,
        /// `warning` of `message.complete`, if any.
        warning: String,
    },
    /// Collapsible thinking row before the assistant row
    /// (`thinking.delta` / `reasoning.delta`).
    Thinking { text: String },
    /// Tool card, updated in place for the whole life of one `tool_id`.
    Tool(ToolCard),
    /// Approval card; `command` is already redacted server-side and `choices`
    /// are pre-computed — never recomputed in the app (PLAN §4 T4).
    Approval(ApprovalCard),
    /// Clarification card (single-question and batch shapes).
    Clarify(ClarifyCard),
    /// Status strip text (`status.update`).
    Status { kind: StatusKind, text: String },
    /// Session header (`session.info` / `session.title`).
    Header(SessionHeader),
    /// System error row (`error`).
    Error { message: String },
}

/// Verified `status.update.kind` values (PLAN §4 T4); unknown kinds degrade
/// to [`StatusKind::Other`] carrying the raw kind string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusKind {
    Status,
    Lifecycle,
    Compacting,
    Compacted,
    Heartbeat,
    Other(String),
}

impl StatusKind {
    pub(crate) fn parse(kind: &str) -> Self {
        match kind {
            "status" => StatusKind::Status,
            "lifecycle" => StatusKind::Lifecycle,
            "compacting" => StatusKind::Compacting,
            "compacted" => StatusKind::Compacted,
            "heartbeat" => StatusKind::Heartbeat,
            other => StatusKind::Other(other.to_string()),
        }
    }
}

/// Tool card state. Exactly one card exists per `tool_id` at a time and is
/// updated in place by `tool.generating` → `start` → `progress` → `complete`.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolCard {
    /// Wire tool id (`tool.start`/`tool.complete`); empty until `tool.start`.
    pub tool_id: String,
    pub name: String,
    /// Terminal state flag: true only after `tool.complete`.
    pub complete: bool,
    /// `context` of `tool.start`, when present.
    pub context: String,
    /// Compact JSON string of `args` (kept untyped per PLAN §3).
    pub args_json: String,
    /// Compact JSON string of `result` of `tool.complete` (untyped per §3).
    pub result_json: String,
    /// `duration_s` of `tool.complete`, if present (seconds, fractional).
    pub duration_s: f64,
}

/// Approval card. `command` arrives already redacted and `choices` are
/// pre-computed server-side (`["once","session","always","deny"]` subset
/// depending on `allow_session`/`allow_permanent`/`smart_denied`) — both are
/// taken as-is (PLAN §4 T4, verified wire shape).
#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalCard {
    pub request_id: String,
    /// Already redacted by the server; do NOT recompute.
    pub command: String,
    pub description: String,
    /// Pre-computed server-side; do NOT recompute.
    pub choices: Vec<String>,
    /// True once [`resolve_approval`](crate::transcript::reducer::Reducer::resolve_approval)
    /// resolved this request.
    pub resolved: bool,
}

/// One clarification question (both wire shapes funnel into this).
#[derive(Debug, Clone, PartialEq)]
pub struct ClarifyQuestion {
    /// Question id: the batch shape carries `qid`; the single-question shape
    /// has none — defaulted to `"q0"` (what the fixture gateway uses).
    pub qid: String,
    pub question: String,
    pub choices: Vec<String>,
    pub multi_select: bool,
}

/// Clarify card, covering both payload shapes (PLAN §1.4):
/// `{request_id, question, choices, multi_select?}` and
/// `{request_id, questions:[{qid, question, choices, multi_select}], answers?}`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClarifyCard {
    pub request_id: String,
    pub questions: Vec<ClarifyQuestion>,
    /// True once [`resolve_clarify`](crate::transcript::reducer::Reducer::resolve_clarify)
    /// cleared the pending question.
    pub resolved: bool,
}

/// Session header data (`session.info`, `session.title`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SessionHeader {
    pub title: String,
    pub model: String,
    pub cwd: String,
    pub branch: String,
    /// Compact JSON string of the `usage` object, when present (untyped).
    pub usage_json: String,
}

/// One markdown block produced by
/// [`split_blocks`](crate::transcript::markdown::split_blocks).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownBlock {
    /// Info string of the fence (`rust`, `json`, …), empty for plain text.
    pub language: String,
    /// The block text, without the fence lines.
    pub text: String,
    /// True while the fence that opened this block is not yet closed — the
    /// normal state of the last block during streaming. An open block is kept
    /// (never dropped) so the UI can append as deltas arrive.
    pub open: bool,
}

/// Full transcript state of ONE session — the per-session state machine
/// state held by [`Reducer`](crate::transcript::reducer::Reducer).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Transcript {
    /// Rows in event order. Session-less events never create rows.
    pub rows: Vec<Row>,
    /// Latest session header (`session.info`/`session.title` merge into it).
    pub header: SessionHeader,
}

impl Transcript {
    /// Convenience accessor: kind name of row `index`, `""` when absent.
    pub fn kind_at(&self, index: usize) -> &'static str {
        match self.rows.get(index).map(|r| &r.kind) {
            Some(RowKind::User { .. }) => "user",
            Some(RowKind::Assistant { .. }) => "assistant",
            Some(RowKind::Thinking { .. }) => "thinking",
            Some(RowKind::Tool(_)) => "tool",
            Some(RowKind::Approval(_)) => "approval",
            Some(RowKind::Clarify(_)) => "clarify",
            Some(RowKind::Status { .. }) => "status",
            Some(RowKind::Header(_)) => "header",
            Some(RowKind::Error { .. }) => "error",
            None => "",
        }
    }
}

