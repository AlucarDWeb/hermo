//! Gateway WebSocket client (framework/driver layer — PLAN.md §4 T2).
//!
//! The only place in the crate where tokio + tokio-tungstenite are used
//! directly. Policy stays out: heartbeat intervals/deadlines are named
//! [`ClientConfig`] fields, overridable by tests; nothing here knows about
//! transcripts, sessions or UI rows.
//!
//! Wire facts (PLAN.md §1.1, verified against Hermes v0.21.1): text frames,
//! newline-delimited JSON-RPC 2.0 both ways; the first inbound frame after
//! accept is always the `gateway.ready` event; the client sends
//! `gateway.ping` every `ping_interval` and closes when no inbound frame
//! arrives within `deadline`.
//!
//! The heartbeat wall clock is `SystemTime`, never `Instant`: on iOS process
//! suspension the tokio clock stops and a 5-minute background pause would
//! look like zero elapsed time.

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde_json::Value;
use thiserror::Error;
use tokio::sync::{broadcast, mpsc, oneshot, watch, Notify};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::frame::{CloseFrame, Utf8Bytes};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use crate::json;
use crate::rpc::frames::{decode, split_lines, Decoded, EventParams, Request};

/// Default heartbeat ping interval (shipped TS client: 15 s).
pub const DEFAULT_PING_INTERVAL: Duration = Duration::from_secs(15);
/// Default inbound-frame deadline (shipped TS client: 45 s).
pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(45);
/// Timeout for `call()` responses.
pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(120);
/// Timeout of the foreground `probe_now()` liveness ping.
pub const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// How long `connect()` waits for `gateway.ready` before failing.
pub const DEFAULT_READY_TIMEOUT: Duration = Duration::from_secs(15);
/// The one token for every ungraceful death: EOF without a WS close frame,
/// a reset without a closing handshake, a failed writer send, or `deadline`
/// of total inbound silence (PLAN §1.1). A negotiated WS Close frame keeps
/// the server's own reason instead, and a local `close(reason)` keeps its
/// reason — the three reason kinds never mix.
pub const UNGRACEFUL_CLOSE_REASON: &str = "heartbeat timeout";

/// Overridable policy numbers for [`GatewayClient`].
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Send `gateway.ping` at this interval. `Duration::ZERO` disables the
    /// heartbeat task entirely.
    pub ping_interval: Duration,
    /// Close with [`UNGRACEFUL_CLOSE_REASON`] when no inbound frame of any
    /// kind arrives for this long (wall clock).
    pub deadline: Duration,
    /// Per-call response timeout.
    pub call_timeout: Duration,
    /// Timeout of the foreground `probe_now()` liveness ping (short on
    /// purpose: a liveness check must never wait out `call_timeout`).
    pub probe_timeout: Duration,
    /// How long `connect()` waits for `gateway.ready`.
    pub ready_timeout: Duration,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            ping_interval: DEFAULT_PING_INTERVAL,
            deadline: DEFAULT_DEADLINE,
            call_timeout: DEFAULT_CALL_TIMEOUT,
            probe_timeout: DEFAULT_PROBE_TIMEOUT,
            ready_timeout: DEFAULT_READY_TIMEOUT,
        }
    }
}

impl ClientConfig {
    /// Shorten every interval to test-friendly values (item 4 of the brief:
    /// never sleep 45 real seconds in a test).
    pub fn for_tests() -> Self {
        Self {
            ping_interval: Duration::from_millis(100),
            deadline: Duration::from_millis(300),
            call_timeout: Duration::from_secs(5),
            probe_timeout: Duration::from_secs(1),
            ready_timeout: Duration::from_secs(2),
        }
    }
}

/// Errors surfaced by the client API.
#[derive(Debug, Clone, Error)]
pub enum ClientError {
    /// The RPC was answered with a JSON-RPC error object.
    #[error("rpc error {code}: {message}")]
    Rpc { code: i64, message: String },
    /// No response within `call_timeout`.
    #[error("rpc timeout after {0:?}")]
    Timeout(Duration),
    /// The connection dropped (or was never established) while a call was in
    /// flight. The string is the sticky close reason.
    #[error("connection closed: {0}")]
    Closed(String),
    /// Transport / protocol failure from the underlying WebSocket.
    #[error("transport error: {0}")]
    Transport(String),
    /// The WebSocket handshake was rejected with an HTTP error status before
    /// any WebSocket framing existed (PLAN §1.1: pre-accept rejections —
    /// the gateway's auth/host/origin guard answers 403, the client never
    /// sees a close frame). Distinct from [`ClientError::Transport`]: this
    /// is a policy answer, not a socket failure, and must not be retried as
    /// if the connection had merely dropped.
    #[error("handshake rejected with http {0}")]
    HandshakeRejected(u16),
}

