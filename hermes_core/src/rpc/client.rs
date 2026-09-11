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
//! `gateway.ping` every `ping_interval` while any inbound frame within
//! `deadline` keeps the connection alive.
//!
//! Heartbeat uses **wall-clock** time (`SystemTime`), not `Instant`: on iOS
//! process suspension the tokio clock stops and a 5-minute background would
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
use tokio::sync::{broadcast, mpsc, oneshot, watch};
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
/// Timeout for `probe_now()` and for waiting on `gateway.ready` at connect.
const READY_TIMEOUT: Duration = Duration::from_secs(15);

/// Overridable policy numbers for [`GatewayClient`].
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Send `gateway.ping` at this interval. `Duration::ZERO` disables the
    /// heartbeat task entirely.
    pub ping_interval: Duration,
    /// Close with `heartbeat timeout` when no inbound frame of any kind
    /// arrives for this long (wall clock).
    pub deadline: Duration,
    /// Per-call response timeout.
    pub call_timeout: Duration,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            ping_interval: DEFAULT_PING_INTERVAL,
            deadline: DEFAULT_DEADLINE,
            call_timeout: DEFAULT_CALL_TIMEOUT,
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
    /// flight.
    #[error("connection closed: {0}")]
    Closed(String),
    /// Transport / protocol failure from the underlying WebSocket.
    #[error("transport error: {0}")]
    Transport(String),
    /// A call was attempted while the client is not open (including during
    /// the `gateway.ready` handshake).
    #[error("not connected")]
    NotConnected,
}

impl From<tokio_tungstenite::tungstenite::Error> for ClientError {
    fn from(e: tokio_tungstenite::tungstenite::Error) -> Self {
        ClientError::Transport(e.to_string())
    }
}

/// Connection state, published on a watch channel.
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Idle,
    Connecting,
    Open,
    /// Terminal state; the string is the reason ("heartbeat timeout", a close
    /// frame reason, or a transport error).
    Closed(String),
}

/// Events delivered to subscribers. `Watermark` keeps the seq/replay_epoch
/// bookkeeping visible to the upper layers (T4/T5); `Event` is the per-frame
/// payload.
#[derive(Debug, Clone, PartialEq)]
pub enum GatewayEvent {
    Event(EventParams),
    /// The server told us its replay epoch (from `gateway.ready` payload) and
    /// the seq we believe the session to be at.
    Watermark {
        replay_epoch: String,
        last_seq: i64,
    },
}

/// Internal message from the API half to the writer task.
enum Outbound {
    /// A JSON-RPC request line.
    Line(String),
    /// A protocol-level close: the heartbeat found the connection dead.
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
    /// Per-session seq watermark (session-bound events carry `seq`).
    last_seq: Mutex<HashMap<String, i64>>,
    /// Server's replay epoch from `gateway.ready` ("" until seen).
    replay_epoch: Mutex<String>,
    /// Watch half of the connection state.
    state_tx: watch::Sender<ConnectionState>,
    /// Receiver kept alive forever so `send` never fails and `state()` can
    /// clone a live subscription (a watch send with zero receivers drops the
    /// update — the Open transition would be lost otherwise).
    _state_keepalive: watch::Receiver<ConnectionState>,
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

    fn set_state(&self, state: ConnectionState) {
        let _ = self.state_tx.send(state);
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

        let (events_tx, _) = broadcast::channel(1024);
        let (state_tx, state_rx) = watch::channel(ConnectionState::Connecting);
        let (outbound_tx, outbound_rx) = mpsc::unbounded_channel();
        let shared = Arc::new(Shared {
            last_inbound_wall_ms: AtomicU64::new(Shared::now_wall_ms()),
            pending: Mutex::new(HashMap::new()),
            events_tx,
            last_seq: Mutex::new(HashMap::new()),
            replay_epoch: Mutex::new(String::new()),
            state_tx,
            _state_keepalive: state_rx,
            config,
            outbound_tx: outbound_tx.clone(),
        });
        let client = Self {
            shared: shared.clone(),
            outbound_tx,
            next_id: Arc::new(AtomicU64::new(1)),
        };

        let (sink, stream) = stream.split();
        let writer = spawn_writer(sink, outbound_rx);
        let _reader = spawn_reader(stream, shared.clone(), writer);

        // Wait for gateway.ready (or failure) before resolving.
        let mut events = shared.events_tx.subscribe();
        let deadline = tokio::time::Instant::now() + READY_TIMEOUT;
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
                    let reason = "closed before gateway.ready".to_string();
                    return Err(ClientError::Closed(reason));
                }
                Ok(Ok(GatewayEvent::Event(ev))) if ev.event_type == "gateway.ready" => {
                    // replay_epoch lives in the ready payload.
                    let epoch = json::str_at(&ev.payload, "replay_epoch").to_string();
                    *shared.replay_epoch.lock() = epoch;
                    shared.set_state(ConnectionState::Open);
                    break;
                }
                Ok(Ok(_)) => continue,
            }
        }

        spawn_heartbeat(shared);
        Ok(client)
    }

    /// Current connection state (watch channel, no polling).
    pub fn state(&self) -> watch::Receiver<ConnectionState> {
        self.shared.state_tx.subscribe()
    }

    /// Subscribe to server events and watermark updates.
    pub fn events(&self) -> broadcast::Receiver<GatewayEvent> {
        self.shared.events_tx.subscribe()
    }

    /// Most recent server replay epoch (empty until `gateway.ready`).
    pub fn replay_epoch(&self) -> String {
        self.shared.replay_epoch.lock().clone()
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

        match tokio::time::timeout(self.shared.config.call_timeout, rx).await {
            Err(_) => {
                // Slow responses leave the client usable: the pending entry
                // is dropped here so a late reply finds no caller and events
                // keep flowing (brief test (d)).
                self.shared.pending.lock().remove(&id);
                Err(ClientError::Timeout(self.shared.config.call_timeout))
            }
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(e))) => Err(e),
            Ok(Err(_recv)) => Err(ClientError::Closed("connection lost".into())),
        }
    }

    /// One immediate `gateway.ping` with a short timeout — the foreground
    /// "is the connection alive?" probe.
    pub async fn probe_now(&self) -> Result<(), ClientError> {
        self.call(
            "gateway.ping",
            serde_json::Value::Null,
        )
        .await
        .map(|_| ())
    }

    /// Explicitly close with `reason`; idempotent.
    pub fn close(&self, reason: &str) {
        self.shutdown(reason);
    }

    fn shutdown(&self, reason: &str) {
        self.shared.set_state(ConnectionState::Closed(reason.to_string()));
        // Fail every pending call immediately; the writer gets a close too.
        let mut pending = self.shared.pending.lock();
        for (_, tx) in pending.drain() {
            let _ = tx.send(Err(ClientError::Closed(reason.to_string())));
        }
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
        .ok_or_else(|| {
            ClientError::Transport(format!("unsupported url scheme: {url}"))
        })?;
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
/// Returns a handle the reader can drop to end the writer.
fn spawn_writer(
    mut sink: futures_util::stream::SplitSink<
        WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
        Message,
    >,
    mut rx: mpsc::UnboundedReceiver<Outbound>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let result = match msg {
                Outbound::Line(line) => sink.send(Message::text(Utf8Bytes::from(line))).await,
                Outbound::CloseNow(reason) => {
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
                break;
            }
        }
    })
}

