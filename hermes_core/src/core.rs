//! `HermesCore` — the use case / FFI surface (PLAN §4 T5, §3 threading).
//!
//! The single object the Android app talks to. Clean Architecture: this is
//! the only layer allowed to know adapters *and* entities; it orchestrates
//! the WebSocket client, the auth client, the transcript reducers and the
//! session registry, and owns all I/O policy (endpoint file, registry file).
//!
//! Threading (PLAN §3): `HermesCore` owns a 2-worker multi-thread tokio
//! runtime. Exported async methods delegate their body onto that runtime
//! (`self.runtime.spawn(...)`), so the foreign executor only drives a thin
//! wrapper. Events reach the app through the [`EventSink`] foreign callback
//! trait; every transcript change carries its session key. One reducer task
//! per open session filters the single broadcast on its live sid; reconnect
//! (backoff 1/2/4/8/15 s + ≤20% jitter, `crate::reconnect`) resumes every
//! open session and re-emits unresolved approval cards deduped by
//! `request_id`. `SessionExpired` surfaces as [`ConnectionStatus::NeedsPassword`]
//! and keeps every transcript.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde_json::{Value, json};

use crate::auth::client::AuthClient;
use crate::auth::endpoint::{GatewayEndpoint, parse_qr_payload, ws_url};
use crate::error::CoreError;
use crate::json;
use crate::protocol::EventParams;
use crate::reconnect;
use crate::rpc::api;
use crate::rpc::client::{ClientConfig, ConnectionState, GatewayClient, GatewayEvent};
use crate::session_registry::{SessionRecord, SessionRegistry};
use crate::transcript::markdown::split_blocks;
use crate::transcript::model::{Row, RowKind};
use crate::transcript::reducer::{Reducer, SessionChange, TranscriptChange};

/// Durable endpoint file: `<data_dir>/endpoint.json` (URL + username only —
/// never a secret, per PI_ROLE).
const ENDPOINT_FILE: &str = "endpoint.json";
/// Durable tab list: `<data_dir>/sessions.json` (PLAN §4 T5 item 3).
const SESSIONS_FILE: &str = "sessions.json";
/// The close reason of a deliberate local disconnect — the supervisor must
/// never reconnect after it (every other reason reconnects).
const LOCAL_CLOSE_REASON: &str = "user logout";
/// Terminal cols used when a session is created without a UI-provided value.
const DEFAULT_COLS: i64 = 80;

// ── FFI DTOs (plain data only, PLAN §3) ─────────────────────────────────────

/// A paired gateway endpoint (the parsed QR payload, persisted).
#[derive(Debug, Clone, uniffi::Record)]
pub struct EndpointDto {
    pub base_url: String,
    pub username: String,
    pub display_name: String,
}

/// Kind of a transcript change, mirroring the entity's
/// [`TranscriptChange`] for the FFI boundary.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum TranscriptChangeKind {
    RowAppended,
    RowUpdated,
    Reset,
    HeaderUpdated,
}

/// One routable transcript change, as delivered to [`EventSink::on_transcript`].
/// `index` is the affected row (`u32::MAX` for `Reset`/`HeaderUpdated`);
/// `row_json` carries the full row as compact JSON for append/update.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TranscriptChangeDto {
    pub key: String,
    pub kind: TranscriptChangeKind,
    pub index: u32,
    pub row_json: String,
}

/// Connection state as seen by the app (PLAN §4 T6 phases).
#[derive(Debug, Clone, uniffi::Enum)]
pub enum ConnectionStatus {
    Connecting,
    Open,
    Closed { reason: String },
    /// The gateway session expired: the user must re-enter the password.
    /// All transcripts are kept.
    NeedsPassword,
}

/// One open chat tab (`open_sessions()`), with the flags the tab strip needs.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SessionSummary {
    /// The durable session key (what `open_session`/`close_session` take).
    pub key: String,
    pub title: String,
    pub running: bool,
    pub header_json: String,
}

/// One entry of the gateway's `session.list` (the remote picker).
#[derive(Debug, Clone, uniffi::Record)]
pub struct RemoteSessionDto {
    pub id: String,
    pub title: String,
    pub preview: String,
    pub message_count: i64,
}

/// One markdown block (`split_markdown`).
#[derive(Debug, Clone, uniffi::Record)]
pub struct MarkdownBlockDto {
    pub language: String,
    pub text: String,
    pub open: bool,
}

/// One composer completion item (`complete_slash`).
#[derive(Debug, Clone, uniffi::Record)]
pub struct SlashCompletionDto {
    pub display: String,
    pub text: String,
    pub kind: String,
}

// ── EventSink (foreign callback trait, PLAN §3/§4 T5) ───────────────────────

/// The foreign callback the app registers once at `connect`. UniFFI invokes
/// it on a Rust thread; the app marshals to its own main thread. Every
/// payload is a plain DTO — no framework type crosses the FFI.
#[uniffi::export(with_foreign)]
pub trait EventSink: Send + Sync {
    /// A transcript change for the session `key` (routing is by key — the
    /// sink must never see another tab's change under this key).
    fn on_transcript(&self, change: TranscriptChangeDto);
    /// Connection state changed.
    fn on_connection(&self, status: ConnectionStatus);
}

// ── internal: live session entry ────────────────────────────────────────────

/// One open chat: the live sid it currently maps to, its reducer, and its
/// seq watermark (mirrored into the registry on resume). The durable key is
/// the map key itself (`sessions: HashMap<String, LiveSession>`).
struct LiveSession {
    live_sid: String,
    reducer: Reducer,
    /// Highest seq applied (the watermark `events.since` resumes from).
    last_seen_seq: i64,
}

/// Convert an entity [`TranscriptChange`] into its FFI DTO, stamping the
/// session key. Pure helper (testable without a socket).
pub(crate) fn change_to_dto(key: &str, change: &TranscriptChange, transcript_rows: &[Row]) -> TranscriptChangeDto {
    let (kind, index, row_json) = match change {
        TranscriptChange::RowAppended { index } => {
            let idx = *index;
            (TranscriptChangeKind::RowAppended, idx, row_json(transcript_rows.get(idx)))
        }
        TranscriptChange::RowUpdated { index } => {
            let idx = *index;
            (TranscriptChangeKind::RowUpdated, idx, row_json(transcript_rows.get(idx)))
        }
        TranscriptChange::Reset => (TranscriptChangeKind::Reset, usize::MAX, String::new()),
        TranscriptChange::HeaderUpdated => (TranscriptChangeKind::HeaderUpdated, usize::MAX, String::new()),
    };
    TranscriptChangeDto {
        key: key.to_string(),
        kind,
        index: u32::try_from(index).unwrap_or(u32::MAX),
        row_json,
    }
}