impl From<tokio_tungstenite::tungstenite::Error> for ClientError {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        // Review #3 carry-over: `connect()` is the only place that sees the
        // HTTP status of a failed upgrade. The handshake status is a policy
        // answer, not a socket failure: it gets its own variant (403 — the
        // pre-accept rejection per PLAN §1.1 — maps to
        // `CoreError::UpgradeRejected` downstream, other statuses to
        // `CoreError::Http`). No sniffing of `Transport(String)` for "403".
        if let tokio_tungstenite::tungstenite::Error::Http(resp) = &e {
            return ClientError::HandshakeRejected(resp.status().as_u16());
        }
        ClientError::Transport(e.to_string())
    }
}

// PR #3 finding 6 (Dependency Rule): the entity (`error.rs`) must not
// import adapter types, so the `From<…> for CoreError` impls live next to
// the types that define them — an impl may live on either side of the
// `From` arrow. Conversion behaviour unchanged from the previous home.
impl From<ClientError> for crate::error::CoreError {
    fn from(e: ClientError) -> Self {
        match e {
            ClientError::Rpc { code, message } => crate::error::CoreError::Rpc { code, message },
            ClientError::Timeout(_) => crate::error::CoreError::Timeout,
            ClientError::Closed(_) => crate::error::CoreError::NotConnected,
            ClientError::Transport(s) => crate::error::CoreError::Network(s),
            // Review #3 carry-over: an HTTP 403 on the WS upgrade maps to
            // the dedicated domain error (never `Network` — the app must
            // not retry an auth/host guard rejection as if it were offline,
            // PR #3 finding 4).
            ClientError::HandshakeRejected(403) => crate::error::CoreError::UpgradeRejected,
            ClientError::HandshakeRejected(code) => crate::error::CoreError::Http(code),
        }
    }
}

/// Connection state, published on a watch channel.
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Idle,
    Connecting,
    Open,
    /// Terminal state. The string is the reason and comes from exactly one
    /// of three kinds, which never mix:
    ///   * a local close reason (we closed on purpose: `close("user logout")`),
    ///   * the server's own close-frame reason (a negotiated WS Close),
    ///   * [`UNGRACEFUL_CLOSE_REASON`] (reset / EOF / dead writer / silence).
    ///
    /// First closer wins: once `Closed`, the reason never changes.
    Closed(String),
}

/// Events delivered to subscribers: the per-frame payload. Unknown event
/// types pass through untouched — the wire protocol is internal and grows;
/// upper layers ignore what they do not know (defensive-parse rule).
#[derive(Debug, Clone, PartialEq)]
pub enum GatewayEvent {
    Event(EventParams),
}

/// Internal message from the API half to the writer task.
enum Outbound {
    /// A JSON-RPC request line.
    Line(String),
    /// A protocol-level close: a deliberate local close (caller or
    /// heartbeat deadline).
    CloseNow(String),
}