/// The reader task: splits frames, decodes every line, routes results to
/// pending oneshots and events to the broadcast, tracks seq watermarks.
fn spawn_reader(
    mut stream: futures_util::stream::SplitStream<
        WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    >,
    shared: Arc<Shared>,
    writer: tokio::task::JoinHandle<()>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut closed_reason: Option<String> = None;
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
                    closed_reason = Some(
                        frame
                            .map(|f| f.reason.to_string())
                            .unwrap_or_else(|| "closed by server".into()),
                    );
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
                Err(e) => {
                    // A reset WITHOUT a closing handshake is a silently dead
                    // socket, not a negotiated close: fold it into the
                    // heartbeat-timeout path (the deadline decides the
                    // wording). A real Close frame keeps the server's reason.
                    closed_reason = Some(match e {
                        tokio_tungstenite::tungstenite::Error::ConnectionClosed
                        | tokio_tungstenite::tungstenite::Error::Protocol(
                            tokio_tungstenite::tungstenite::error::ProtocolError::ResetWithoutClosingHandshake,
                        ) => String::new(),
                        other => other.to_string(),
                    });
                    break;
                }
            }
        }
        // Terminal path: fail pending calls, publish the state, end writer.
        //
        // The heartbeat task is the authority for the *silent* dead-socket
        // case (PLAN §1.1: "no inbound frame of any kind for 45 s → drop the
        // socket"): if the deadline elapsed, or the socket died without a
        // closing handshake (reset / hard drop, `closed_reason` emptied in
        // the reader), report `heartbeat timeout`. A reason arriving BEFORE
        // the deadline with a real Close frame is a server-initiated close
        // and wins.
        let deadline_elapsed = {
            let last = shared.last_inbound_wall_ms.load(Ordering::Relaxed);
            Shared::now_wall_ms().saturating_sub(last)
                >= shared.config.deadline.as_millis() as u64
        };
        let heartbeat_dead = deadline_elapsed
            || closed_reason
                .as_ref()
                .map(String::is_empty)
                .unwrap_or(false);
        let reason = if heartbeat_dead {
            "heartbeat timeout".to_string()
        } else {
            closed_reason.unwrap_or_else(|| "reader ended".into())
        };
        shared.set_state(ConnectionState::Closed(reason.clone()));
        let mut pending = shared.pending.lock();
        for (_, tx) in pending.drain() {
            let _ = tx.send(Err(ClientError::Closed(reason.clone())));
        }
        drop(pending);
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
/// when no inbound frame of any kind arrived for `deadline` (wall clock).
fn spawn_heartbeat(shared: Arc<Shared>) {
    tokio::spawn(async move {
        let mut seq: u64 = 0;
        let interval = shared.config.ping_interval;
        if interval.is_zero() {
            return; // heartbeat disabled
        }
        loop {
            tokio::time::sleep(interval).await;
            if let ConnectionState::Closed(_) = shared.state_tx.borrow().clone() {
                return;
            }
            let last = shared.last_inbound_wall_ms.load(Ordering::Relaxed);
            let now = Shared::now_wall_ms();
            if now.saturating_sub(last) >= shared.config.deadline.as_millis() as u64 {
                shared.set_state(ConnectionState::Closed("heartbeat timeout".into()));
                let mut pending = shared.pending.lock();
                for (_, tx) in pending.drain() {
                    let _ = tx.send(Err(ClientError::Closed("heartbeat timeout".into())));
                }
                drop(pending);
                let _ = shared
                    .events_tx
                    .send(GatewayEvent::Event(EventParams {
                        event_type: "gateway.closed".into(),
                        session_id: String::new(),
                        seq: None,
                        payload: serde_json::json!({"reason": "heartbeat timeout"}),
                    }));
                let _ = shared
                    .outbound_tx
                    .send(Outbound::CloseNow("heartbeat timeout".into()));
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