/// Compact JSON of one row, or `""` when the index is out of range (the
/// defensive rule: never panic on a routing race after a reset).
fn row_json(row: Option<&Row>) -> String {
    let Some(row) = row else { return String::new() };
    let payload = match &row.kind {
        RowKind::User { text } => json!({"kind": "user", "text": text}),
        RowKind::Assistant { text, streaming, usage_json, warning } => json!({
            "kind": "assistant", "text": text, "streaming": streaming,
            "usage": usage_json, "warning": warning,
        }),
        RowKind::Thinking { text } => json!({"kind": "thinking", "text": text}),
        RowKind::Tool(card) => json!({
            "kind": "tool", "tool_id": card.tool_id, "name": card.name,
            "complete": card.complete, "context": card.context,
            "args": card.args_json, "result": card.result_json,
            "duration_s": card.duration_s,
        }),
        RowKind::Approval(card) => json!({
            "kind": "approval", "request_id": card.request_id,
            "command": card.command, "description": card.description,
            "choices": card.choices, "resolved": card.resolved,
        }),
        RowKind::Clarify(card) => json!({
            "kind": "clarify", "request_id": card.request_id,
            "questions": card.questions.iter().map(|q| json!({
                "qid": q.qid, "question": q.question,
                "choices": q.choices, "multi_select": q.multi_select,
            })).collect::<Vec<_>>(),
            "resolved": card.resolved,
        }),
        RowKind::Status { kind, text } => json!({"kind": "status", "status": format!("{kind:?}"), "text": text}),
        RowKind::Error { message } => json!({"kind": "error", "message": message}),
    };
    json::to_compact_string(&payload)
}

/// Wall-clock milliseconds (registry timestamps, jitter seed).
pub(crate) fn now_wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Deterministic-in-tests jitter factor from the wall clock: maps the
/// sub-second part onto 0.0..1.0. The schedule stays 1/2/4/8/15 s + ≤20%
/// (tests pass a fixed factor to `reconnect::jittered_delay` directly).
pub(crate) fn clock_jitter() -> f64 {
    (now_wall_ms() % 1000) as f64 / 1000.0
}

// ── HermesCore object ───────────────────────────────────────────────────────

/// The single object the app talks to (PLAN §4 T5). Session-keyed: every
/// per-session verb takes the durable session key.
#[derive(uniffi::Object)]
pub struct HermesCore {
    /// The 2-worker multi-thread runtime (PLAN §3). Held in an `Option` only
    /// so `Drop` can move it to a detached thread: dropping a multi-thread
    /// runtime BLOCKS until its workers stop, and the app (or the probe) can
    /// drop the core from inside an async context, where blocking panics
    /// ("Cannot drop a runtime in a context where blocking is not allowed").
    runtime: Option<tokio::runtime::Runtime>,
    /// Handle used to spawn every body onto that runtime.
    handle: tokio::runtime::Handle,
    data_dir: PathBuf,
    inner: Mutex<CoreState>,
    /// Shared with the supervisor task so `disconnect()` can stop it.
    stop_flag: Arc<AtomicBool>,
    /// True while a supervisor task is alive (PLAN §3: ONE supervisor).
    /// `app_did_foreground` checks it so a foreground probe never races a
    /// second supervisor against a live reconnect ladder.
    supervising: Arc<AtomicBool>,
}

/// Mutable core state, guarded by a `parking_lot::Mutex` (sync methods only;
/// async bodies take the lock briefly to clone handles).
struct CoreState {
    endpoint: Option<GatewayEndpoint>,
    auth: Option<AuthClient>,
    client: Option<GatewayClient>,
    /// live sid -> session key (rebuilt at create/resume; PLAN §3).
    sid_index: HashMap<String, String>,
    /// Durable key -> live session (reducer + watermark).
    sessions: HashMap<String, LiveSession>,
    registry: SessionRegistry,
    /// Unresolved approval request_ids already on a transcript — the dedupe
    /// set for `approval.pending` re-emission after a (re)connect.
    emitted_approvals: HashMap<String, Vec<String>>,
    /// The app's sink, registered once by `connect`. Every transcript change
    /// and connection state goes out through it; the core never calls a
    /// foreign callback while holding the state lock.
    sink: Option<Arc<dyn EventSink>>,
}

impl HermesCore {
    /// Read `<data_dir>/sessions.json` through the entity's tolerant parser.
    /// A missing file is an empty registry (first launch); a corrupt file is
    /// an error that surfaces (same policy as the cookie jar).
    pub(crate) fn load_registry(data_dir: &std::path::Path) -> Result<SessionRegistry, CoreError> {
        let path = data_dir.join(SESSIONS_FILE);
        if !path.exists() {
            return Ok(SessionRegistry::new());
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| CoreError::Io(format!("read {}: {e}", path.display())))?;
        SessionRegistry::from_json(&raw)
    }