/// Shared state between the async facade and the reader task.
struct Shared {
    config: ClientConfig,
    /// pending request id -> response sender. The reader resolves these
    /// without ever blocking on caller code.
    pending: Mutex<HashMap<String, oneshot::Sender<Result<Value, ClientError>>>>,
    /// Wall-clock instant (ms since the Unix epoch) of the last inbound byte.
    last_inbound_wall_ms: AtomicU64,
    /// Events broadcast to every subscriber.
    events_tx: broadcast::Sender<GatewayEvent>,
    /// The subscription `connect()` made BEFORE the reader task started.
    /// Handed to the first `events()` caller, so frames emitted in the
    /// `gateway.ready` era are already buffered in it instead of being
    /// dropped by a zero-receiver broadcast.
    first_events_rx: Mutex<Option<broadcast::Receiver<GatewayEvent>>>,
    /// Kept alive forever so `events_tx.send` never fails with zero
    /// receivers (mirrors `_state_keepalive`).
    _events_keepalive: broadcast::Receiver<GatewayEvent>,
    /// Per-session seq watermark (session-bound events carry `seq`).
    last_seq: Mutex<HashMap<String, i64>>,
    /// Server's replay epoch from `gateway.ready` ("" until seen).
    replay_epoch: Mutex<String>,
    /// `skin` object of the `gateway.ready` payload (stashed at connect so
    /// late subscribers cannot lose it).
    skin: Mutex<Value>,
    /// Watch half of the connection state.
    state_tx: watch::Sender<ConnectionState>,
    /// Receiver kept alive forever so `send` never fails and `state()` can
    /// clone a live subscription (a watch send with zero receivers drops the
    /// update — the Open transition would be lost otherwise).
    _state_keepalive: watch::Receiver<ConnectionState>,
    /// Serializes terminal transitions: first closer wins.
    close_lock: Mutex<()>,
    /// Wakes the heartbeat task out of its ping sleep when the connection
    /// dies by other means (writer send failure, reader EOF).
    heartbeat_wake: Notify,
    /// Handle to the writer task's outbound queue (heartbeat + close).
    outbound_tx: mpsc::UnboundedSender<Outbound>,
}

impl Shared {
    fn now_wall_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Connecting -> Open. Only `connect()` may call this.
    fn mark_open(&self) {
        let _ = self.state_tx.send(ConnectionState::Open);
    }

    /// Terminal transition: first closer wins. Later close attempts (the
    /// reader's bookkeeping, a caller's `close()`, the heartbeat, the
    /// writer) must not overwrite the first reason. Returns false when the
    /// state was already `Closed`.
    fn close_terminal(&self, reason: &str) -> bool {
        {
            let _guard = self.close_lock.lock();
            if matches!(&*self.state_tx.borrow(), ConnectionState::Closed(_)) {
                return false;
            }
            let _ = self.state_tx.send(ConnectionState::Closed(reason.to_string()));
        }
        // Wake the heartbeat so it notices the closed state without waiting
        // out the rest of the ping interval.
        self.heartbeat_wake.notify_one();
        true
    }

    /// Fail every pending call with `err`. Idempotent: drains whatever is
    /// left, whoever gets there first.
    fn fail_pending(&self, err: ClientError) {
        let mut pending = self.pending.lock();
        for (_, tx) in pending.drain() {
            let _ = tx.send(Err(err.clone()));
        }
    }
}

/// The public face of the client. Cloning is cheap (all handles are channels).
#[derive(Clone)]
pub struct GatewayClient {
    shared: Arc<Shared>,
    outbound_tx: mpsc::UnboundedSender<Outbound>,
    next_id: Arc<AtomicU64>,
}

