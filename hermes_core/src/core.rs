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
use crate::rpc::api::{self, SubmitStatus};
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
/// How often the supervisor flushes a dirty session registry to disk. The
/// watermark moves with every streamed event, so writing the file inline
/// rewrote it hundreds of times per turn; the writes are coalesced and the
/// worst case after a crash is re-replaying a few seconds of events, which
/// `events.since` already handles.
const REGISTRY_FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

/// Flatten the `JoinHandle` of a body delegated onto the core's runtime.
///
/// Every exported async method ends in this. The handle is always cloned out
/// of `self` first at the call site: `self.handle.spawn(async move { self… })`
/// borrows `self` for the whole call while the future moves it (E0505).
async fn joined<T>(task: tokio::task::JoinHandle<Result<T, CoreError>>) -> Result<T, CoreError> {
    task.await
        .map_err(|e| CoreError::Io(format!("join: {e}")))?
}

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
    /// Snapshot of [`CoreState::history_epoch`] taken at THIS session's last
    /// transcript rebuild (every `LiveSession` insert and every
    /// `rebuild_session`). `send` captures it before awaiting the submit RPC
    /// and re-checks it under the lock, so a rebuild that re-ingested the
    /// session in between skips the user-row append (the server-rebuilt
    /// history already contains the message) — the submit/resume race,
    /// PR #10 finding 2.
    history_epoch: u64,
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
    /// Wakes the reconnect ladder out of its backoff sleep the moment
    /// `stop_flag` is raised, so the ladder never polls the flag.
    stop_notify: Arc<tokio::sync::Notify>,
    /// Set when an event moved a watermark; cleared by `flush_registry`.
    /// See [`REGISTRY_FLUSH_INTERVAL`].
    registry_dirty: Arc<AtomicBool>,
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
    /// Monotonic counter bumped on EVERY transcript rebuild — a new
    /// `LiveSession` (`open_session`) or a `rebuild_session` from
    /// `resume_all`. Live sessions snapshot it; see
    /// [`LiveSession::history_epoch`]. Global (never reset per session) so a
    /// session closed and reopened under the same durable key can never
    /// alias an in-flight `send` snapshot.
    history_epoch: u64,
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
                history_epoch: 0,
            }),
            stop_flag: Arc::new(AtomicBool::new(false)),
            stop_notify: Arc::new(tokio::sync::Notify::new()),
            registry_dirty: Arc::new(AtomicBool::new(false)),
            supervising: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Parse a `hermes://connect?...` QR payload without pairing yet.
    pub async fn parse_qr(self: Arc<Self>, payload: String) -> Result<EndpointDto, CoreError> {
        let handle = self.handle.clone();
        joined(handle.spawn(async move {
                let ep = parse_qr_payload(&payload)?;
                Ok(EndpointDto {
                    base_url: ep.base_url.to_string(),
                    username: ep.username,
                    display_name: ep.display_name,
                })
            })
        )
        .await
    }

    /// The paired endpoint, if any (from disk at construction).
    pub async fn saved_endpoint(self: Arc<Self>) -> Option<EndpointDto> {
        let handle = self.handle.clone();
        let this = Arc::clone(&self);
        handle.spawn(async move {
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
        let handle = self.handle.clone();
        joined(handle.spawn(async move {
                let ep = parse_qr_payload(&payload)?;
                let _ = Self::save_endpoint(&self.data_dir, &ep);
                self.inner.lock().endpoint = Some(ep.clone());
                Ok(EndpointDto {
                    base_url: ep.base_url.to_string(),
                    username: ep.username,
                    display_name: ep.display_name,
                })
            })
        )
        .await
    }

    /// Password login against the paired endpoint (policy lives in the auth
    /// adapter: 401 → `InvalidCredentials`, 429 → `RateLimited`, …).
    pub async fn login(self: Arc<Self>, password: String) -> Result<(), CoreError> {
        let handle = self.handle.clone();
        joined(handle.spawn(async move {
                let (base, username) = {
                    let state = self.inner.lock();
                    let ep = state.endpoint.as_ref().ok_or(CoreError::NotConnected)?;
                    (ep.base_url.to_string(), ep.username.clone())
                };
                let auth = {
                    let mut state = self.inner.lock();
                    if state.auth.is_none() {
                        // A corrupt jar is an error the UI must see, never a
                        // panic (same policy as `AuthClient::new`).
                        let jar = AuthClient::new(Some(&self.data_dir))
                            .map_err(|e| CoreError::Io(format!("cookie jar: {e}")))?;
                        state.auth = Some(jar);
                    }
                    state.auth.clone().ok_or(CoreError::NotConnected)?
                };
                auth.login(&base, &username, &password).await
            })
        )
        .await
    }

    /// Forget the gateway: drop the endpoint file, the tab list and the
    /// cookies. Transcripts are irrelevant after this (logout wipes tabs).
    pub async fn forget_gateway(self: Arc<Self>) -> Result<(), CoreError> {
        let handle = self.handle.clone();
        joined(handle.spawn(async move {
                // Stop the supervisor first: it must not keep a socket open
                // (and reconnect!) for a gateway the app just forgot. It
                // clears its own `supervising` flag when it exits.
                self.signal_stop();
                if let Some(client) = self.inner.lock().client.take() {
                    client.close(LOCAL_CLOSE_REASON);
                }
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
        )
        .await
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
        let handle = self.handle.clone();
        joined(handle.spawn(async move { self.open_session_inner(stored_id, cols).await })
        )
        .await
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
        let (key, live_sid, messages) = match &stored_id {
            None => {
                let created = api::create_session(&client, cols).await?;
                (created.stored_session_id, created.session_id, Vec::new())
            }
            Some(id) => {
                let resumed = api::resume_session(&client, id).await?;
                (id.clone(), resumed.session_id, resumed.messages)
            }
        };
        let mut changes = Vec::new();
        let mut state = self.inner.lock();
        // A fresh LiveSession is a rebuild: bump the epoch so an in-flight
        // `send` that snapshotted the PREVIOUS session under this key can
        // never mistake the new transcript for the one it submitted into.
        state.history_epoch += 1;
        let mut live = LiveSession {
            live_sid: live_sid.clone(),
            reducer: Reducer::new(),
            last_seen_seq: 0,
            history_epoch: state.history_epoch,
        };
        if !messages.is_empty() {
            changes.extend(live.reducer.ingest_resume_messages(&key, &messages));
        }
        state.sessions.insert(key.clone(), live);
        state.sid_index.insert(live_sid, key.clone());
        // Re-opening a tab must not blank what the last run persisted: an
        // `upsert` of a fresh record dropped the replay epoch (which the
        // reconnect rebuild check needs non-empty to fire) and the title,
        // leaving the titlebar empty until a `session.title` event happened to
        // land. The transcript IS rebuilt from `messages` here, so the
        // watermark does reset; the epoch and title do not.
        let (title, replay_epoch) = state
            .registry
            .get(&key)
            .map(|rec| (rec.title.clone(), rec.replay_epoch.clone()))
            .unwrap_or_default();
        state.registry.upsert(SessionRecord {
            stored_id: key.clone(),
            last_seen_seq: 0,
            replay_epoch,
            cols,
            title,
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
        let handle = self.handle.clone();
        joined(handle.spawn(async move {
                let mut state = self.inner.lock();
                if let Some(live) = state.sessions.remove(&key) {
                    state.sid_index.remove(&live.live_sid);
                }
                state.emitted_approvals.remove(&key);
                state.registry.close(&key);
                let _ = Self::save_registry(&self.data_dir, &state.registry);
                Ok(())
            })
        )
        .await
    }

    /// The open tabs with their header + running flags (the tab strip).
    pub async fn open_sessions(self: Arc<Self>) -> Vec<SessionSummary> {
        let handle = self.handle.clone();
        let this = Arc::clone(&self);
        handle.spawn(async move {
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

    /// The durable key of the tab that was active when the app last ran, if
    /// the registry still holds it. The app resumes this instead of minting a
    /// fresh session on every launch — without it `sessions.json` grew by one
    /// record per launch and nothing ever read the tab list back.
    pub async fn last_active_session(self: Arc<Self>) -> Option<String> {
        let handle = self.handle.clone();
        let this = Arc::clone(&self);
        handle
            .spawn(async move {
                let state = this.inner.lock();
                let active = state.registry.active_key()?;
                state
                    .registry
                    .get(active)
                    .map(|rec| rec.stored_id.clone())
            })
            .await
            .ok()
            .flatten()
    }

    /// Submit a prompt to the session `key` (T7c contract). On a SUCCESSFUL
    /// submit the core appends the user's own row to the transcript and
    /// delivers it through the same sink fan-out the wire events use — the
    /// server never emits a row event for the user's message, and the change
    /// stream is the single source of rows (no optimistic echo anywhere
    /// else). Both `status:"streaming"` and the busy `status:"redirected"`
    /// count as a successful submit: `redirected` is an ordinary result, not
    /// an error (verified live on v0.21.1, PLAN §1.3) — the sent text became
    /// part of the conversation in both cases, so the row is published for
    /// both, and NEVER for a failed RPC. An unknown `status` on a successful
    /// RPC is [`CoreError::UnexpectedStatus`] and publishes NO row (the
    /// turn's state is unknown; the caller surfaces the error instead of
    /// silently swallowing the message — PR #10 finding 1). The append is
    /// guarded against the submit/resume race (PR #10 finding 2): the submit
    /// RPC is awaited OUTSIDE the lock, so if `resume_all` re-ingested this
    /// session in between (truncated replay / changed epoch), the rebuilt
    /// history already contains the message and the append is skipped — the
    /// per-session history epoch is captured before the await and re-checked
    /// under the lock.
    pub async fn send(self: Arc<Self>, key: String, text: String) -> Result<(), CoreError> {
        let handle = self.handle.clone();
        joined(handle.spawn(async move {
                // One lock for all three: client, live sid and the history
                // epoch snapshot used by the post-submit race re-check.
                let (client, live_sid, history_epoch) = {
                    let state = self.inner.lock();
                    let client = state.client.clone().ok_or(CoreError::NotConnected)?;
                    let live = state
                        .sessions
                        .get(&key)
                        .ok_or(CoreError::NotConnected)?;
                    (client, live.live_sid.clone(), live.history_epoch)
                };
                let status = api::submit(&client, &live_sid, &text).await?;
                if !matches!(status, SubmitStatus::Streaming | SubmitStatus::Redirected) {
                    // A successful RPC with an unknown/missing `status`
                    // leaves the turn's state unknown: publish NO row (the
                    // rebuild path owns the truth) and return a stable error
                    // so the caller surfaces the failure instead of silently
                    // swallowing the user's message (PR #10 finding 1).
                    return Err(CoreError::UnexpectedStatus);
                }
                // The user row is a transcript change like any other: through
                // the reducer, out of the state lock, then to the sink (never
                // a foreign callback under the lock).
                let (changes, rows, sink) = {
                    let mut state = self.inner.lock();
                    let changes = match state.sessions.get_mut(&key) {
                        // A rebuild re-ingested this session while the submit
                        // RPC was in flight: the server-rebuilt history
                        // already contains the user's message — appending
                        // would duplicate the row (PR #10 finding 2). The
                        // submit still succeeded; skip the append.
                        Some(live) if live.history_epoch == history_epoch => {
                            live.reducer.append_user_row(&key, &text)
                        }
                        _ => Vec::new(),
                    };
                    let rows = state
                        .sessions
                        .get(&key)
                        .map(|l| l.reducer.transcript().rows.clone())
                        .unwrap_or_default();
                    (changes, rows, state.sink.clone())
                };
                if let Some(sink) = sink {
                    for change in &changes {
                        // Durable key (see deliver_event): the reducer stamps
                        // the key it was given, the UI routes on the tab key.
                        sink.on_transcript(change_to_dto(&key, &change.change, &rows));
                    }
                }
                Ok(())
            })
        )
        .await
    }

    /// Interrupt the running turn of session `key`.
    pub async fn interrupt(self: Arc<Self>, key: String) -> Result<(), CoreError> {
        let handle = self.handle.clone();
        joined(handle.spawn(async move {
                let (client, live_sid) = {
                    let state = self.inner.lock();
                    let client = state.client.clone().ok_or(CoreError::NotConnected)?;
                    let live_sid = state
                        .sessions
                        .get(&key)
                        .map(|l| l.live_sid.clone())
                        .ok_or(CoreError::NotConnected)?;
                    (client, live_sid)
                };
                api::interrupt(&client, &live_sid)
                    .await
                    .map(|_| ())
                    .map_err(CoreError::from)
            })
        )
        .await
    }

    /// Answer an approval card. Resolves the local card only after the RPC
    /// succeeded (4009 = answered elsewhere still resolves the UI state).
    pub async fn respond_approval(
        self: Arc<Self>,
        key: String,
        request_id: String,
        choice: String,
    ) -> Result<(), CoreError> {
        let handle = self.handle.clone();
        joined(handle.spawn(async move {
                let (client, live_sid) = {
                    let state = self.inner.lock();
                    let client = state.client.clone().ok_or(CoreError::NotConnected)?;
                    let live_sid = state
                        .sessions
                        .get(&key)
                        .map(|l| l.live_sid.clone())
                        .ok_or(CoreError::NotConnected)?;
                    (client, live_sid)
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
        )
        .await
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
        let handle = self.handle.clone();
        joined(handle.spawn(async move {
                let (client, live_sid) = {
                    let state = self.inner.lock();
                    let client = state.client.clone().ok_or(CoreError::NotConnected)?;
                    let live_sid = state
                        .sessions
                        .get(&key)
                        .map(|l| l.live_sid.clone())
                        .ok_or(CoreError::NotConnected)?;
                    (client, live_sid)
                };
                api::respond_clarify(
                    &client,
                    &live_sid,
                    &request_id,
                    &answer,
                    question_id.as_deref(),
                )
                .await?;
                // The resolved card is a transcript change like any other:
                // dropping it here is why the UI never saw a clarify answer
                // (the approval path already delivered its changes).
                let (changes, rows, sink) = {
                    let mut state = self.inner.lock();
                    let changes = state
                        .sessions
                        .get_mut(&key)
                        .map(|l| l.reducer.resolve_clarify(&key, &request_id))
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
                        sink.on_transcript(change_to_dto(&key, &change.change, &rows));
                    }
                }
                Ok(())
            })
        )
        .await
    }

    /// Run a slash command in session `key` (e.g. `/model`).
    pub async fn run_slash(self: Arc<Self>, key: String, command: String) -> Result<String, CoreError> {
        let handle = self.handle.clone();
        joined(handle.spawn(async move {
                let (client, live_sid) = {
                    let state = self.inner.lock();
                    let client = state.client.clone().ok_or(CoreError::NotConnected)?;
                    let live_sid = state
                        .sessions
                        .get(&key)
                        .map(|l| l.live_sid.clone())
                        .ok_or(CoreError::NotConnected)?;
                    (client, live_sid)
                };
                let value = api::slash_exec(&client, &live_sid, &command).await?;
                Ok(json::str_at(&value, "output").to_string())
            })
        )
        .await
    }

    /// Composer completion (gateway-global, no session key).
    pub async fn complete_slash(self: Arc<Self>, text: String) -> Result<Vec<SlashCompletionDto>, CoreError> {
        let handle = self.handle.clone();
        joined(handle.spawn(async move {
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
        )
        .await
    }

    /// Split markdown text into blocks (entity rule, exposed over FFI).
    pub async fn split_markdown(self: Arc<Self>, text: String) -> Vec<MarkdownBlockDto> {
        let handle = self.handle.clone();
        handle.spawn(async move {
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
        let handle = self.handle.clone();
        joined(handle.spawn(async move {
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
        )
        .await
    }

    /// Deliberate disconnect (PI_TASK_T5A §3): set `stop_flag` so the
    /// supervisor never reconnects, close the client with
    /// [`LOCAL_CLOSE_REASON`] (the supervisor treats that reason as
    /// terminal), clear the client handle and emit a `Closed` status.
    /// Transcripts and the registry survive (the app keeps rendering tabs
    /// offline).
    pub async fn disconnect(self: Arc<Self>) -> Result<(), CoreError> {
        let handle = self.handle.clone();
        joined(handle.spawn(async move {
                self.signal_stop();
                let client = self.inner.lock().client.take();
                if let Some(client) = client {
                    client.close(LOCAL_CLOSE_REASON);
                }
                self.flush_registry();
                // No `Closed` emission here: the SUPERVISOR observes the
                // client's state watch and is the single source of connection
                // status. Emitting here too produced two `Closed` events per
                // user action (caught by the live gate).
                Ok(())
            })
        )
        .await
    }

    /// The app came back to the foreground: probe the connection with the
    /// short-timeout liveness ping and, when the supervisor is gone, swap
    /// in a fresh client and restart it. A live supervisor handles
    /// everything itself; this must never spawn a second one (PLAN §3).
    pub async fn app_did_foreground(self: Arc<Self>) -> Result<(), CoreError> {
        let handle = self.handle.clone();
        joined(handle.spawn(async move {
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
                // A previous supervisor may have exited after a stop: the new
                // one must not inherit the raised flag (it would exit at once).
                self.stop_flag.store(false, Ordering::Relaxed);
                drop(self.handle.spawn(Self::supervise(self.clone(), client, sink)));
                Ok(())
            })
        )
        .await
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
        let handle = self.handle.clone();
        joined(handle.spawn(async move { self.connect_inner(sink).await })
        )
        .await
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
        // A second `connect` while a supervisor is alive must NOT start a
        // second one: two supervisors would double-deliver every change and
        // fight over the reconnect. The sink is still updated above.
        if self.supervising.swap(true, Ordering::Relaxed) {
            log::warn!("connect: a supervisor is already running, keeping it");
            return Ok(());
        }
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
        // `supervise_inner` takes the Arc by value, so hand it a clone and keep
        // this one to clear the flag afterwards.
        let this = Arc::clone(&self);
        this.supervise_inner(client, sink).await;
        // The supervisor is gone (deliberate close, NeedsPassword, stop): clear
        // the flag, or `app_did_foreground` would see a "live" supervisor
        // forever and only ping instead of recovering.
        self.supervising.store(false, Ordering::Relaxed);
    }

    async fn supervise_inner(
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

            // `GatewayClient::connect` marks the state Open before it returns,
            // and `watch::Sender::subscribe` hands back a receiver that already
            // counts the current value as seen — so `state.changed()` below
            // never fires for THIS connection's Open and the arm in the select
            // was unreachable. Publish from the initial value instead, or the
            // app never hears that the socket came up.
            let mut dead_on_arrival = false;
            match state.borrow_and_update().clone() {
                ConnectionState::Open => {
                    attempt = 0;
                    sink.on_connection(ConnectionStatus::Open);
                }
                ConnectionState::Closed(reason) => {
                    sink.on_connection(ConnectionStatus::Closed { reason: reason.clone() });
                    if reason == LOCAL_CLOSE_REASON {
                        self.flush_registry();
                        return;
                    }
                    // Already terminal: the watch will never change again and
                    // the broadcast keepalive means `recv()` would block
                    // forever, so skip the event loop and take the ladder.
                    dead_on_arrival = true;
                }
                _ => {}
            }

            // ── resume phase: every open session, every (re)connect ─────
            let fresh_epoch = current.replay_epoch();
            if !dead_on_arrival {
                self.resume_all(&current, &fresh_epoch, &sink).await;
            }

            // ── event loop ───────────────────────────────────────────────
            let mut flush_tick = tokio::time::interval(REGISTRY_FLUSH_INTERVAL);
            flush_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            flush_tick.tick().await; // the first tick resolves immediately
            // The flag cannot change inside the loop, so this is a guard,
            // not a condition (clippy::while_immutable_condition).
            if !dead_on_arrival {
                loop {
                    tokio::select! {
                        _ = flush_tick.tick() => {
                            self.flush_registry();
                        }
                        ev = events.recv() => match ev {
                            Ok(GatewayEvent::Event(params)) => {
                                self.deliver_event(&params, &sink);
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                // Broadcast overflow: fill the gap from the
                                // per-session watermarks.
                                log::warn!("supervisor lagged, dropped {n} events");
                                self.catch_up_all(&current, &sink).await;
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
                                        self.flush_registry();
                                        return;
                                    }
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }

            // ── reconnect ladder ─────────────────────────────────────────
            // The socket is down: the watermarks are worth more on disk than
            // in memory now.
            self.flush_registry();
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
                    self.flush_registry();
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
                        self.flush_registry();
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
            // Coalesced, never written inline: see REGISTRY_FLUSH_INTERVAL.
            self.registry_dirty.store(true, Ordering::Relaxed);
        }
        for batch in deliveries {
            for change in batch {
                sink.on_transcript(change);
            }
        }
    }

    /// Resolve when `stop_flag` is set (the ladder's interruptible sleep).
    /// The `notified()` future is registered BEFORE the flag is read, so a
    /// `signal_stop` landing between the two still wakes this future instead
    /// of parking it for the rest of the backoff step.
    async fn sleep_until_stopped(&self) {
        loop {
            let waiter = self.stop_notify.notified();
            if self.stop_flag.load(Ordering::Relaxed) {
                return;
            }
            waiter.await;
        }
    }

    /// Raise the stop flag and wake every waiter on it.
    fn signal_stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        self.stop_notify.notify_waiters();
    }

    /// Write the session registry out if an event moved a watermark since the
    /// last flush. A no-op when nothing changed.
    fn flush_registry(&self) {
        if !self.registry_dirty.swap(false, Ordering::Relaxed) {
            return;
        }
        let state = self.inner.lock();
        if let Err(e) = Self::save_registry(&self.data_dir, &state.registry) {
            log::warn!("session registry flush failed: {e}");
            self.registry_dirty.store(true, Ordering::Relaxed);
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
                // Rebuild ONLY this session from the resume messages (the
                // single rebuild path, shared with the submit/resume race
                // guard in `send`).
                let changes = self.rebuild_session(
                    &key,
                    &resumed.messages,
                    Some(replay.epoch.as_str()).filter(|e| !e.is_empty()),
                );
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

    /// Rebuild ONE session's transcript from resume history — the single
    /// rebuild path (PR #10 finding 2): `ingest_resume_messages` (Reset +
    /// one `RowAppended` per rebuilt row), a bumped per-session history
    /// epoch (the re-check `send` uses to skip its append when a rebuild
    /// raced the submit), the watermark reset and the registry refresh.
    /// Returns the DTOs the sink must receive; the caller delivers them
    /// (never a foreign callback under the lock). `replay_epoch` (when
    /// present) is recorded so the next reconnect's epoch comparison sees it.
    fn rebuild_session(
        &self,
        key: &str,
        messages: &[Value],
        replay_epoch: Option<&str>,
    ) -> Vec<TranscriptChangeDto> {
        let mut state = self.inner.lock();
        state.history_epoch += 1;
        let epoch = state.history_epoch;
        let Some(live) = state.sessions.get_mut(key) else {
            return Vec::new();
        };
        live.history_epoch = epoch;
        let changes = live.reducer.ingest_resume_messages(key, messages);
        live.last_seen_seq = 0;
        let rows = live.reducer.transcript().rows.clone();
        if let Some(rec) = state.registry.get_mut(key) {
            rec.last_seen_seq = 0;
            if let Some(epoch) = replay_epoch {
                rec.replay_epoch = epoch.to_string();
            }
        }
        changes
            .into_iter()
            // Durable key, not `c.key` (see deliver_event).
            .map(|c| change_to_dto(key, &c.change, &rows))
            .collect()
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
    async fn catch_up_all(&self, client: &GatewayClient, sink: &Arc<dyn EventSink>) {
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
                let (changes, rows) = {
                    let mut state = self.inner.lock();
                    // Dedupe BEFORE touching the reducer. Applying the card
                    // first and only then honouring `already` appended a row
                    // the sink never heard about, so the core's transcript and
                    // the app's row list drifted apart by one and every later
                    // index in the change stream addressed the wrong row.
                    let already = state
                        .emitted_approvals
                        .get(&key)
                        .map(|v| v.iter().any(|id| id == &request_id))
                        .unwrap_or(false);
                    let mut changes: Vec<SessionChange> = Vec::new();
                    let mut rows: Vec<Row> = Vec::new();
                    if !already {
                        if let Some(live) = state.sessions.get_mut(&key) {
                            // Dedupe, second half: a card already sitting on
                            // THIS transcript unresolved is not re-emitted.
                            if !live.reducer.has_unresolved_approval(&request_id) {
                                if let Some(emitted) =
                                    live.reducer.apply_approval_card(&key, &request_id, card)
                                {
                                    changes = emitted;
                                    rows = live.reducer.transcript().rows.clone();
                                }
                            }
                        }
                    }
                    if !changes.is_empty() {
                        state
                            .emitted_approvals
                            .entry(key.clone())
                            .or_default()
                            .push(request_id.clone());
                    }
                    (changes, rows)
                };
                if changes.is_empty() {
                    continue;
                }
                for change in &changes {
                    sink.on_transcript(change_to_dto(&key, &change.change, &rows));
                }
            }
        }
    }
}

// ── tests (T5 stream B) ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use std::sync::Mutex as StdMutex;
    use tokio_tungstenite::tungstenite::Message;
    type WsStream = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;

    /// Monotonic temp-dir names: no two tests share a data_dir.
    fn temp_dir(tag: &str) -> PathBuf {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "hermo-core-{}-{}-{}",
            tag,
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create data_dir");
        dir
    }

    /// Records every DTO the core delivers, for routing assertions.
    #[derive(Default)]
    struct RecordingSink {
        transcript: StdMutex<Vec<TranscriptChangeDto>>,
        connections: StdMutex<Vec<String>>,
    }

    impl RecordingSink {
        fn transcript(&self) -> Vec<TranscriptChangeDto> {
            self.transcript.lock().unwrap().clone()
        }
        /// Deliveries whose row is an approval card (`row_json` is compact).
        fn approval_deliveries(&self) -> usize {
            self.transcript()
                .iter()
                .filter(|c| c.row_json.contains("\"kind\":\"approval\""))
                .count()
        }
    }

    impl EventSink for RecordingSink {
        fn on_transcript(&self, change: TranscriptChangeDto) {
            self.transcript.lock().unwrap().push(change);
        }
        fn on_connection(&self, status: ConnectionStatus) {
            let name = match status {
                ConnectionStatus::Connecting => "connecting".to_string(),
                ConnectionStatus::Open => "open".to_string(),
                ConnectionStatus::Closed { reason } => format!("closed({reason})"),
                ConnectionStatus::NeedsPassword => "needs-password".to_string(),
            };
            self.connections.lock().unwrap().push(name);
        }
    }

    // ── fake gateway (in-process WS, no port binding beyond 127.0.0.1:0) ────

    /// A minimal fake gateway: accepts ONE connection, answers every JSON-RPC
    /// request via `handler`, and can push wire events.
    struct FakeGw {
        url: String,
        /// Wire frames to push (events/framed RPC replies).
        #[allow(dead_code)] // harness for the fake-gateway end-to-end tests
        push: tokio::sync::mpsc::UnboundedSender<String>,
    }

    impl FakeGw {
        /// `handler` receives (method, params) and returns the JSON-RPC result
        /// value (or an Err message string -> error object).
        fn spawn<F, Fut>(handler: F) -> Self
        where
            F: Fn(String, Value) -> Fut + Send + 'static,
            Fut: std::future::Future<Output = Result<Value, String>> + Send + 'static,
        {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
            listener.set_nonblocking(true).expect("nonblocking");
            let listener = tokio::net::TcpListener::from_std(listener).expect("tokio listener");
            let addr = listener.local_addr().expect("addr");
            let (push_tx, mut push_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            // The gateway always greets with `gateway.ready`; pushed into the
            // channel first, so it is the first frame the client receives.
            let _ = push_tx.send(
                json!({
                    "jsonrpc": "2.0", "method": "event",
                    "params": {
                        "type": "gateway.ready",
                        "payload": {"heartbeat": true, "replay_epoch": "e-fake", "skin": {}, "change_events": true}
                    }
                })
                .to_string(),
            );
            tokio::spawn(async move {
                let Ok((stream, _)) = listener.accept().await else { return };
                let Ok(ws) = tokio_tungstenite::accept_async(stream).await else { return };
                let (mut sink, mut stream_r): (
                    futures_util::stream::SplitSink<WsStream, Message>,
                    futures_util::stream::SplitStream<WsStream>,
                ) = ws.split();
                // Frames pushed before the client finished connecting are
                // queued and drained first (a push after connect would lose
                // `gateway.ready` to the ready-timeout race).
                let mut queued: Vec<String> = Vec::new();
                loop {
                    // First drain the pre-connect queue in order.
                    if let Some(line) = if queued.is_empty() { None } else { Some(queued.remove(0)) } {
                        if sink.send(Message::text(line)).await.is_err() { return; }
                        continue;
                    }
                    tokio::select! {
                        out = push_rx.recv() => {
                            match out {
                                Some(line) => {
                                    if sink.send(Message::text(line)).await.is_err() { return; }
                                }
                                None => return,
                            }
                        }
                        inbound = stream_r.next() => {
                            let Some(Ok(Message::Text(text))) = inbound else { return };
                            for line in text.split('\n').filter(|l| !l.trim().is_empty()) {
                                let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
                                    continue;
                                };
                                // Heartbeats get the trivial answer, never the handler.
                                if v.get("method").and_then(Value::as_str) == Some("gateway.ping") {
                                    let reply = json!({
                                        "jsonrpc": "2.0", "id": v.get("id").cloned().unwrap_or(Value::Null),
                                        "result": {"ok": true}
                                    });
                                    if sink.send(Message::text(reply.to_string())).await.is_err() { return; }
                                    continue;
                                }
                                let method = v.get("method").and_then(Value::as_str).unwrap_or("").to_string();
                                let params = v.get("params").cloned().unwrap_or(Value::Null);
                                let id = v.get("id").cloned().unwrap_or(Value::Null);
                                let result = handler(method, params).await;
                                let reply = match result {
                                    Ok(value) => json!({"jsonrpc": "2.0", "id": id, "result": value}),
                                    Err(msg) => json!({"jsonrpc": "2.0", "id": id,
                                        "error": {"code": -32000, "message": msg}}),
                                };
                                if sink.send(Message::text(reply.to_string())).await.is_err() { return; }
                            }
                        }
                    }
                }
            });
            FakeGw { url: format!("ws://{addr}"), push: push_tx }
        }

        /// Push one decoded wire event.
        #[allow(dead_code)] // harness kept for the T5 fake-gateway end-to-end tests
        fn push_event(&self, event_type: &str, session_id: &str, seq: i64, payload: Value) {
            let frame = json!({
                "jsonrpc": "2.0", "method": "event",
                "params": {
                    "type": event_type, "session_id": session_id, "seq": seq, "payload": payload
                }
            });
            let _ = self.push.send(frame.to_string());
        }

        /// The `gateway.ready` event frame the core's client waits for.
        #[allow(dead_code)] // harness for the fake-gateway end-to-end tests
        fn push_ready(&self) {
            let frame = json!({
                "jsonrpc": "2.0", "method": "event",
                "params": {
                    "type": "gateway.ready",
                    "payload": {"heartbeat": true, "replay_epoch": "e-fake", "skin": {}, "change_events": true}
                }
            });
            let _ = self.push.send(frame.to_string());
        }

        /// Push one decoded wire event, already framed by the caller.
        #[allow(dead_code)] // used by the routing tests that push live events
        fn push_raw(&self, line: String) {
            let _ = self.push.send(line);
        }
    }

    /// Standard `session.create` reply.
    #[allow(dead_code)] // harness for the fake-gateway end-to-end tests
    fn created(sid: &str, stored: &str) -> Value {
        json!({"session_id": sid, "stored_session_id": stored, "message_count": 0})
    }

    /// A core wired to `gw` with a recording sink, as after `connect`:
    /// the client is live (the fake answered `gateway.ready`) and the
    /// supervisor is NOT spawned — routing tests drive `deliver_event`
    /// (the exact production delivery path) synchronously instead of
    /// racing the event loop.
    async fn connected_core(dir: &std::path::Path, gw: &FakeGw) -> (Arc<HermesCore>, Arc<RecordingSink>) {
        let core = HermesCore::new(dir.to_string_lossy().to_string());
        core.inner.lock().endpoint = Some(GatewayEndpoint::new(
            url::Url::parse("http://gateway.test").expect("url"),
            "user".to_string(),
            "test".to_string(),
        ));
        core.inner.lock().auth = Some(AuthClient::new(None).expect("auth"));
        let sink = Arc::new(RecordingSink::default());
        // Same connection path as `connect` minus the auth HTTP round trip
        // (the ticket minting needs a real HTTP endpoint; the WS client and
        // the supervisor fan-out below are the code under test).
        let client = GatewayClient::connect(&gw.url, ClientConfig::for_tests())
            .await
            .expect("fake connect");
        core.inner.lock().client = Some(client);
        core.inner.lock().sink = Some(sink.clone());
        core.stop_flag.store(false, Ordering::Relaxed);
        core.supervising.store(true, Ordering::Relaxed);
        (core, sink)
    }

    /// Register a live session directly in the core state (the part of
    /// `open_session` that matters for routing) and return its durable key.
    fn attach_session(core: &HermesCore, durable_key: &str, live_sid: &str) {
        let mut state = core.inner.lock();
        state.history_epoch += 1;
        let epoch = state.history_epoch;
        state.sessions.insert(
            durable_key.to_string(),
            LiveSession {
                live_sid: live_sid.to_string(),
                reducer: Reducer::new(),
                last_seen_seq: 0,
                history_epoch: epoch,
            },
        );
        state.sid_index.insert(live_sid.to_string(), durable_key.to_string());
        state.registry.upsert(SessionRecord {
            stored_id: durable_key.to_string(),
            last_seen_seq: 0,
            replay_epoch: String::new(),
            cols: DEFAULT_COLS,
            title: String::new(),
        });
    }

    /// The event the fan-out routes (wire shape).
    fn ev(event_type: &str, session_id: &str, seq: i64, payload: Value) -> EventParams {
        EventParams {
            event_type: event_type.to_string(),
            session_id: session_id.to_string(),
            seq: Some(seq),
            payload,
        }
    }

    // ── test 1: durable-key routing (the defect the live gate caught) ───────

    /// Rule pinned (PI_TASK_T5B §1): a change produced for a session whose
    /// live sid is `S` reaches the sink carrying the DURABLE session key,
    /// never the live sid and never another session's key. The entity's
    /// `SessionChange.key` IS the live sid (the reducer stamps it); if the
    /// delivery path reverts to `change.key` — the exact bug that made every
    /// change invisible to the UI — this test fails.
    /// Rule pinned (PI_TASK_T5B §2): `gateway.ready`, `sessions.changed` and
    /// ANY event without a session id emit nothing to the sink and add no
    /// row to any transcript.
    #[tokio::test]
    async fn session_less_events_emit_nothing_and_add_no_rows() {
        let dir = temp_dir("sessionless");
        let gw = FakeGw::spawn(|_m, _p| async { Ok(json!({})) });
        let (core, sink) = connected_core(&dir, &gw).await;
        attach_session(&core, "stored-alpha", "live-A");

        core.deliver_event(&ev("gateway.ready", "", 1, json!({"heartbeat": true})), &(sink.clone() as Arc<dyn EventSink>));
        core.deliver_event(&ev("sessions.changed", "", 2, json!({})), &(sink.clone() as Arc<dyn EventSink>));
        // seq None and an unknown type with no sid: still nothing.
        core.deliver_event(
            &EventParams { event_type: "skin.changed".into(), session_id: String::new(), seq: None, payload: json!({}) },
            &(sink.clone() as Arc<dyn EventSink>),
        );

        assert!(
            sink.transcript().is_empty(),
            "session-less events must not reach the sink: {:?}",
            sink.transcript()
        );
        let state = core.inner.lock();
        let rows = state
            .sessions
            .get("stored-alpha")
            .map(|l| l.reducer.transcript().rows.len())
            .unwrap_or(0);
        assert_eq!(rows, 0, "session-less events must add no row");
        assert_eq!(state.registry.get("stored-alpha").unwrap().last_seen_seq, 0,
            "a session-less seq must not move the watermark");
    }

    // ── test 3: registry round-trip through the core ────────────────────────

    /// Rule pinned (PI_TASK_T5B §3): everything the app needs to restore its
    /// tabs — the tab list, its order, the active tab and the per-session
    /// watermarks — survives building a NEW `HermesCore` over the same
    /// `data_dir`. Fails if `save_registry` stopped being atomic (a torn
    /// `sessions.json` would not parse) or losty (dropped fields).
    ///
    /// Also pins the write-coalescing contract: `deliver_event` only marks the
    /// registry dirty, and `flush_registry` is the single writer.
    #[test]
    fn registry_round_trip_through_a_new_core_instance() {
        let dir = temp_dir("registry");

        // Instance 1: two tabs, one active, watermarks moved by delivery.
        let core1 = HermesCore::new(dir.to_string_lossy().to_string());
        attach_session(&core1, "stored-alpha", "live-A");
        attach_session(&core1, "stored-beta", "live-B");
        {
            let mut state = core1.inner.lock();
            state.registry.set_active(Some("stored-beta"));
            // Simulate routed traffic having moved the watermarks.
            state.registry.touch("stored-alpha", 41, "e-1");
            state.registry.touch("stored-beta", 7, "");
        }
        let sink = Arc::new(RecordingSink::default());
        core1.deliver_event(
            &ev("message.start", "live-A", 42, json!({})),
            &(sink.clone() as Arc<dyn EventSink>),
        );
        // Delivery marks the registry dirty and writes NOTHING: the file is
        // rewritten on a timer, not once per streamed event
        // (REGISTRY_FLUSH_INTERVAL). The flush is what persists.
        assert!(
            core1.registry_dirty.load(Ordering::Relaxed),
            "delivery marks the registry dirty",
        );
        assert!(
            !dir.join(SESSIONS_FILE).exists(),
            "delivery does not write the registry inline",
        );
        core1.flush_registry();
        assert!(
            !core1.registry_dirty.load(Ordering::Relaxed),
            "a flush clears the dirty flag",
        );
        drop(core1);

        // Instance 2: same data_dir, fresh process state.
        let core2 = HermesCore::new(dir.to_string_lossy().to_string());
        let state = core2.inner.lock();
        let ids: Vec<&str> = state.registry.sessions().iter().map(|r| r.stored_id.as_str()).collect();
        assert_eq!(ids, vec!["stored-alpha", "stored-beta"], "tab list and order survive");
        assert_eq!(state.registry.active_key(), Some("stored-beta"), "active tab survives");
        let alpha = state.registry.get("stored-alpha").expect("alpha persisted");
        assert_eq!(alpha.last_seen_seq, 42, "watermark survives (delivery touched it)");
        assert_eq!(alpha.replay_epoch, "e-1", "epoch survives");
        let beta = state.registry.get("stored-beta").expect("beta persisted");
        assert_eq!(beta.last_seen_seq, 7, "per-session watermarks are independent");
    }

    // ── test 4: approval dedupe ─────────────────────────────────────────────

    #[tokio::test]
    async fn delivery_stamps_the_durable_session_key_not_the_live_sid() {
        let dir = temp_dir("routing");
        let gw = FakeGw::spawn(|_m, _p| async { Ok(json!({})) });
        let (core, sink) = connected_core(&dir, &gw).await;
        // Two open tabs: durable keys differ from their live sids.
        attach_session(&core, "stored-alpha", "live-A");
        attach_session(&core, "stored-beta", "live-B");

        // Interleaved events for both live sids.
        core.deliver_event(
            &ev("message.start", "live-A", 1, json!({})),
            &(sink.clone() as Arc<dyn EventSink>),
        );
        core.deliver_event(
            &ev("message.delta", "live-B", 1, json!({"text": "b"})),
            &(sink.clone() as Arc<dyn EventSink>),
        );
        core.deliver_event(
            &ev("message.delta", "live-A", 2, json!({"text": "a"})),
            &(sink.clone() as Arc<dyn EventSink>),
        );

        let deliveries = sink.transcript();
        assert_eq!(deliveries.len(), 3, "every change is delivered once");
        let by_key: Vec<&str> = deliveries.iter().map(|d| d.key.as_str()).collect();
        assert_eq!(
            by_key,
            vec!["stored-alpha", "stored-beta", "stored-alpha"],
            "every DTO carries its DURABLE key, in routing order (got {by_key:?})"
        );
        // Belt and braces: the live sids appear NOWHERE.
        for d in &deliveries {
            assert_ne!(d.key, "live-A", "a DTO carried the live sid: {d:?}");
            assert_ne!(d.key, "live-B", "a DTO carried the live sid: {d:?}");
        }
        // The deltas landed in the right transcripts (routing is not just a
        // relabel: text must not cross between tabs).
        let alpha_text = {
            let state = core.inner.lock();
            state
                .sessions
                .get("stored-alpha")
                .map(|l| l.reducer.transcript().rows.clone())
                .unwrap_or_default()
        };
        assert_eq!(alpha_text.len(), 1, "alpha got exactly one assistant row");
        let beta_rows = {
            let state = core.inner.lock();
            state.sessions.get("stored-beta").map(|l| l.reducer.transcript().rows.clone()).unwrap_or_default()
        };
        assert_eq!(beta_rows.len(), 1, "beta got exactly one assistant row");
    }

    // ── test 2: session-less events ─────────────────────────────────────────

    /// Rule pinned (PI_TASK_T5B §2): `gateway.ready`, `sessions.changed` and
    /// ANY event without a session id emit nothing to the sink and add no
    /// row to any transcript.
    #[tokio::test]
    async fn reemit_approvals_dedupes_by_request_id_on_the_real_path() {
        // Rule pinned: the reconnect re-emission path ITSELF is deduped by
        // `request_id`, on both halves of the guard — the emitted set and an
        // unresolved card already on the transcript. The first version of this
        // test replicated the checks by hand instead of calling
        // `reemit_approvals`, so it stayed green while the dedupe had been
        // removed from the shipping code (caught in review #5).
        let dir = temp_dir("approvals");
        // The gateway keeps answering with the SAME pending card.
        let gw = FakeGw::spawn(|method, _params| async move {
            if method == "approval.pending" {
                Ok(json!({
                    "pending": [{
                        "request_id": "req-1",
                        "command": "rm -r /tmp/x",
                        "description": "d",
                        "choices": ["allow", "deny"]
                    }]
                }))
            } else {
                Ok(json!({}))
            }
        });
        let (core, sink) = connected_core(&dir, &gw).await;
        attach_session(&core, "stored-alpha", "live-A");
        let client = core.inner.lock().client.clone().expect("client");
        let sink_dyn = sink.clone() as Arc<dyn EventSink>;

        core.reemit_approvals(&client, &sink_dyn).await;
        assert_eq!(
            sink.approval_deliveries(),
            1,
            "the pending card is emitted exactly once"
        );

        // Same answer, same request_id: the emitted set blocks a second one.
        core.reemit_approvals(&client, &sink_dyn).await;
        assert_eq!(
            sink.approval_deliveries(),
            1,
            "a request_id already emitted is never delivered again"
        );

        // Second half of the guard, on its own: forget the emitted set (a
        // fresh process would have an empty one) and re-run. The UNRESOLVED
        // CARD ON THE TRANSCRIPT must still block the re-emission.
        core.inner.lock().emitted_approvals.clear();
        core.reemit_approvals(&client, &sink_dyn).await;
        assert_eq!(
            sink.approval_deliveries(),
            1,
            "an approval card already unresolved on the transcript is not duplicated"
        );
        let approval_rows = {
            let state = core.inner.lock();
            state
                .sessions
                .get("stored-alpha")
                .map(|l| {
                    l.reducer
                        .transcript()
                        .rows
                        .iter()
                        .filter(|r| matches!(r.kind, RowKind::Approval(_)))
                        .count()
                })
                .unwrap_or(0)
        };
        assert_eq!(approval_rows, 1, "exactly one approval row exists");

        // The case the dedupe used to get wrong: the request_id IS in the
        // emitted set AND the card has since been resolved, so
        // `has_unresolved_approval` says no. The old order applied the card to
        // the reducer first and only then honoured the emitted set, appending
        // a row the sink never heard about — the core's transcript and the
        // app's row list then disagreed on every later index.
        {
            let mut state = core.inner.lock();
            // An earlier step cleared the emitted set to exercise the other
            // half of the guard; put req-1 back, then resolve the card.
            state
                .emitted_approvals
                .insert("stored-alpha".to_string(), vec!["req-1".to_string()]);
            let live = state.sessions.get_mut("stored-alpha").expect("session");
            live.reducer.resolve_approval("stored-alpha", "req-1");
        }
        let deliveries_before = sink.transcript().len();
        core.reemit_approvals(&client, &sink_dyn).await;
        assert_eq!(
            sink.transcript().len(),
            deliveries_before,
            "a resolved, already-emitted card delivers nothing",
        );
        let rows_after = {
            let state = core.inner.lock();
            state
                .sessions
                .get("stored-alpha")
                .map(|l| l.reducer.transcript().rows.len())
                .unwrap_or(0)
        };
        assert_eq!(
            rows_after, 1,
            "and appends no row the sink was never told about",
        );
    }

    // ── test 5: the supervisor's ladder attempt behaviour ───────────────────

    /// Rule pinned (PI_TASK_T5B §5): the supervisor uses the SAME schedule as
    /// `reconnect.rs` (reset to the first step after a success, capped at the
    /// final step for the life of the connection). The supervisor's attempt
    /// counter is not directly reachable without a socket, so the ladder
    /// arithmetic it calls — `delay_for_attempt` reset/cap semantics — is
    /// pinned here against the supervisor's call shape
    /// (`jittered_delay(attempt, clock_jitter())`, `attempt += 1` per retry,
    /// `attempt = 0` on success): a changed ladder or a non-resetting
    /// supervisor's counter breaks these invariants.
    #[test]
    fn supervisor_ladder_resets_on_success_and_caps_at_the_final_step() {
        // A supervisor that just connected: attempt 0 -> first step (1 s ±20%).
        let first = reconnect::jittered_delay(0, clock_jitter());
        assert!(
            (1_000..=1_200).contains(&first),
            "a fresh attempt (reset counter) waits the FIRST step, got {first} ms"
        );
        // A supervisor whose ladder ran past the last step holds at 15 s.
        for attempt in [5u32, 6, 20, u32::MAX] {
            let d = reconnect::jittered_delay(attempt, 0.0);
            assert_eq!(d, reconnect::BACKOFF_MS[reconnect::BACKOFF_MS.len() - 1],
                "attempt {attempt} caps at the final step");
        }
        // The full ladder the supervisor walks while failing, with the reset
        // in the middle, never leaves the schedule's bounds.
        for attempt in 0..10u32 {
            let d = reconnect::jittered_delay(attempt, 0.5);
            let base = reconnect::delay_for_attempt(attempt);
            assert!(d >= base && d <= base + base / 5, "attempt {attempt}: {d} ms out of bounds");
        }
    }

    // ── test 6: Drop inside an async context ────────────────────────────────

    /// Rule pinned (PI_TASK_T5B §6, the second live-gate defect): dropping
    /// `HermesCore` from INSIDE an async context must not panic. `Drop` moves
    /// the runtime to a detached thread (`shutdown_background`); if it ever
    /// drops the runtime on the current thread again, `Runtime::drop` blocks
    /// on the workers and tokio panics with "Cannot drop a runtime in a
    /// context where blocking is not allowed".
    #[tokio::test]
    async fn dropping_core_inside_async_context_does_not_panic() {
        let core = HermesCore::new(temp_dir("drop").to_string_lossy().to_string());
        // Two workers have nothing parked; drop it right here, inside the
        // tokio test runtime's worker context.
        drop(core);
        // Yield once so any mis-dropped runtime surfaces synchronously.
        tokio::task::yield_now().await;
    }

    /// Rule pinned: when the supervisor exits, `supervising` is cleared. Without
    /// it, `app_did_foreground` sees a "live" supervisor forever and only pings
    /// instead of recovering (review #5, should 2).
    #[tokio::test]
    async fn supervisor_clears_the_supervising_flag_on_exit() {
        let dir = temp_dir("supervising");
        let gw = FakeGw::spawn(|_m, _p| async { Ok(json!({})) });
        let (core, sink) = connected_core(&dir, &gw).await;
        let client = core.inner.lock().client.clone().expect("client");
        core.supervising.store(true, Ordering::Relaxed);
        let task = tokio::spawn(HermesCore::supervise(
            core.clone(),
            client.clone(),
            sink.clone() as Arc<dyn EventSink>,
        ));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // A deliberate local close is terminal for the supervisor.
        client.close(LOCAL_CLOSE_REASON);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), task).await;
        assert!(
            !core.supervising.load(Ordering::Relaxed),
            "the flag must be cleared when the supervisor exits"
        );
    }

    /// Rule pinned: answering a clarification delivers the resolved card to the
    /// sink. The pre-review code resolved the reducer state and threw the
    /// changes away, so the UI never saw the answer (review #5, should 3).
    #[tokio::test]
    async fn respond_clarify_delivers_the_resolved_card() {
        let dir = temp_dir("clarify");
        let gw = FakeGw::spawn(|method, _params| async move {
            if method == "clarify.respond" {
                Ok(json!({"status": "ok"}))
            } else {
                Ok(json!({}))
            }
        });
        let (core, sink) = connected_core(&dir, &gw).await;
        attach_session(&core, "stored-A", "live-A");
        let sink_dyn = sink.clone() as Arc<dyn EventSink>;
        core.deliver_event(
            &ev(
                "clarify.request",
                "live-A",
                1,
                json!({"request_id": "c-1", "question": "Proceed?", "choices": ["yes", "no"]}),
            ),
            &sink_dyn,
        );
        let before = sink.transcript().len();
        assert_eq!(before, 1, "the clarify card opened exactly one row");

        core.respond_clarify(
            "stored-A".to_string(),
            "c-1".to_string(),
            "yes".to_string(),
            None,
        )
        .await
        .expect("respond_clarify");

        let after = sink.transcript();
        assert!(
            after.len() > before,
            "the resolved clarify card must reach the sink (it was dropped before review #5)"
        );
        assert_eq!(
            after.last().map(|c| c.key.as_str()),
            Some("stored-A"),
            "and it carries the durable key"
        );
    }

    // ── test 7: the user row at submit (T7c defect B) ────────────────────

    /// Rule pinned (T7c): after a SUCCESSFUL `prompt.submit` (`status:
    /// "streaming"`) the core appends the user's own row to the transcript
    /// and delivers it through the same sink fan-out as every wire event —
    /// the stream is the single source of rows. Pre-fix the core created no
    /// row at submit and the app placed an optimistic echo at `rows.size`,
    /// which the turn's first `ROW_UPDATED` then overwrote (same index,
    /// different meaning): the sent text never appeared as its own row.
    #[tokio::test]
    async fn send_publishes_the_user_row_through_the_sink() {
        let dir = temp_dir("send-user-row");
        let gw = FakeGw::spawn(|method, _params| async move {
            match method.as_str() {
                "prompt.submit" => Ok(json!({"status": "streaming"})),
                _ => Ok(json!({})),
            }
        });
        let (core, sink) = connected_core(&dir, &gw).await;
        attach_session(&core, "stored-alpha", "live-A");

        core.clone().send("stored-alpha".to_string(), "log a meal".to_string())
            .await
            .expect("send");

        let deliveries = sink.transcript();
        let user_rows: Vec<&TranscriptChangeDto> = deliveries
            .iter()
            .filter(|c| {
                c.row_json.contains("\"kind\":\"user\"") && c.row_json.contains("log a meal")
            })
            .collect();
        assert_eq!(
            user_rows.len(),
            1,
            "the user row is delivered exactly once: {:?}",
            deliveries
        );
        assert_eq!(user_rows[0].key, "stored-alpha", "it carries the durable key");
        assert!(
            matches!(user_rows[0].kind, TranscriptChangeKind::RowAppended),
            "delivered as RowAppended, not RowUpdated"
        );
        // It IS the transcript state: the row sits in the reducer (the app
        // sees the same row it would after a resume of this session).
        let rows = {
            let state = core.inner.lock();
            state
                .sessions
                .get("stored-alpha")
                .map(|l| l.reducer.transcript().rows.clone())
                .unwrap_or_default()
        };
        assert_eq!(rows.len(), 1, "no other row is created by send");
        assert_eq!(rows[0].index, 0);
        assert!(
            matches!(&rows[0].kind, RowKind::User { text } if text == "log a meal"),
            "the reducer holds a User row with the sent text"
        );
    }

    /// Rule pinned (T7c): a `redirected` submit is an ordinary successful
    /// result (verified live on v0.21.1, PLAN §1.3 — the server answers it
    /// when a turn was already running) and the sent message is still part
    /// of the conversation: the user row is published for it too.
    #[tokio::test]
    async fn send_publishes_the_user_row_even_when_redirected() {
        let dir = temp_dir("send-user-row-redirected");
        let gw = FakeGw::spawn(|method, _params| async move {
            match method.as_str() {
                "prompt.submit" => Ok(json!({"status": "redirected"})),
                _ => Ok(json!({})),
            }
        });
        let (core, sink) = connected_core(&dir, &gw).await;
        attach_session(&core, "stored-alpha", "live-A");

        core.clone().send("stored-alpha".to_string(), "second text".to_string())
            .await
            .expect("send");

        assert!(
            sink.transcript()
                .iter()
                .any(|c| c.row_json.contains("\"kind\":\"user\"")
                    && c.row_json.contains("second text")),
            "a redirected submit still publishes the user row: {:?}",
            sink.transcript()
        );
    }

    /// Rule pinned (T7c): a FAILED submit (RPC error) publishes NO user row —
    /// the sent text never became part of the conversation.
    #[tokio::test]
    async fn send_publishes_no_user_row_on_a_failed_submit() {
        let dir = temp_dir("send-user-row-error");
        let gw = FakeGw::spawn(|method, _params| async move {
            match method.as_str() {
                "prompt.submit" => Err("gateway down".to_string()),
                _ => Ok(json!({})),
            }
        });
        let (core, sink) = connected_core(&dir, &gw).await;
        attach_session(&core, "stored-alpha", "live-A");

        let result = core.clone().send("stored-alpha".to_string(), "lost".to_string()).await;
        assert!(result.is_err(), "the submit error must surface");
        assert!(
            sink.transcript().is_empty(),
            "no user row after a failed submit: {:?}",
            sink.transcript()
        );
        let rows = {
            let state = core.inner.lock();
            state
                .sessions
                .get("stored-alpha")
                .map(|l| l.reducer.transcript().rows.clone())
                .unwrap_or_default()
        };
        assert!(rows.is_empty(), "the reducer holds no row after a failed submit");
    }

    /// Rule pinned (PR #10 blocking 1): a SUCCESSFUL `prompt.submit` whose
    /// `status` is unknown or missing must surface as an error, not a silent
    /// `Ok(())` — with the app-side echo removed, silence swallows the user's
    /// message (no row anywhere, no error surface). Both halves are pinned on
    /// the WIRE (the fake gateway answers the real RPC): the call fails AND
    /// no row is published (the turn's state is unknown, so no row may be
    /// invented). The parser test in `api.rs` only proves `{}`/`5` parse to
    /// `Other`; it says nothing about what `send` does with it.
    #[tokio::test]
    async fn send_errors_and_publishes_no_row_on_an_unknown_submit_status() {
        for (tag, reply) in [
            ("missing", json!({})),
            ("unknown", json!({"status": "from-the-future"})),
        ] {
            let dir = temp_dir(&format!("send-unknown-status-{tag}"));
            let reply = reply.clone();
            let gw = FakeGw::spawn(move |method, _params| {
                let reply = reply.clone();
                async move {
                    match method.as_str() {
                        "prompt.submit" => Ok(reply),
                        _ => Ok(json!({})),
                    }
                }
            });
            let (core, sink) = connected_core(&dir, &gw).await;
            attach_session(&core, "stored-alpha", "live-A");

            let result = core
                .clone()
                .send("stored-alpha".to_string(), "swallowed".to_string())
                .await;
            assert!(
                matches!(result, Err(CoreError::UnexpectedStatus)),
                "[{tag}] an unknown submit status must surface as UnexpectedStatus, got {result:?}"
            );
            assert!(
                sink.transcript().is_empty(),
                "[{tag}] no row is published for an unknown status: {:?}",
                sink.transcript()
            );
            let rows = {
                let state = core.inner.lock();
                state
                    .sessions
                    .get("stored-alpha")
                    .map(|l| l.reducer.transcript().rows.clone())
                    .unwrap_or_default()
            };
            assert!(
                rows.is_empty(),
                "[{tag}] the reducer holds no row for an unknown status"
            );
        }
    }

    /// Rule pinned (PR #10 blocking 2): a resume rebuild that lands between
    /// the successful `prompt.submit` and the state-lock acquisition must NOT
    /// duplicate the user's row. The window is real: the submit RPC is
    /// awaited OUTSIDE the lock, and `resume_all` re-ingests the session
    /// (truncated replay / changed epoch) over the SAME connection — so the
    /// server-rebuilt history already contains the submitted text, and a
    /// blind `append_user_row` would add it a second time (this layer exists
    /// because of a row duplicate; close the class here).
    ///
    /// The fake gateway's `prompt.submit` handler performs the rebuild —
    /// through the exact primitive `resume_all`'s rebuild branch uses — while
    /// `send` is awaiting that very reply, then answers `streaming`. That is
    /// a true interleaving of the race window (the rebuild strictly between
    /// the submit wire round trip and the post-submit lock acquisition), not
    /// a re-creation of it.
    #[tokio::test]
    async fn send_does_not_duplicate_the_user_row_when_a_rebuild_races_the_submit() {
        let dir = temp_dir("send-rebuild-race");
        // The core does not exist when the gateway is spawned; the handler
        // picks it up from this cell once `connected_core` has built it (the
        // handler only runs for a `prompt.submit`, long after that). Shared
        // through an `Arc`: `OnceLock::clone` would hand the handler an
        // empty COPY of the cell, not the same one.
        let core_cell: Arc<std::sync::OnceLock<Arc<HermesCore>>> =
            Arc::new(std::sync::OnceLock::new());
        let gw = {
            let cell = Arc::clone(&core_cell);
            FakeGw::spawn(move |method, _params| {
                let cell = cell.clone();
                async move {
                    match method.as_str() {
                        "prompt.submit" => {
                            // The interleaving under test: while `send` is
                            // awaiting THIS reply, the resume path rebuilds
                            // the session from the server history — which
                            // already contains the submitted message. The
                            // rebuild goes through the PRODUCTION path
                            // (`rebuild_session`, the helper `resume_all`'s
                            // rebuild branch uses), so the guard is
                            // exercised against the shipping code, not a
                            // re-creation of it.
                            if let Some(core) = cell.get().cloned() {
                                let messages =
                                    vec![json!({"role": "user", "text": "raced text"})];
                                let sink = core.inner.lock().sink.clone();
                                let dtos = core.rebuild_session("stored-alpha", &messages, None);
                                if let Some(sink) = sink {
                                    for dto in dtos {
                                        sink.on_transcript(dto);
                                    }
                                }
                            }
                            Ok(json!({"status": "streaming"}))
                        }
                        _ => Ok(json!({})),
                    }
                }
            })
        };
        let (core, sink) = connected_core(&dir, &gw).await;
        attach_session(&core, "stored-alpha", "live-A");
        core_cell.set(core.clone()).ok().expect("core registered once");
        core.clone()
            .send("stored-alpha".to_string(), "raced text".to_string())
            .await
            .expect("the submit itself succeeded");

        // Exactly ONE user row survives: the rebuild's. The pre-fix code
        // appended a second one after the rebuilt history already carried
        // the message — the duplicate this test exists to kill.
        let rows = {
            let state = core.inner.lock();
            state
                .sessions
                .get("stored-alpha")
                .map(|l| l.reducer.transcript().rows.clone())
                .unwrap_or_default()
        };
        assert_eq!(
            rows.len(),
            1,
            "the raced rebuild must not duplicate the user row: {:?}",
            rows
        );
        let user_deliveries = sink
            .transcript()
            .iter()
            .filter(|c| c.row_json.contains("\"kind\":\"user\""))
            .count();
        assert_eq!(
            user_deliveries, 1,
            "the user text is delivered exactly once across the race: {:?}",
            sink.transcript()
        );
    }
}