    /// Atomically persist the registry (tmp + rename, jar pattern).
    pub(crate) fn save_registry(data_dir: &std::path::Path, reg: &SessionRegistry) -> Result<(), CoreError> {
        let path = data_dir.join(SESSIONS_FILE);
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, reg.to_json())
            .map_err(|e| CoreError::Io(format!("write {}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, &path)
            .map_err(|e| CoreError::Io(format!("rename {}: {e}", path.display())))
    }

    /// Read the persisted endpoint (`<data_dir>/endpoint.json`), tolerantly.
    pub(crate) fn load_endpoint(data_dir: &std::path::Path) -> Option<GatewayEndpoint> {
        let raw = std::fs::read_to_string(data_dir.join(ENDPOINT_FILE)).ok()?;
        let v: Value = serde_json::from_str(&raw).ok()?;
        let base = url::Url::parse(json::str_at(&v, "base_url")).ok()?;
        Some(GatewayEndpoint::new(
            base,
            json::str_at(&v, "username").to_string(),
            json::str_at(&v, "display_name").to_string(),
        ))
    }

    /// Persist the endpoint (URL + username + display name only — never a
    /// secret).
    pub(crate) fn save_endpoint(data_dir: &std::path::Path, ep: &GatewayEndpoint) -> Result<(), CoreError> {
        let body = json!({
            "base_url": ep.base_url.as_str(),
            "username": ep.username,
            "display_name": ep.display_name,
        });
        std::fs::create_dir_all(data_dir)
            .map_err(|e| CoreError::Io(format!("create {}: {e}", data_dir.display())))?;
        std::fs::write(data_dir.join(ENDPOINT_FILE), json::to_compact_string(&body))
            .map_err(|e| CoreError::Io(format!("write endpoint: {e}")))
    }

    /// Fan out one decoded event to exactly the live sessions whose live sid
    /// matches (session-less events reach no reducer and create no rows).
    /// Pure over the state maps — the core of the key-routed fan-out.
    fn route_event(
        state: &mut CoreState,
        ev: &EventParams,
    ) -> Vec<(String, Vec<SessionChange>, String)> {
        if ev.session_id.is_empty() {
            return Vec::new();
        }
        let Some(key) = state.sid_index.get(&ev.session_id).cloned() else {
            return Vec::new();
        };
        let Some(live) = state.sessions.get_mut(&key) else {
            return Vec::new();
        };
        let changes = live.reducer.apply(ev);
        if let Some(seq) = ev.seq {
            if seq > live.last_seen_seq {
                live.last_seen_seq = seq;
            }
        }
        vec![(key, changes, live.reducer.transcript().header.title.clone())]
    }
}

impl Drop for HermesCore {
    /// Shut the runtime down OFF the current thread: `Runtime::drop` blocks
    /// until every worker stops, which panics when the core is dropped from
    /// inside an async context (an Android callback is exactly that).
    /// `shutdown_background` returns immediately and lets the workers finish
    /// in their own time.
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            let _ = std::thread::Builder::new()
                .name("hermo-runtime-shutdown".to_string())
                .spawn(move || runtime.shutdown_background());
        }
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl HermesCore {
    /// Build the core over a writable `data_dir` (Android `filesDir`). Loads
    /// the endpoint and the durable tab list from disk.
    #[uniffi::constructor]
    pub fn new(data_dir: String) -> Arc<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("core runtime");
        let handle = runtime.handle().clone();
        let dir = PathBuf::from(data_dir);
        let endpoint = Self::load_endpoint(&dir);
        let registry = Self::load_registry(&dir).unwrap_or_default();
        let auth = AuthClient::new(Some(&dir)).ok();
        Arc::new(Self {
            runtime: Some(runtime),
            handle,
            data_dir: dir,
            inner: Mutex::new(CoreState {
                endpoint,
                auth,
                client: None,
                sid_index: HashMap::new(),
                sessions: HashMap::new(),
                registry,
                emitted_approvals: HashMap::new(),
                sink: None,
            }),
            stop_flag: Arc::new(AtomicBool::new(false)),
            supervising: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Parse a `hermes://connect?...` QR payload without pairing yet.
    pub async fn parse_qr(self: Arc<Self>, payload: String) -> Result<EndpointDto, CoreError> {
        // Clone the handle out first: calling `self.runtime.spawn(async move { self… })`
        // borrows `self` for the whole call while the future moves it (E0505).
        let handle = self.handle.clone();
        handle
            .spawn(async move {
                let ep = parse_qr_payload(&payload)?;
                Ok(EndpointDto {
                    base_url: ep.base_url.to_string(),
                    username: ep.username,
                    display_name: ep.display_name,
                })
            })
            .await
            .map_err(|e| CoreError::Io(format!("join: {e}")))?
    }

    /// The paired endpoint, if any (from disk at construction).
    pub async fn saved_endpoint(self: Arc<Self>) -> Option<EndpointDto> {
        let handle = self.handle.clone();
        let this = Arc::clone(&self);
        handle
            .spawn(async move {
                this.inner.lock().endpoint.as_ref().map(|ep| EndpointDto {
                    base_url: ep.base_url.to_string(),
                    username: ep.username.clone(),
                    display_name: ep.display_name.clone(),
                })
            })
            .await
            .ok()
            .flatten()
    }

    /// Pair from a QR payload: parse, validate and persist (no login yet).
    pub async fn pair(self: Arc<Self>, payload: String) -> Result<EndpointDto, CoreError> {
        // Clone the handle out first: calling `self.runtime.spawn(async move { self… })`
        // borrows `self` for the whole call while the future moves it (E0505).
        let handle = self.handle.clone();
        handle
            .spawn(async move {
                let ep = parse_qr_payload(&payload)?;
                let _ = Self::save_endpoint(&self.data_dir, &ep);
                self.inner.lock().endpoint = Some(ep.clone());
                Ok(EndpointDto {
                    base_url: ep.base_url.to_string(),
                    username: ep.username,
                    display_name: ep.display_name,
                })
            })
            .await
            .map_err(|e| CoreError::Io(format!("join: {e}")))?
    }

    /// Password login against the paired endpoint (policy lives in the auth
    /// adapter: 401 → `InvalidCredentials`, 429 → `RateLimited`, …).
    pub async fn login(self: Arc<Self>, password: String) -> Result<(), CoreError> {
        // Clone the handle out first: calling `self.runtime.spawn(async move { self… })`
        // borrows `self` for the whole call while the future moves it (E0505).
        let handle = self.handle.clone();
        handle
            .spawn(async move {
                let (base, username) = {
                    let state = self.inner.lock();
                    let ep = state.endpoint.as_ref().ok_or(CoreError::NotConnected)?;
                    (ep.base_url.to_string(), ep.username.clone())
                };
                let auth = {
                    let mut state = self.inner.lock();
                    state
                        .auth
                        .get_or_insert_with(|| AuthClient::new(Some(&self.data_dir)).ok().unwrap())
                        .clone()
                };
                auth.login(&base, &username, &password).await
            })
            .await
            .map_err(|e| CoreError::Io(format!("join: {e}")))?
    }

    /// Forget the gateway: drop the endpoint file, the tab list and the
    /// cookies. Transcripts are irrelevant after this (logout wipes tabs).
    pub async fn forget_gateway(self: Arc<Self>) -> Result<(), CoreError> {
        // Clone the handle out first: calling `self.runtime.spawn(async move { self… })`
        // borrows `self` for the whole call while the future moves it (E0505).
        let handle = self.handle.clone();
        handle
            .spawn(async move {
                let _ = std::fs::remove_file(self.data_dir.join(ENDPOINT_FILE));
                let _ = std::fs::remove_file(self.data_dir.join(SESSIONS_FILE));
                let mut state = self.inner.lock();
                state.endpoint = None;
                state.registry = SessionRegistry::new();
                state.sessions.clear();
                state.sid_index.clear();
                state.emitted_approvals.clear();
                Ok(())
            })
            .await
            .map_err(|e| CoreError::Io(format!("join: {e}")))?
    }

    /// Open (or resume) a chat tab. `stored_id = None` mints a new session
    /// (`session.create`), `Some(id)` resumes a durable one
    /// (`session.resume`, ingesting its `messages` history). Returns the
    /// session key of the tab. Must be called on a connected core; the
    /// transcript is rebuilt here and pushed through the sink already
    /// registered by `connect`.
    pub async fn open_session(
        self: Arc<Self>,
        stored_id: Option<String>,
        cols: i64,
    ) -> Result<String, CoreError> {
        // Clone the handle out first: calling `self.runtime.spawn(async move { self… })`
        // borrows `self` for the whole call while the future moves it (E0505).
        let handle = self.handle.clone();
        handle
            .spawn(async move { self.open_session_inner(stored_id, cols).await })
            .await
            .map_err(|e| CoreError::Io(format!("join: {e}")))?
    }

    async fn open_session_inner(
        self: Arc<Self>,
        stored_id: Option<String>,
        cols: i64,
    ) -> Result<String, CoreError> {
        let client = {
            let state = self.inner.lock();
            state.client.clone().ok_or(CoreError::NotConnected)?
        };
        let cols = if cols <= 0 { DEFAULT_COLS } else { cols };
        let (key, live_sid, mut changes, title, messages) = match &stored_id {
            None => {
                let created = api::create_session(&client, cols).await?;
                (
                    created.stored_session_id.clone(),
                    created.session_id.clone(),
                    Vec::new(),
                    String::new(),
                    Vec::new(),
                )
            }
            Some(id) => {
                let resumed = api::resume_session(&client, id).await?;
                (id.clone(), resumed.session_id.clone(), Vec::new(), String::new(), resumed.messages)
            }
        };
        let mut state = self.inner.lock();
        let mut live = LiveSession {
            live_sid: live_sid.clone(),
            reducer: Reducer::new(),
            last_seen_seq: 0,
        };
        if !messages.is_empty() {
            changes.extend(live.reducer.ingest_resume_messages(&key, &messages));
        }
        state.sessions.insert(key.clone(), live);
        state.sid_index.insert(live_sid, key.clone());
        state.registry.upsert(SessionRecord {
            stored_id: key.clone(),
            last_seen_seq: 0,
            replay_epoch: String::new(),
            cols,
            title: title.clone(),
        });
        state.registry.set_active(Some(&key));
        let _ = Self::save_registry(&self.data_dir, &state.registry);
        let rows = state
            .sessions
            .get(&key)
            .map(|l| l.reducer.transcript().rows.clone())
            .unwrap_or_default();
        let sink = state.sink.clone();
        drop(state);
        // Never call a foreign callback while holding the state lock.
        if let Some(sink) = sink {
            for change in &changes {
                // Durable key (see deliver_event): the entity stamps the live sid.
                sink.on_transcript(change_to_dto(&key, &change.change, &rows));
            }
        }
        Ok(key)
    }

    /// Close a tab: remove it from the core state and the registry. The
    /// server keeps the durable session (it can be re-opened later).
    pub async fn close_session(self: Arc<Self>, key: String) -> Result<(), CoreError> {
        // Clone the handle out first: calling `self.runtime.spawn(async move { self… })`
        // borrows `self` for the whole call while the future moves it (E0505).
        let handle = self.handle.clone();
        handle
            .spawn(async move {
                let mut state = self.inner.lock();
                if let Some(live) = state.sessions.remove(&key) {
                    state.sid_index.remove(&live.live_sid);
                }
                state.emitted_approvals.remove(&key);
                state.registry.close(&key);
                let _ = Self::save_registry(&self.data_dir, &state.registry);
                Ok(())
            })
            .await
            .map_err(|e| CoreError::Io(format!("join: {e}")))?
    }

    /// The open tabs with their header + running flags (the tab strip).
    pub async fn open_sessions(self: Arc<Self>) -> Vec<SessionSummary> {
        let handle = self.handle.clone();
        let this = Arc::clone(&self);
        handle
            .spawn(async move {
                let state = this.inner.lock();
                state
                    .registry
                    .sessions()
                    .iter()
                    .map(|rec| {
                        let running = state
                            .sessions
                            .get(&rec.stored_id)
                            .map(|l| l.reducer.is_streaming())
                            .unwrap_or(false);
                        SessionSummary {
                            key: rec.stored_id.clone(),
                            title: rec.title.clone(),
                            running,
                            header_json: state
                                .sessions
                                .get(&rec.stored_id)
                                .map(|l| {
                                    json::to_compact_string(&json!({
                                        "title": l.reducer.transcript().header.title,
                                        "model": l.reducer.transcript().header.model,
                                    }))
                                })
                                .unwrap_or_default(),
                        }
                    })
                    .collect()
            })
            .await
            .unwrap_or_default()
    }

    /// Submit a prompt to the session `key`. The user row is NOT created
    /// here: the wire replay (`message.*` events) drives the transcript, so
    /// the same code path serves send, resume and reconnect.
    pub async fn send(self: Arc<Self>, key: String, text: String) -> Result<(), CoreError> {
        // Clone the handle out first: calling `self.runtime.spawn(async move { self… })`
        // borrows `self` for the whole call while the future moves it (E0505).
        let handle = self.handle.clone();
        handle
            .spawn(async move {
                let client = {
                    let state = self.inner.lock();
                    state.client.clone().ok_or(CoreError::NotConnected)?
                };
                let live_sid = {
                    let state = self.inner.lock();
                    state
                        .sessions
                        .get(&key)
                        .map(|l| l.live_sid.clone())
                        .ok_or(CoreError::NotConnected)?
                };
                api::submit(&client, &live_sid, &text)
                    .await
                    .map(|_| ())
                    .map_err(CoreError::from)
            })
            .await
            .map_err(|e| CoreError::Io(format!("join: {e}")))?
    }

    /// Interrupt the running turn of session `key`.
    pub async fn interrupt(self: Arc<Self>, key: String) -> Result<(), CoreError> {
        // Clone the handle out first: calling `self.runtime.spawn(async move { self… })`
        // borrows `self` for the whole call while the future moves it (E0505).
        let handle = self.handle.clone();
        handle
            .spawn(async move {
                let client = {
                    let state = self.inner.lock();
                    state.client.clone().ok_or(CoreError::NotConnected)?
                };
                let live_sid = {
                    let state = self.inner.lock();
                    state
                        .sessions
                        .get(&key)
                        .map(|l| l.live_sid.clone())
                        .ok_or(CoreError::NotConnected)?
                };
                api::interrupt(&client, &live_sid)
                    .await
                    .map(|_| ())
                    .map_err(CoreError::from)
            })
            .await
            .map_err(|e| CoreError::Io(format!("join: {e}")))?
    }

    /// Answer an approval card. Resolves the local card only after the RPC
    /// succeeded (4009 = answered elsewhere still resolves the UI state).
    pub async fn respond_approval(
        self: Arc<Self>,
        key: String,
        request_id: String,
        choice: String,
    ) -> Result<(), CoreError> {
        // Clone the handle out first: calling `self.runtime.spawn(async move { self… })`
        // borrows `self` for the whole call while the future moves it (E0505).
        let handle = self.handle.clone();
        handle
            .spawn(async move {
                let client = {
                    let state = self.inner.lock();
                    state.client.clone().ok_or(CoreError::NotConnected)?
                };
                let live_sid = {
                    let state = self.inner.lock();
                    state
                        .sessions
                        .get(&key)
                        .map(|l| l.live_sid.clone())
                        .ok_or(CoreError::NotConnected)?
                };
                api::respond_approval(&client, &live_sid, &request_id, &choice).await?;
                let (changes, rows, sink) = {
                    let mut state = self.inner.lock();
                    let changes = state
                        .sessions
                        .get_mut(&key)
                        .map(|l| l.reducer.resolve_approval(&key, &request_id))
                        .unwrap_or_default();
                    let rows = state
                        .sessions
                        .get(&key)
                        .map(|l| l.reducer.transcript().rows.clone())
                        .unwrap_or_default();
                    (changes, rows, state.sink.clone())
                };
                if let Some(sink) = sink {
                    for change in &changes {
                        // Durable key, not `change.key`: the entity stamps the
                        // live sid, the UI routes on the tab key (T5 fan-out).
                        sink.on_transcript(change_to_dto(&key, &change.change, &rows));
                    }
                }
                Ok(())
            })
            .await
            .map_err(|e| CoreError::Io(format!("join: {e}")))?
    }

    /// Answer a clarification card (single or batch: `answer`/`question_id`
    /// apply to the single shape; the batch shape answers one question per
    /// call, `remaining` is surfaced by the caller via the ack DTO).
    pub async fn respond_clarify(
        self: Arc<Self>,
        key: String,
        request_id: String,
        answer: String,
        question_id: Option<String>,
    ) -> Result<(), CoreError> {
        // Clone the handle out first: calling `self.runtime.spawn(async move { self… })`
        // borrows `self` for the whole call while the future moves it (E0505).
        let handle = self.handle.clone();
        handle
            .spawn(async move {
                let client = {
                    let state = self.inner.lock();
                    state.client.clone().ok_or(CoreError::NotConnected)?
                };
                let live_sid = {
                    let state = self.inner.lock();
                    state
                        .sessions
                        .get(&key)
                        .map(|l| l.live_sid.clone())
                        .ok_or(CoreError::NotConnected)?
                };
                api::respond_clarify(
                    &client,
                    &live_sid,
                    &request_id,
                    &answer,
                    question_id.as_deref(),
                )
                .await?;
                let mut state = self.inner.lock();
                state
                    .sessions
                    .get_mut(&key)
                    .map(|l| l.reducer.resolve_clarify(&key, &request_id))
                    .unwrap_or_default();
                Ok(())
            })
            .await
            .map_err(|e| CoreError::Io(format!("join: {e}")))?
    }

    /// Run a slash command in session `key` (e.g. `/model`).
    pub async fn run_slash(self: Arc<Self>, key: String, command: String) -> Result<String, CoreError> {
        // Clone the handle out first: calling `self.runtime.spawn(async move { self… })`
        // borrows `self` for the whole call while the future moves it (E0505).
        let handle = self.handle.clone();
        handle
            .spawn(async move {
                let client = {
                    let state = self.inner.lock();
                    state.client.clone().ok_or(CoreError::NotConnected)?
                };
                let live_sid = {
                    let state = self.inner.lock();
                    state
                        .sessions
                        .get(&key)
                        .map(|l| l.live_sid.clone())
                        .ok_or(CoreError::NotConnected)?
                };
                let value = api::slash_exec(&client, &live_sid, &command).await?;
                Ok(json::str_at(&value, "output").to_string())
            })
            .await
            .map_err(|e| CoreError::Io(format!("join: {e}")))?
    }

    /// Composer completion (gateway-global, no session key).
    pub async fn complete_slash(self: Arc<Self>, text: String) -> Result<Vec<SlashCompletionDto>, CoreError> {
        // Clone the handle out first: calling `self.runtime.spawn(async move { self… })`
        // borrows `self` for the whole call while the future moves it (E0505).
        let handle = self.handle.clone();
        handle
            .spawn(async move {
                let client = {
                    let state = self.inner.lock();
                    state.client.clone().ok_or(CoreError::NotConnected)?
                };
                let completions = api::complete_slash(&client, &text).await?;
                Ok(completions
                    .items
                    .into_iter()
                    .map(|i| SlashCompletionDto {
                        display: i.display,
                        text: i.text,
                        kind: i.kind,
                    })
                    .collect())
            })
            .await
            .map_err(|e| CoreError::Io(format!("join: {e}")))?
    }

    /// Split markdown text into blocks (entity rule, exposed over FFI).
    pub async fn split_markdown(self: Arc<Self>, text: String) -> Vec<MarkdownBlockDto> {
        // Clone the handle out first: calling `self.runtime.spawn(async move { self… })`
        // borrows `self` for the whole call while the future moves it (E0505).
        let handle = self.handle.clone();
        handle
            .spawn(async move {
                split_blocks(&text)
                    .into_iter()
                    .map(|b| MarkdownBlockDto {
                        language: b.language,
                        text: b.text,
                        open: b.open,
                    })
                    .collect()
            })
            .await
            .unwrap_or_default()
    }

    /// The gateway's own `session.list` (the remote session picker).
    pub async fn list_remote_sessions(self: Arc<Self>) -> Result<Vec<RemoteSessionDto>, CoreError> {
        // Clone the handle out first: calling `self.runtime.spawn(async move { self… })`
        // borrows `self` for the whole call while the future moves it (E0505).
        let handle = self.handle.clone();
        handle
            .spawn(async move {
                let client = {
                    let state = self.inner.lock();
                    state.client.clone().ok_or(CoreError::NotConnected)?
                };
                let list = api::list_sessions(&client, 200).await?;
                Ok(list
                    .sessions
                    .into_iter()
                    .map(|s| RemoteSessionDto {
                        id: s.id,
                        title: s.title,
                        preview: s.preview,
                        message_count: s.message_count,
                    })
                    .collect())
            })
            .await
            .map_err(|e| CoreError::Io(format!("join: {e}")))?
    }

    /// Deliberate disconnect (PI_TASK_T5A §3): set `stop_flag` so the
    /// supervisor never reconnects, close the client with
    /// [`LOCAL_CLOSE_REASON`] (the supervisor treats that reason as
    /// terminal), clear the client handle and emit a `Closed` status.
    /// Transcripts and the registry survive (the app keeps rendering tabs
    /// offline).
    pub async fn disconnect(self: Arc<Self>) -> Result<(), CoreError> {
        let handle = self.handle.clone();
        handle
            .spawn(async move {
                self.stop_flag.store(true, Ordering::Relaxed);
                let client = self.inner.lock().client.take();
                if let Some(client) = client {
                    client.close(LOCAL_CLOSE_REASON);
                }
                // No `Closed` emission here: the SUPERVISOR observes the
                // client's state watch and is the single source of connection
                // status. Emitting here too produced two `Closed` events per
                // user action (caught by the live gate).
                Ok(())
            })
            .await
            .map_err(|e| CoreError::Io(format!("join: {e}")))?
    }

    /// The app came back to the foreground: probe the connection with the
    /// short-timeout liveness ping and, when the supervisor is gone, swap
    /// in a fresh client and restart it. A live supervisor handles
    /// everything itself; this must never spawn a second one (PLAN §3).
    pub async fn app_did_foreground(self: Arc<Self>) -> Result<(), CoreError> {
        let handle = self.handle.clone();
        handle
            .spawn(async move {
                // A live supervisor owns recovery: just probe (the dead
                // peer will trip its own deadline / reconnect ladder).
                if self.supervising.load(Ordering::Relaxed) {
                    let client = self.inner.lock().client.clone();
                    if let Some(client) = client {
                        let _ = client.probe_now().await;
                    }
                    return Ok(());
                }
                if self.supervising.swap(true, Ordering::Relaxed) {
                    return Ok(()); // a foreground race lost to another restart
                }
                let sink = self.inner.lock().sink.clone();
                let Some(sink) = sink else {
                    self.supervising.store(false, Ordering::Relaxed);
                    return Ok(());
                };
                let client = match self.establish().await {
                    Ok(c) => c,
                    Err(CoreError::SessionExpired) => {
                        self.supervising.store(false, Ordering::Relaxed);
                        sink.on_connection(ConnectionStatus::NeedsPassword);
                        return Ok(());
                    }
                    Err(e) => {
                        self.supervising.store(false, Ordering::Relaxed);
                        return Err(e);
                    }
                };
                self.inner.lock().client = Some(client.clone());
                drop(self.handle.spawn(Self::supervise(self.clone(), client, sink)));
                Ok(())
            })
            .await
            .map_err(|e| CoreError::Io(format!("join: {e}")))?
    }

    /// Connect to the paired gateway (PLAN §4 T5 item 1 / PI_TASK_T5A §1).
    ///
    /// Stores the sink, ensures auth from the persisted cookie jar (on
    /// `SessionExpired` or missing credentials the error returns so the UI
    /// can ask for the password via `login`), mints the single-use ws
    /// ticket, connects the [`GatewayClient`] and spawns ONE supervisor
    /// task that owns the single broadcast subscription and does the
    /// key-routed fan-out, the reconnect ladder and the per-session resume.
    /// The state lock is never held across an `.await`.
    pub async fn connect(self: Arc<Self>, sink: Arc<dyn EventSink>) -> Result<(), CoreError> {
        // Clone the handle out first: `self.runtime.spawn(async move { self… })`
        // borrows `self` for the whole call while the future moves it (E0505).
        let handle = self.handle.clone();
        handle
            .spawn(async move { self.connect_inner(sink).await })
            .await
            .map_err(|e| CoreError::Io(format!("join: {e}")))?
    }
}

// ── connection internals (NOT exported: plain async impl) ──────────────────
// Private helpers stay outside the `#[uniffi::export]` block: UniFFI would
// try to codegen an FFI signature for them, and `GatewayClient` is not an
// FFI type.

impl HermesCore {
    async fn connect_inner(self: Arc<Self>, sink: Arc<dyn EventSink>) -> Result<(), CoreError> {
        // The sink is the single one the app registers; the supervisor and
        // every exported method deliver through it.
        self.inner.lock().sink = Some(Arc::clone(&sink));
        let client = self.establish().await?;
        self.inner.lock().client = Some(client.clone());
        self.stop_flag.store(false, Ordering::Relaxed);
        self.supervising.store(true, Ordering::Relaxed);
        drop(self.handle.spawn(Self::supervise(self.clone(), client, sink)));
        Ok(())
    }

    /// One authenticated gateway connection: ensure the cookie jar (no
    /// login call here — `login` is a separate exported verb), mint the
    /// ws-ticket, connect and wait for `gateway.ready`.
    async fn establish(&self) -> Result<GatewayClient, CoreError> {
        let (base_url_str, ws_base, auth) = {
            let state = self.inner.lock();
            let ep = state.endpoint.as_ref().ok_or(CoreError::NotConnected)?;
            let auth = state.auth.clone().ok_or(CoreError::SessionExpired)?;
            (
                ep.base_url.to_string(),
                ep.base_url.clone(),
                auth,
            )
        };
        // Lock dropped: the HTTP round trip below must never run under it.
        let ticket = auth.mint_ticket(&base_url_str).await?;
        let url = ws_url(&ws_base, &ticket.ticket);
        let client = GatewayClient::connect(url.as_str(), ClientConfig::default()).await?;
        Ok(client)
    }

    /// The supervisor (PLAN §3: ONE task owning the single broadcast
    /// subscription; PI_TASK_T5A §2). Per connection it forwards every
    /// `GatewayEvent::Event` through `route_event` and delivers the
    /// returned changes to the sink **with the session key** (rows cloned
    /// out of the lock before the foreign call); maps `ConnectionState` →
    /// `ConnectionStatus` on the sink; resumes EVERY open session after a
    /// (re)connect (`session.resume` + rebuild on truncated/epoch change,
    /// else `events.since` per watermark) and re-emits unresolved
    /// `approval.pending` cards deduped by `request_id`; and reconnects
    /// with the existing backoff ladder until `stop_flag` or a
    /// `LOCAL_CLOSE_REASON` close. The state lock is never held across an
    /// `.await`.
    async fn supervise(
        self: Arc<Self>,
        client: GatewayClient,
        sink: Arc<dyn EventSink>,
    ) {
        let mut attempt: u32 = 0;
        let mut current = client;
        loop {
            // ── subscription + state watch for THIS connection ──────────
            let mut events = current.events();
            let mut state = current.state();
            sink.on_connection(ConnectionStatus::Connecting);

            // ── resume phase: every open session, every (re)connect ─────
            let fresh_epoch = current.replay_epoch();
            self.resume_all(&current, &fresh_epoch, &sink).await;

            // ── event loop ───────────────────────────────────────────────
            loop {
                tokio::select! {
                    ev = events.recv() => match ev {
                        Ok(GatewayEvent::Event(params)) => {
                            self.deliver_event(&params, &sink);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            // Broadcast overflow: fill the gap from the
                            // per-session watermarks.
                            log::warn!("supervisor lagged, dropped {n} events");
                            self.catch_up_all(&current, &fresh_epoch, &sink).await;
                        }
                        Err(_) => break, // sender dropped: connection over
                    },
                    changed = state.changed() => {
                        if changed.is_err() {
                            break;
                        }
                        match state.borrow().clone() {
                            ConnectionState::Open => {
                                attempt = 0; // successful connect resets the ladder
                                sink.on_connection(ConnectionStatus::Open);
                            }
                            ConnectionState::Closed(reason) => {
                                sink.on_connection(ConnectionStatus::Closed { reason: reason.clone() });
                                if reason == LOCAL_CLOSE_REASON {
                                    // Deliberate local disconnect: never
                                    // reconnect after it (PI_TASK_T5A §3).
                                    return;
                                }
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }

            // ── reconnect ladder ─────────────────────────────────────────
            if self.stop_flag.load(Ordering::Relaxed) {
                return;
            }
            loop {
                let delay = reconnect::jittered_delay(attempt, clock_jitter());
                attempt += 1;
                sink.on_connection(ConnectionStatus::Connecting);
                // `Ok` = stop_flag set while waiting (the wait future
                // resolved before the timeout), so the supervisor exits.
                if tokio::time::timeout(
                    std::time::Duration::from_millis(delay),
                    self.sleep_until_stopped(),
                )
                .await
                .is_ok()
                {
                    return;
                }
                match self.establish().await {
                    Ok(new_client) => {
                        // A successful connect (gateway.ready received)
                        // resets the ladder (PI_TASK_T5A §2).
                        attempt = 0;
                        self.inner.lock().client = Some(new_client.clone());
                        current = new_client;
                        break;
                    }
                    Err(CoreError::SessionExpired) => {
                        // Auth is gone: the user must re-enter the password.
                        // Transcripts survive (kept in `sessions`).
                        sink.on_connection(ConnectionStatus::NeedsPassword);
                        return;
                    }
                    Err(_) => continue, // stay on the ladder
                }
            }
        }
    }

    /// Route one wire event through the fan-out and deliver every change to
    /// the sink with its session key. Rows are cloned out of the state lock
    /// BEFORE any foreign callback (PI_TASK_T5A §2: never call the sink
    /// under the lock).
    fn deliver_event(&self, params: &EventParams, sink: &Arc<dyn EventSink>) {
        let (deliveries, registry_updates) = {
            let mut state = self.inner.lock();
            let routed = Self::route_event(&mut state, params);
            let mut deliveries = Vec::with_capacity(routed.len());
            for (key, changes, _title) in &routed {
                let rows = state
                    .sessions
                    .get(key)
                    .map(|l| l.reducer.transcript().rows.clone())
                    .unwrap_or_default();
                deliveries.push(
                    changes
                        // Durable key, not `c.key` (the entity stamps the live
                        // sid; the UI routes on the tab key).
                        .iter()
                        .map(|c| change_to_dto(key, &c.change, &rows))
                        .collect::<Vec<_>>(),
                );
                if let Some(seq) = params.seq {
                    state.registry.touch(key, seq, "");
                }
            }
            (deliveries, !routed.is_empty())
        };
        if registry_updates {
            let state = self.inner.lock();
            let _ = Self::save_registry(&self.data_dir, &state.registry);
        }
        for batch in deliveries {
            for change in batch {
                sink.on_transcript(change);
            }
        }
    }

    /// Resolve when `stop_flag` is set (the ladder's interruptible sleep).
    async fn sleep_until_stopped(&self) {
        while !self.stop_flag.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    }

    /// Resume EVERY open session on a (re)connect (PI_TASK_T5A §2):
    /// `session.resume` per durable id (it also returns the NEW live sid —
    /// the sid_index is re-pointed here), then `events.since` per watermark:
    /// a `truncated` answer or a changed replay epoch rebuilds that one
    /// session via `ingest_resume_messages`, otherwise the replay fills the
    /// gap through the same fan-out.
    async fn resume_all(&self, client: &GatewayClient, fresh_epoch: &str, sink: &Arc<dyn EventSink>) {
        // Snapshot the durable keys; the lock is dropped before the RPCs.
        let keys: Vec<String> = {
            let state = self.inner.lock();
            state.registry.sessions().iter().map(|r| r.stored_id.clone()).collect()
        };
        for key in keys {
            let record = {
                let state = self.inner.lock();
                state.registry.get(&key).cloned()
            };
            let Some(record) = record else { continue };
            // `session.resume` reattaches the durable session and returns
            // the live sid it now answers to (it may differ after a
            // reconnect — the sid_index MUST follow it).
            let resumed = match api::resume_session(client, &key).await {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("resume of session {key} failed: {e}");
                    continue;
                }
            };
            self.repoint_live_sid(&key, &resumed.session_id);
            // Truncation is answered by `events.since`; a changed epoch is
            // detected against the epoch the watermark was taken under.
            let replay = match api::events_since(client, &key, record.last_seen_seq).await {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("events.since for {key} failed: {e}");
                    continue;
                }
            };
            let stored_epoch_changed =
                !record.replay_epoch.is_empty() && record.replay_epoch != fresh_epoch;
            let replay_epoch_changed =
                !replay.epoch.is_empty() && !record.replay_epoch.is_empty()
                    && replay.epoch != record.replay_epoch;
            if replay.truncated || stored_epoch_changed || replay_epoch_changed {
                // Rebuild ONLY this session from the resume messages.
                let changes = {
                    let mut state = self.inner.lock();
                    let Some(live) = state.sessions.get_mut(&key) else {
                        continue;
                    };
                    let changes = live.reducer.ingest_resume_messages(&key, &resumed.messages);
                    live.last_seen_seq = 0;
                    let rows = live.reducer.transcript().rows.clone();
                    drop(state);
                    // Registry write in its own scope (the live borrow is gone).
                    {
                        let mut state = self.inner.lock();
                        if let Some(rec) = state.registry.get_mut(&key) {
                            rec.last_seen_seq = 0;
                            if !replay.epoch.is_empty() {
                                rec.replay_epoch = replay.epoch.clone();
                            }
                        }
                    }
                    changes
                        .into_iter()
                        // Durable key, not `c.key` (see deliver_event).
                        .map(|c| change_to_dto(&key, &c.change, &rows))
                        .collect::<Vec<_>>()
                };
                for change in changes {
                    sink.on_transcript(change);
                }
            } else {
                // Ordinary gap fill: replay each event through the fan-out.
                for ev in &replay.events {
                    let params = EventParams {
                        event_type: ev.event_type.clone(),
                        session_id: ev.session_id.clone(),
                        seq: Some(ev.seq),
                        payload: ev.payload.clone(),
                    };
                    self.deliver_event(&params, sink);
                }
                {
                    let mut state = self.inner.lock();
                    state.registry.touch(&key, replay.latest_seq, &replay.epoch);
                }
            }
        }
        self.reemit_approvals(client, sink).await;
    }

    /// Re-point a session's live sid after a resume (the sid_index is what
    /// `route_event` matches incoming events against).
    fn repoint_live_sid(&self, key: &str, new_live_sid: &str) {
        let mut state = self.inner.lock();
        let old_sid = match state.sessions.get(key) {
            Some(live) if live.live_sid != new_live_sid => live.live_sid.clone(),
            _ => return,
        };
        state.sid_index.remove(&old_sid);
        if let Some(live) = state.sessions.get_mut(key) {
            live.live_sid = new_live_sid.to_string();
        }
        state
            .sid_index
            .insert(new_live_sid.to_string(), key.to_string());
    }

    /// Gap fill after a broadcast lag: the transcript state is intact, only
    /// the missed frames must be replayed per watermark.
    async fn catch_up_all(&self, client: &GatewayClient, fresh_epoch: &str, sink: &Arc<dyn EventSink>) {
        let snapshots: Vec<(String, i64)> = {
            let state = self.inner.lock();
            state
                .registry
                .sessions()
                .iter()
                .map(|r| (r.stored_id.clone(), r.last_seen_seq))
                .collect()
        };
        for (key, last_seen) in snapshots {
            let replay = match api::events_since(client, &key, last_seen).await {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("events.since for {key} failed: {e}");
                    continue;
                }
            };
            for ev in &replay.events {
                let params = EventParams {
                    event_type: ev.event_type.clone(),
                    session_id: ev.session_id.clone(),
                    seq: Some(ev.seq),
                    payload: ev.payload.clone(),
                };
                self.deliver_event(&params, sink);
            }
        }
        let _ = fresh_epoch;
    }

    /// Re-emit unresolved `approval.pending` cards after a (re)connect,
    /// deduped by `request_id` against `emitted_approvals` and the
    /// transcript itself (`Reducer::has_unresolved_approval`).
    async fn reemit_approvals(&self, client: &GatewayClient, sink: &Arc<dyn EventSink>) {
        let keys: Vec<String> = {
            let state = self.inner.lock();
            state.sessions.keys().cloned().collect()
        };
        for key in keys {
            let live_sid = {
                let state = self.inner.lock();
                match state.sessions.get(&key) {
                    Some(l) => l.live_sid.clone(),
                    None => continue,
                }
            };
            let pending = match api::pending_approvals(client, &live_sid).await {
                Ok(v) => v,
                Err(e) => {
                    log::warn!("approval.pending for {key} failed: {e}");
                    continue;
                }
            };
            let list = pending
                .get("pending")
                .or_else(|| pending.get("approvals"))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            for card in &list {
                let request_id = crate::json::str_at(card, "request_id").to_string();
                if request_id.is_empty() {
                    continue;
                }
                // Dedupe by request_id: a card already emitted for this
                // session (or already on the transcript unresolved) is
                // never re-emitted (PI_TASK_T5A §2).
                let (already, changes, rows) = {
                    let mut state = self.inner.lock();
                    let already = state
                        .emitted_approvals
                        .get(&key)
                        .map(|v| v.iter().any(|id| id == &request_id))
                        .unwrap_or(false);
                    let mut changes: Vec<SessionChange> = Vec::new();
                    let mut rows: Vec<Row> = Vec::new();
                    if !already {
                        if let Some(live) = state.sessions.get_mut(&key) {
                            if !live.reducer.has_unresolved_approval(&request_id) {
                                if let Some(emitted) =
                                    live.reducer.apply_approval_card(&key, &request_id, card)
                                {
                                    changes = emitted;
                                }
                            }
                            rows = live.reducer.transcript().rows.clone();
                        }
                    }
                    (already, changes, rows)
                };
                if already || changes.is_empty() {
                    continue;
                }
                {
                    let mut state = self.inner.lock();
                    state
                        .emitted_approvals
                        .entry(key.clone())
                        .or_default()
                        .push(request_id);
                }
                for change in &changes {
                    sink.on_transcript(change_to_dto(&key, &change.change, &rows));
                }
            }
        }
    }
}