impl GatewayClient {
    /// Connect to `url` and resolve only after `gateway.ready` arrived.
    ///
    /// `url` may be an `http(s)://` base — it is rewritten to `ws(s)://` and
    /// `/api/ws` is appended when no path is present, so the probe can take
    /// the same dashboard base URL the auth client (T3) will use. Any query
    /// string (`?token=…` on the loopback path) is preserved.
    pub async fn connect(url: &str, config: ClientConfig) -> Result<Self, ClientError> {
        let ws_url = to_ws_url(url)?;
        let (stream, _resp) = connect_async(&ws_url).await.map_err(ClientError::from)?;

        let (events_tx, events_keepalive) = broadcast::channel(1024);
        // Subscribe BEFORE the reader starts: this is the receiver handed to
        // the first `events()` caller, so not even `gateway.ready` can race
        // a zero-receiver broadcast.
        let first_events_rx = events_tx.subscribe();
        let (state_tx, state_rx) = watch::channel(ConnectionState::Connecting);
        let (outbound_tx, outbound_rx) = mpsc::unbounded_channel();
        let shared = Arc::new(Shared {
            last_inbound_wall_ms: AtomicU64::new(Shared::now_wall_ms()),
            pending: Mutex::new(HashMap::new()),
            events_tx,
            first_events_rx: Mutex::new(Some(first_events_rx)),
            _events_keepalive: events_keepalive,
            last_seq: Mutex::new(HashMap::new()),
            replay_epoch: Mutex::new(String::new()),
            skin: Mutex::new(Value::Null),
            state_tx,
            _state_keepalive: state_rx,
            close_lock: Mutex::new(()),
            heartbeat_wake: Notify::new(),
            config,
            outbound_tx: outbound_tx.clone(),
        });
        let client = GatewayClient {
            shared: shared.clone(),
            outbound_tx,
            next_id: Arc::new(AtomicU64::new(1)),
        };

        let (sink, stream) = stream.split();
        let writer = spawn_writer(sink, outbound_rx, shared.clone());
        let _reader = spawn_reader(stream, shared.clone(), writer);

        // Wait for gateway.ready (or failure) before resolving, using the
        // subscription made before the reader spawned.
        let mut events = shared
            .first_events_rx
            .lock()
            .take()
            .expect("connect holds the first events subscription");
        let deadline = tokio::time::Instant::now() + shared.config.ready_timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                client.shutdown("gateway.ready not received");
                return Err(ClientError::Closed("gateway.ready not received".into()));
            }
            match tokio::time::timeout(remaining, events.recv()).await {
                Err(_) => {
                    client.shutdown("gateway.ready not received");
                    return Err(ClientError::Closed("gateway.ready not received".into()));
                }
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(_)) => {
                    // Cannot happen while `_events_keepalive` lives, but
                    // degrade instead of hanging.
                    return Err(ClientError::Closed("closed before gateway.ready".into()));
                }
                Ok(Ok(GatewayEvent::Event(ev))) if ev.event_type == "gateway.ready" => {
                    // Stash the ready payload: replay_epoch for the reconnect
                    // bookkeeping and skin for the upper layers (T4) — both
                    // readable without subscribing to events.
                    *shared.replay_epoch.lock() =
                        json::str_at(&ev.payload, "replay_epoch").to_string();
                    *shared.skin.lock() =
                        ev.payload.get("skin").cloned().unwrap_or(Value::Null);
                    shared.mark_open();
                    break;
                }
                Ok(Ok(_)) => continue,
            }
        }

        // Hand the connect-time subscription back for the first `events()`
        // caller: frames emitted after `gateway.ready` but before the caller
        // subscribes stay buffered in it instead of being lost.
        *shared.first_events_rx.lock() = Some(events);

        spawn_heartbeat(shared);
        Ok(client)
    }

    /// Current connection state (watch channel, no polling).
    pub fn state(&self) -> watch::Receiver<ConnectionState> {
        self.shared.state_tx.subscribe()
    }

    /// Subscribe to server events.
    ///
    /// The FIRST caller receives the subscription `connect()` used while
    /// waiting for `gateway.ready` — frames emitted in the ready era are
    /// already buffered in it. Later callers get fresh subscriptions.
    pub fn events(&self) -> broadcast::Receiver<GatewayEvent> {
        self.shared
            .first_events_rx
            .lock()
            .take()
            .unwrap_or_else(|| self.shared.events_tx.subscribe())
    }

    /// Most recent server replay epoch (empty until `gateway.ready`).
    pub fn replay_epoch(&self) -> String {
        self.shared.replay_epoch.lock().clone()
    }

    /// The `skin` object from the `gateway.ready` payload (`Value::Null`
    /// before it arrived). Stashed at connect so a caller that subscribes
    /// late cannot lose it.
    pub fn skin(&self) -> Value {
        self.shared.skin.lock().clone()
    }

    /// Highest seq seen for `session_id` (0 when never seen).
    pub fn last_seq(&self, session_id: &str) -> i64 {
        *self.shared.last_seq.lock().get(session_id).unwrap_or(&0)
    }

    /// Issue one JSON-RPC call, resolving on the response (result or error).
    ///
    /// `call()` never blocks the reader: the request line is queued on an
    /// unbounded channel, the pending id sits in a map the reader consults,
    /// and events keep flowing while the caller awaits.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, ClientError> {
        self.call_with_timeout(method, params, self.shared.config.call_timeout)
            .await
    }

    async fn call_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, ClientError> {
        if let ConnectionState::Closed(reason) = self.shared.state_tx.borrow().clone() {
            return Err(ClientError::Closed(reason));
        }
        let id = format!("r{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        let request = Request {
            jsonrpc: "2.0".into(),
            id: id.clone(),
            method: method.into(),
            params,
        };
        let line = json::to_compact_string(&serde_json::to_value(&request).map_err(|e| {
            ClientError::Transport(format!("serialize request: {e}"))
        })?);

        let (tx, rx) = oneshot::channel();
        self.shared.pending.lock().insert(id.clone(), tx);
        if self.outbound_tx.send(Outbound::Line(line)).is_err() {
            self.shared.pending.lock().remove(&id);
            return Err(ClientError::Closed("writer task gone".into()));
        }

        match tokio::time::timeout(timeout, rx).await {
            Err(_) => {
                // Slow responses leave the client usable: the pending entry
                // is dropped here so a late reply finds no caller and events
                // keep flowing (brief test (d)).
                self.shared.pending.lock().remove(&id);
                Err(ClientError::Timeout(timeout))
            }
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(e))) => Err(e),
            Ok(Err(_recv)) => Err(ClientError::Closed("connection lost".into())),
        }
    }

    /// One immediate `gateway.ping` with its own short timeout — the
    /// foreground "is the connection alive?" probe. It must not wait out the
    /// much longer `call_timeout`.
    pub async fn probe_now(&self) -> Result<(), ClientError> {
        self.call_with_timeout(
            "gateway.ping",
            Value::Null,
            self.shared.config.probe_timeout,
        )
        .await
        .map(|_| ())
    }

    /// Explicitly close with `reason`; idempotent. The FIRST close reason
    /// (here or from the server, or the ungraceful token) sticks.
    pub fn close(&self, reason: &str) {
        self.shutdown(reason);
    }

    fn shutdown(&self, reason: &str) {
        self.shared.close_terminal(reason);
        self.shared
            .fail_pending(ClientError::Closed(reason.to_string()));
        let _ = self.outbound_tx.send(Outbound::CloseNow(reason.to_string()));
    }
}

/// Convert a dashboard base URL (`http://host:port`, `https://…`) into the
/// gateway WS URL (`ws://host:port/api/ws`). Already-ws URLs and URLs with a
/// path pass through (the loopback probe URL may carry `?token=`).
fn to_ws_url(url: &str) -> Result<String, ClientError> {
    if url.starts_with("ws://") || url.starts_with("wss://") {
        return Ok(url.to_string());
    }
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(|| ClientError::Transport(format!("unsupported url scheme: {url}")))?;
    let scheme = if url.starts_with("https://") { "wss" } else { "ws" };
    let (authority, path_and_query) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let path = match path_and_query.find('?') {
        Some(i) => {
            let (p, q) = path_and_query.split_at(i);
            // Empty path on a base URL -> /api/ws; a real path passes through.
            if p.is_empty() {
                format!("/api/ws{q}")
            } else {
                path_and_query.to_string()
            }
        }
        None => {
            if path_and_query.is_empty() {
                "/api/ws".to_string()
            } else {
                path_and_query.to_string()
            }
        }
    };
    Ok(format!("{scheme}://{authority}{path}"))
}

/// The writer task: drains the outbound channel into the WebSocket sink.
/// On a send failure the connection is dying NOW: fail closed with the one
/// ungraceful token, fail all pending RPCs and wake the heartbeat instead of
/// silently swallowing sends. (The `deadline` stays for a half-open socket
/// that still buffers writes; a failed write is a different, immediate
/// death and must not be claimed to surface in any particular number of
/// seconds.)
fn spawn_writer(
    mut sink: futures_util::stream::SplitSink<
        WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
        Message,
    >,
    mut rx: mpsc::UnboundedReceiver<Outbound>,
    shared: Arc<Shared>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let result = match msg {
                Outbound::Line(line) => sink.send(Message::text(Utf8Bytes::from(line))).await,
                Outbound::CloseNow(reason) => {
                    // Best effort: the close frame may not fit on a socket
                    // that is already dead — that is exactly what the
                    // ungraceful token is for, and whoever called for the
                    // close already published its reason.
                    let _ = sink
                        .send(Message::Close(Some(CloseFrame {
                            code: CloseCode::Normal,
                            reason: Utf8Bytes::from(reason),
                        })))
                        .await;
                    break;
                }
            };
            if result.is_err() {
                if shared.close_terminal(UNGRACEFUL_CLOSE_REASON) {
                    shared.fail_pending(ClientError::Closed(
                        UNGRACEFUL_CLOSE_REASON.to_string(),
                    ));
                }
                break;
            }
        }
    })
}

/// The reader task: splits frames, decodes every line, routes results to
/// pending oneshots and events onto the broadcast, tracks seq watermarks.
fn spawn_reader(
    mut stream: futures_util::stream::SplitStream<
        WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    >,
    shared: Arc<Shared>,
    writer: tokio::task::JoinHandle<()>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Some(ref) only when the server negotiated a real WS Close frame;
        // every other way out of the loop is ungraceful.
        let mut server_close_reason: Option<String> = None;
        while let Some(msg) = stream.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    shared
                        .last_inbound_wall_ms
                        .store(Shared::now_wall_ms(), Ordering::Relaxed);
                    for line in split_lines(&text) {
                        match decode(line) {
                            Some(Decoded::Result { id, result }) => {
                                if let Some(tx) = shared.pending.lock().remove(&id) {
                                    let _ = tx.send(Ok(result));
                                }
                            }
                            Some(Decoded::Error { id, error }) => {
                                if let Some(tx) = shared.pending.lock().remove(&id) {
                                    let _ = tx.send(Err(ClientError::Rpc {
                                        code: error.code,
                                        message: error.message,
                                    }));
                                }
                            }
                            Some(Decoded::Event(ev)) => {
                                record_seq(&shared, &ev);
                                let _ = shared.events_tx.send(GatewayEvent::Event(ev));
                            }
                            Some(Decoded::Ignored) | None => {}
                        }
                    }
                }
                Ok(Message::Close(frame)) => {
                    // A negotiated close is the server's word: keep its
                    // reason. A frame without a reason text keeps the code,
                    // so 4401 (ticket invalid) and 4403 (request guard) stay
                    // distinct (PLAN §1.1) even when the server sends no
                    // reason string.
                    server_close_reason = Some(match frame {
                        Some(f) => {
                            let reason = f.reason.to_string();
                            if reason.is_empty() {
                                format!("closed by server (code {})", f.code)
                            } else {
                                reason
                            }
                        }
                        None => "closed by server".into(),
                    });
                    break;
                }
                Ok(Message::Ping(_) | Message::Pong(_)) => {
                    // Transport-level keepalive still counts as "inbound
                    // activity" for the deadline.
                    shared
                        .last_inbound_wall_ms
                        .store(Shared::now_wall_ms(), Ordering::Relaxed);
                }
                Ok(Message::Binary(_) | Message::Frame(_)) => {
                    shared
                        .last_inbound_wall_ms
                        .store(Shared::now_wall_ms(), Ordering::Relaxed);
                }
                Err(_e) => {
                    // Any transport death without a closing handshake —
                    // reset, EOF-style ConnectionClosed, protocol or utf8
                    // errors — folds into the ONE ungraceful token, the same
                    // the silence deadline uses (PLAN §1.1). Details of the
                    // internal wire errors are deliberately not surfaced as
                    // a close reason.
                    break;
                }
            }
        }
        // Terminal path: `stream.next()` -> None is an EOF without a WS
        // close frame; together with the transport-error break above it is
        // ungraceful and gets [`UNGRACEFUL_CLOSE_REASON`]. A real Close
        // frame keeps the server reason. Either way the FIRST closer wins:
        // an earlier `close("…")` or heartbeat timeout is never overwritten.
        let reason = server_close_reason
            .unwrap_or_else(|| UNGRACEFUL_CLOSE_REASON.to_string());
        shared.close_terminal(&reason);
        shared.fail_pending(ClientError::Closed(reason));
        writer.abort();
    })
}

/// Update the seq watermark for a session-bound event.
fn record_seq(shared: &Arc<Shared>, ev: &EventParams) {
    if let Some(seq) = ev.seq {
        if !ev.session_id.is_empty() {
            let mut map = shared.last_seq.lock();
            let slot = map.entry(ev.session_id.clone()).or_insert(0);
            if seq > *slot {
                *slot = seq;
            }
        }
    }
}

/// The heartbeat task: pings every `ping_interval` and closes the connection
/// with the ungraceful token when no inbound frame of any kind arrived for
/// `deadline` (wall clock). Woken early when the connection dies otherwise.
fn spawn_heartbeat(shared: Arc<Shared>) {
    tokio::spawn(async move {
        let mut seq: u64 = 0;
        let interval = shared.config.ping_interval;
        if interval.is_zero() {
            return; // heartbeat disabled
        }
        loop {
            tokio::select! {
                _ = tokio::time::sleep(interval) => {}
                _ = shared.heartbeat_wake.notified() => {}
            }
            if let ConnectionState::Closed(_) = shared.state_tx.borrow().clone() {
                return;
            }
            let last = shared.last_inbound_wall_ms.load(Ordering::Relaxed);
            let now = Shared::now_wall_ms();
            if now.saturating_sub(last) >= shared.config.deadline.as_millis() as u64 {
                // The one ungraceful token, sticky (first closer wins). No
                // synthetic `gateway.closed` event is emitted: the state
                // watch is the contract (PLAN §1.4).
                shared.close_terminal(UNGRACEFUL_CLOSE_REASON);
                shared.fail_pending(ClientError::Closed(
                    UNGRACEFUL_CLOSE_REASON.to_string(),
                ));
                let _ = shared
                    .outbound_tx
                    .send(Outbound::CloseNow(UNGRACEFUL_CLOSE_REASON.into()));
                return;
            }
            seq += 1;
            let request = Request {
                jsonrpc: "2.0".into(),
                id: format!("heartbeat-{seq}"),
                method: "gateway.ping".into(),
                params: Value::Null,
            };
            if let Ok(line) = serde_json::to_string(&request) {
                let _ = shared.outbound_tx.send(Outbound::Line(line));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoreError;

    /// PR #3 finding 6: this test moved here from `error.rs` together with
    /// the `From<ClientError>` impl it pins (the entity module must not
    /// import adapter types). Behaviour unchanged: every adapter error the
    /// app can see keeps its meaning at the domain boundary — a timeout is
    /// not a network failure, a closed socket is not a transport reset, and
    /// an RPC error keeps its code.
    #[test]
    fn client_error_maps_losslessly_where_it_matters() {
        let timeout: CoreError = ClientError::Timeout(Duration::from_secs(1)).into();
        assert!(matches!(timeout, CoreError::Timeout));
        let closed: CoreError = ClientError::Closed("heartbeat timeout".into()).into();
        assert!(matches!(closed, CoreError::NotConnected));
        let transport: CoreError = ClientError::Transport("reset".into()).into();
        assert!(matches!(transport, CoreError::Network(_)));
        let rpc: CoreError = ClientError::Rpc {
            code: 5000,
            message: "boom".into(),
        }
        .into();
        assert!(matches!(rpc, CoreError::Rpc { code: 5000, .. }));
    }

    /// Review #3 carry-over: a failed WS upgrade answers HTTP 403 — the
    /// pre-accept rejection (PLAN §1.1) — and that must surface as the
    /// dedicated domain error, NOT `Network` (the app must not retry an
    /// auth/host guard as if it were offline), while every other transport
    /// failure still maps to `Network`. The mapping lives in
    /// `From<tungstenite::Error>`: a real `Error::Http(403)` produces
    /// `HandshakeRejected(403)`, and the domain mapping turns it into
    /// `UpgradeRejected`.
    #[test]
    fn handshake_403_maps_to_upgrade_rejected_and_reset_stays_network() {
        use tokio_tungstenite::tungstenite::http::{Response, StatusCode};

        // Build the exact wire error a 403 upgrade rejection produces.
        let resp = Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(None)
            .expect("static response");
        let wire: ClientError = tokio_tungstenite::tungstenite::Error::Http(resp).into();
        assert!(
            matches!(wire, ClientError::HandshakeRejected(403)),
            "the producer must classify the 403 handshake, got {wire:?}"
        );
        let mapped: CoreError = wire.into();
        assert!(
            matches!(mapped, CoreError::UpgradeRejected),
            "403 must map to UpgradeRejected, got {mapped:?}"
        );

        // A plain transport reset (no HTTP status at all) is NOT a rejection.
        let reset: CoreError =
            ClientError::from(tokio_tungstenite::tungstenite::Error::Io(
                std::io::Error::new(std::io::ErrorKind::ConnectionReset, "reset"),
            ))
            .into();
        assert!(
            matches!(reset, CoreError::Network(_)),
            "a transport reset must stay Network, got {reset:?}"
        );

        // Another HTTP status is a distinct domain error, not UpgradeRejected.
        let resp503 = Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .body(None)
            .expect("static response");
        let mapped503: CoreError =
            ClientError::from(tokio_tungstenite::tungstenite::Error::Http(resp503)).into();
        assert!(
            matches!(mapped503, CoreError::Http(503)),
            "503 must map to Http(503), got {mapped503:?}"
        );
    }
}
