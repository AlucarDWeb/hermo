//! Reply shapes here are the ones `hermes_core` actually parses; the recorded
//! frames in `fixtures` carry the rest.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::handshake::server::{
    ErrorResponse, Request as HandshakeRequest, Response as HandshakeResponse,
};
use tokio_tungstenite::tungstenite::http;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

pub struct Config {
    pub port: u16,
    pub user: String,
    pub password: String,
    pub fixture: PathBuf,
    pub synthetic: Option<PathBuf>,
    pub drop_after: Option<usize>,
}

pub struct Running {
    pub addr: SocketAddr,
    pub base_url: String,
    pub pairing_payload: String,
    accept_task: JoinHandle<()>,
}

impl Running {
    pub fn shutdown(self) {
        self.accept_task.abort();
    }
}

pub async fn start(config: Config) -> std::io::Result<Running> {
    let listener = TcpListener::bind(("127.0.0.1", config.port)).await?;
    let addr = listener.local_addr()?;
    let loaded = match &config.synthetic {
        Some(synthetic) => super::fixtures::Fixtures::load_with_synthetic(&config.fixture, synthetic),
        None => super::fixtures::Fixtures::load(&config.fixture),
    }?;
    let base_url = format!("http://{addr}");
    let payload = pairing_payload(&base_url, &config.user, "fake");
    let state = Arc::new(GatewayState::new(config, loaded));
    let accept_task = tokio::spawn(accept_loop(listener, state));
    Ok(Running {
        addr,
        base_url,
        pairing_payload: payload,
        accept_task,
    })
}

const PAIRING_VALUE: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

pub fn pairing_payload(base_url: &str, user: &str, name: &str) -> String {
    let url = percent_encoding::utf8_percent_encode(base_url, PAIRING_VALUE);
    let user = percent_encoding::utf8_percent_encode(user, PAIRING_VALUE);
    let name = percent_encoding::utf8_percent_encode(name, PAIRING_VALUE);
    format!("hermes://connect?v=1&url={url}&user={user}&name={name}")
}

struct ReplayedEventRecord {
    seq: i64,
    event_type: String,
    payload: Value,
}

struct SessionRecord {
    id: String,
    title: String,
    started_at: String,
    order: i64,
    running: bool,
    seq: i64,
    events: Vec<ReplayedEventRecord>,
    turn_handle: Option<JoinHandle<()>>,
}

impl SessionRecord {
    fn new(id: String, title: String, started_at: String, order: i64) -> Self {
        Self {
            id,
            title,
            started_at,
            order,
            running: false,
            seq: 0,
            events: Vec::new(),
            turn_handle: None,
        }
    }
}

struct GatewayState {
    config: Config,
    fixtures: super::fixtures::Fixtures,
    sessions: Mutex<HashMap<String, SessionRecord>>,
    next_session_id: AtomicU64,
    next_order: AtomicI64,
    tickets: Mutex<HashMap<String, bool>>,
    next_ticket: AtomicU64,
}

impl GatewayState {
    fn new(config: Config, fixtures: super::fixtures::Fixtures) -> Self {
        Self {
            config,
            fixtures,
            sessions: Mutex::new(HashMap::new()),
            next_session_id: AtomicU64::new(1),
            next_order: AtomicI64::new(0),
            tickets: Mutex::new(HashMap::new()),
            next_ticket: AtomicU64::new(1),
        }
    }

    fn mint_ticket(&self) -> String {
        let n = self.next_ticket.fetch_add(1, Ordering::SeqCst);
        let ticket = format!("fake-ticket-{n}");
        self.tickets.lock().insert(ticket.clone(), true);
        ticket
    }

    fn take_ticket(&self, ticket: &str) -> bool {
        match self.tickets.lock().get_mut(ticket) {
            Some(unspent) if *unspent => {
                *unspent = false;
                true
            }
            _ => false,
        }
    }

    /// `None` once the session is gone, so a turn still in flight against a
    /// closed session cannot resurrect its record.
    fn next_seq_and_record(
        &self,
        session_id: &str,
        event_type: &str,
        payload: &Value,
    ) -> Option<i64> {
        let mut sessions = self.sessions.lock();
        let record = sessions.get_mut(session_id)?;
        record.seq += 1;
        let seq = record.seq;
        record.events.push(ReplayedEventRecord {
            seq,
            event_type: event_type.to_string(),
            payload: payload.clone(),
        });
        Some(seq)
    }
}

fn now_marker() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string()
}

async fn accept_loop(listener: TcpListener, state: Arc<GatewayState>) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let state = state.clone();
                tokio::spawn(async move {
                    handle_connection(stream, state).await;
                });
            }
            Err(e) => eprintln!("fake gateway: accept failed: {e}"),
        }
    }
}

async fn handle_connection(stream: TcpStream, state: Arc<GatewayState>) {
    let head = match peek_request_head(&stream).await {
        Ok(head) => head,
        Err(e) => {
            eprintln!("fake gateway: could not read a request head: {e}");
            return;
        }
    };
    if is_websocket_upgrade(&head) {
        accept_ws(stream, state).await;
    } else if let Err(e) = serve_http(stream, &state).await {
        eprintln!("fake gateway: http request failed: {e}");
    }
}

async fn peek_request_head(stream: &TcpStream) -> std::io::Result<String> {
    let mut buf = vec![0u8; 4096];
    for _ in 0..100 {
        let n = stream.peek(&mut buf).await?;
        if let Some(pos) = find_header_end(&buf[..n]) {
            return Ok(String::from_utf8_lossy(&buf[..pos]).into_owned());
        }
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "connection closed before a request head",
            ));
        }
        if n == buf.len() {
            buf.resize(buf.len() * 2, 0);
        } else {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "no request head arrived"))
}

fn find_header_end(data: &[u8]) -> Option<usize> {
    data.windows(4).position(|w| w == b"\r\n\r\n")
}

fn find_header_value(head: &str, name: &str) -> Option<String> {
    head.split("\r\n").find_map(|line| {
        let (k, v) = line.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_string())
    })
}

fn is_websocket_upgrade(head: &str) -> bool {
    find_header_value(head, "upgrade")
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
}

async fn accept_ws(stream: TcpStream, state: Arc<GatewayState>) {
    let cb_state = state.clone();
    let callback = move |request: &HandshakeRequest,
                          response: HandshakeResponse|
          -> Result<HandshakeResponse, ErrorResponse> {
        let accepted = request
            .uri()
            .query()
            .and_then(ticket_from_query)
            .is_some_and(|t| cb_state.take_ticket(&t));
        if accepted {
            return Ok(response);
        }
        let body = json!({"detail": "unknown or spent ticket"}).to_string();
        let rejection = http::Response::builder()
            .status(401)
            .header("Content-Type", "application/json")
            .body(Some(body))
            .expect("a static 401 response is well-formed");
        Err(rejection)
    };
    match tokio_tungstenite::accept_hdr_async(stream, callback).await {
        Ok(ws) => serve_ws(ws, state).await,
        Err(e) => eprintln!("fake gateway: websocket handshake rejected: {e}"),
    }
}

fn ticket_from_query(query: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "ticket")
            .then(|| percent_encoding::percent_decode_str(value).decode_utf8_lossy().into_owned())
    })
}

async fn serve_ws(ws: WebSocketStream<TcpStream>, state: Arc<GatewayState>) {
    let (mut sink, mut stream) = ws.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let mut sent: usize = 0;

    let ready_frame = json!({
        "jsonrpc": "2.0",
        "method": "event",
        "params": {"type": "gateway.ready", "payload": state.fixtures.ready_payload()},
    });
    if sink.send(Message::text(ready_frame.to_string())).await.is_err() {
        return;
    }
    sent += 1;
    if state.config.drop_after.is_some_and(|n| sent >= n) {
        return;
    }

    loop {
        tokio::select! {
            outgoing = rx.recv() => {
                let Some(line) = outgoing else { return };
                if sink.send(Message::text(line)).await.is_err() {
                    return;
                }
                sent += 1;
                if state.config.drop_after.is_some_and(|n| sent >= n) {
                    return;
                }
            }
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        for line in text.split('\n') {
                            let line = line.trim();
                            if !line.is_empty() {
                                handle_rpc_line(line, &state, &tx);
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) => return,
                    Some(Ok(_)) => {}
                    None | Some(Err(_)) => return,
                }
            }
        }
    }
}

fn handle_rpc_line(line: &str, state: &Arc<GatewayState>, out: &mpsc::UnboundedSender<String>) {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return;
    };
    let id = value.get("id").cloned().unwrap_or(Value::Null);
    let method = value.get("method").and_then(Value::as_str).unwrap_or("");
    let params = value.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "gateway.ping" => reply_ok(out, id, json!({"ok": true})),
        "session.create" => handle_session_create(state, out, id, &params),
        "session.list" => handle_session_list(state, out, id, &params),
        "session.most_recent" => handle_session_most_recent(state, out, id),
        "session.resume" => handle_session_resume(state, out, id, &params),
        "session.events.since" => handle_events_since(state, out, id, &params),
        "session.close" => handle_session_close(state, out, id, &params),
        "session.interrupt" => handle_session_interrupt(state, out, id, &params),
        "prompt.submit" => handle_prompt_submit(state, out, id, &params),
        "slash.exec" => handle_slash_exec(out, id, &params),
        "command.dispatch" => handle_command_dispatch(out, id, &params),
        "complete.slash" => reply_ok(out, id, slash_completions()),
        "approval.respond" => reply_ok(out, id, json!({"ok": true})),
        "clarify.respond" => reply_ok(out, id, json!({"status": "ok", "remaining": []})),
        _ => reply_err(out, id, -32601, "method not found"),
    }
}

fn reply_ok(out: &mpsc::UnboundedSender<String>, id: Value, result: Value) {
    let _ = out.send(json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string());
}

fn reply_err(out: &mpsc::UnboundedSender<String>, id: Value, code: i64, message: &str) {
    let _ = out.send(
        json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}).to_string(),
    );
}

fn session_entry_json(record: &SessionRecord) -> Value {
    json!({
        "id": record.id,
        "resolved_id": record.id,
        "title": record.title,
        "preview": "",
        "started_at": record.started_at,
        "message_count": 0,
        "source": "fake",
    })
}

fn handle_session_create(
    state: &Arc<GatewayState>,
    out: &mpsc::UnboundedSender<String>,
    id: Value,
    params: &Value,
) {
    let profile = params.get("profile").and_then(Value::as_str).unwrap_or("").to_string();
    let title = params.get("title").and_then(Value::as_str).unwrap_or("").to_string();
    let n = state.next_session_id.fetch_add(1, Ordering::SeqCst);
    let session_id = format!("fake-session-{n}");
    let order = state.next_order.fetch_add(1, Ordering::SeqCst);
    state.sessions.lock().insert(
        session_id.clone(),
        SessionRecord::new(session_id.clone(), title, now_marker(), order),
    );
    reply_ok(
        out,
        id,
        json!({
            "session_id": session_id,
            "stored_session_id": session_id,
            "message_count": 0,
            "info": {"profile_name": profile},
        }),
    );
}

fn handle_session_list(
    state: &Arc<GatewayState>,
    out: &mpsc::UnboundedSender<String>,
    id: Value,
    params: &Value,
) {
    let limit = params
        .get("limit")
        .and_then(Value::as_i64)
        .filter(|n| *n >= 0)
        .unwrap_or(20) as usize;
    let sessions = state.sessions.lock();
    let mut entries: Vec<&SessionRecord> = sessions.values().collect();
    entries.sort_by_key(|s| std::cmp::Reverse(s.order));
    let items: Vec<Value> = entries.into_iter().take(limit).map(session_entry_json).collect();
    reply_ok(out, id, json!({"sessions": items}));
}

fn handle_session_most_recent(state: &Arc<GatewayState>, out: &mpsc::UnboundedSender<String>, id: Value) {
    let sessions = state.sessions.lock();
    let result = sessions
        .values()
        .max_by_key(|s| s.order)
        .map(session_entry_json)
        .unwrap_or_else(|| json!({}));
    reply_ok(out, id, result);
}

fn handle_session_resume(
    state: &Arc<GatewayState>,
    out: &mpsc::UnboundedSender<String>,
    id: Value,
    params: &Value,
) {
    let session_id = params.get("session_id").and_then(Value::as_str).unwrap_or("").to_string();
    let mut sessions = state.sessions.lock();
    let resumed = sessions.contains_key(&session_id);
    let running = {
        let record = sessions.entry(session_id.clone()).or_insert_with(|| {
            SessionRecord::new(
                session_id.clone(),
                String::new(),
                now_marker(),
                state.next_order.fetch_add(1, Ordering::SeqCst),
            )
        });
        record.running
    };
    reply_ok(
        out,
        id,
        json!({
            "session_id": session_id,
            "resumed": resumed,
            "message_count": 0,
            "messages": [],
            "running": running,
            "inflight": false,
            "info": {"profile_name": ""},
        }),
    );
}

fn handle_events_since(
    state: &Arc<GatewayState>,
    out: &mpsc::UnboundedSender<String>,
    id: Value,
    params: &Value,
) {
    let session_id = params.get("session_id").and_then(Value::as_str).unwrap_or("");
    let last_seen = params.get("last_seen").and_then(Value::as_i64).unwrap_or(0);
    let sessions = state.sessions.lock();
    // A session the fake has never heard of (it was closed, or the process was
    // restarted) is a gap this reply cannot fill, so say so rather than claim
    // the client is caught up.
    let (events, latest_seq, truncated) = match sessions.get(session_id) {
        Some(record) => {
            let events: Vec<Value> = record
                .events
                .iter()
                .filter(|e| e.seq > last_seen)
                .map(|e| {
                    json!({
                        "type": e.event_type,
                        "session_id": session_id,
                        "seq": e.seq,
                        "payload": e.payload,
                    })
                })
                .collect();
            (events, record.seq, false)
        }
        None => (Vec::new(), last_seen, true),
    };
    let epoch = state
        .fixtures
        .ready_payload()
        .get("replay_epoch")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    reply_ok(
        out,
        id,
        json!({
            "events": events,
            "latest_seq": latest_seq,
            "truncated": truncated,
            "epoch": epoch,
        }),
    );
}

fn handle_session_close(
    state: &Arc<GatewayState>,
    out: &mpsc::UnboundedSender<String>,
    id: Value,
    params: &Value,
) {
    let session_id = params.get("session_id").and_then(Value::as_str).unwrap_or("");
    if let Some(mut record) = state.sessions.lock().remove(session_id) {
        if let Some(handle) = record.turn_handle.take() {
            handle.abort();
        }
    }
    reply_ok(out, id, json!({"ok": true}));
}

fn handle_session_interrupt(
    state: &Arc<GatewayState>,
    out: &mpsc::UnboundedSender<String>,
    id: Value,
    params: &Value,
) {
    let session_id = params.get("session_id").and_then(Value::as_str).unwrap_or("").to_string();
    let interrupted_frame = {
        let mut sessions = state.sessions.lock();
        match sessions.get_mut(&session_id) {
            Some(record) if record.running => {
                if let Some(handle) = record.turn_handle.take() {
                    handle.abort();
                }
                record.running = false;
                record.seq += 1;
                let seq = record.seq;
                let payload = json!({"status": "interrupted"});
                record.events.push(ReplayedEventRecord {
                    seq,
                    event_type: "message.complete".to_string(),
                    payload: payload.clone(),
                });
                Some(json!({
                    "jsonrpc": "2.0",
                    "method": "event",
                    "params": {
                        "type": "message.complete",
                        "session_id": session_id,
                        "seq": seq,
                        "payload": payload,
                    },
                }))
            }
            _ => None,
        }
    };
    if let Some(frame) = interrupted_frame {
        let _ = out.send(frame.to_string());
    }
    reply_ok(out, id, json!({"ok": true}));
}

fn handle_prompt_submit(
    state: &Arc<GatewayState>,
    out: &mpsc::UnboundedSender<String>,
    id: Value,
    params: &Value,
) {
    let session_id = params.get("session_id").and_then(Value::as_str).unwrap_or("").to_string();
    let text = params.get("text").and_then(Value::as_str).unwrap_or("").to_string();

    let already_running = {
        let mut sessions = state.sessions.lock();
        let record = sessions.entry(session_id.clone()).or_insert_with(|| {
            SessionRecord::new(
                session_id.clone(),
                String::new(),
                now_marker(),
                state.next_order.fetch_add(1, Ordering::SeqCst),
            )
        });
        if record.running {
            true
        } else {
            record.running = true;
            false
        }
    };

    if already_running {
        reply_ok(out, id, json!({"status": "redirected"}));
        return;
    }
    reply_ok(out, id, json!({"status": "streaming"}));

    let kind = super::fixtures::select(&text);
    let frames = state.fixtures.turn(kind);
    let state_bg = state.clone();
    let out_bg = out.clone();
    let sid_bg = session_id.clone();
    let handle = tokio::spawn(async move {
        run_turn(state_bg, sid_bg, kind, frames, out_bg).await;
    });
    if let Some(record) = state.sessions.lock().get_mut(&session_id) {
        record.turn_handle = Some(handle);
    }
}

async fn run_turn(
    state: Arc<GatewayState>,
    session_id: String,
    kind: super::fixtures::TurnKind,
    frames: Vec<Value>,
    out: mpsc::UnboundedSender<String>,
) {
    let per_frame_ms: u64 = if matches!(kind, super::fixtures::TurnKind::Slow) {
        let count = frames.len().max(1) as u64;
        (20_000 / count).max(1)
    } else {
        33
    };
    for frame in &frames {
        let event_type = frame
            .get("params")
            .and_then(|p| p.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let payload = frame
            .get("params")
            .and_then(|p| p.get("payload"))
            .cloned()
            .unwrap_or(Value::Null);
        let Some(seq) = state.next_seq_and_record(&session_id, &event_type, &payload) else {
            return;
        };
        let rewritten = super::fixtures::rewrite(frame, &session_id, seq);
        // A dead channel only means this connection went away (--drop-after, or
        // the client reconnecting). The turn keeps recording so the rest of it
        // is still there for `session.events.since` to fill the gap with.
        let _ = out.send(rewritten.to_string());
        tokio::time::sleep(Duration::from_millis(per_frame_ms)).await;
    }
    if let Some(record) = state.sessions.lock().get_mut(&session_id) {
        record.running = false;
        record.turn_handle = None;
    }
}

fn handle_slash_exec(out: &mpsc::UnboundedSender<String>, id: Value, params: &Value) {
    let command = params.get("command").and_then(Value::as_str).unwrap_or("");
    if command.trim().is_empty() {
        reply_err(out, id, 4004, "empty command");
        return;
    }
    reply_ok(out, id, json!({"type": "exec", "output": format!("ran {command}")}));
}

fn handle_command_dispatch(out: &mpsc::UnboundedSender<String>, id: Value, params: &Value) {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let arg = params.get("arg").and_then(Value::as_str).unwrap_or("");
    let output = if arg.is_empty() {
        format!("dispatched {name}")
    } else {
        format!("dispatched {name} {arg}")
    };
    reply_ok(out, id, json!({"type": "exec", "output": output}));
}

fn slash_completions() -> Value {
    json!({
        "items": [
            {"display": "/help", "text": "/help", "kind": "command", "meta": "list available commands"},
            {"display": "/model", "text": "/model", "kind": "command", "meta": "switch the active model"},
            {"display": "/clear", "text": "/clear", "kind": "command", "meta": "clear the transcript"},
            {"display": "/quit", "text": "/quit", "kind": "command", "meta": "end the session"},
        ],
        "replace_from": 0,
    })
}

async fn serve_http(mut stream: TcpStream, state: &Arc<GatewayState>) -> std::io::Result<()> {
    let mut data: Vec<u8> = Vec::new();
    let mut buf = [0u8; 4096];
    let header_end = loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }
        data.extend_from_slice(&buf[..n]);
        if let Some(pos) = find_header_end(&data) {
            break pos;
        }
    };
    let head = String::from_utf8_lossy(&data[..header_end]).into_owned();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or_default().to_string();
    let raw_path = request_parts.next().unwrap_or_default();
    let path = raw_path.split('?').next().unwrap_or_default().to_string();

    let content_length = find_header_value(&head, "content-length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    let cookie_header = find_header_value(&head, "cookie").unwrap_or_default();

    let body_start = header_end + 4;
    while data.len() < body_start + content_length {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
    }
    let body_end = data.len().min(body_start + content_length);
    let body = String::from_utf8_lossy(&data[body_start..body_end]).into_owned();

    let (status, response_body, cookies) = route_http(&method, &path, &cookie_header, &body, state);
    write_http_response(&mut stream, status, &cookies, &response_body).await
}

fn route_http(
    method: &str,
    path: &str,
    cookie_header: &str,
    body: &str,
    state: &Arc<GatewayState>,
) -> (u16, Value, Vec<String>) {
    let has_session_cookie =
        cookie_header.contains("hermes_session_at") || cookie_header.contains("hermes_session_rt");

    match (method, path) {
        ("GET", "/api/status") => (
            200,
            json!({
                "ok": true,
                "version": "fake-0.1",
                "auth_required": true,
                "auth_providers": ["basic"],
            }),
            Vec::new(),
        ),
        ("POST", "/auth/password-login") => login(body, state),
        ("POST", "/api/auth/ws-ticket") => {
            if has_session_cookie {
                let ticket = state.mint_ticket();
                (200, json!({"ticket": ticket, "ttl_seconds": 30}), Vec::new())
            } else {
                (401, json!({"detail": "unauthorized"}), Vec::new())
            }
        }
        ("GET", "/api/auth/me") => {
            if has_session_cookie {
                (200, json!({"username": state.config.user, "provider": "basic"}), Vec::new())
            } else {
                (401, json!({"detail": "unauthorized"}), Vec::new())
            }
        }
        ("POST", "/auth/logout") => (
            200,
            json!({"ok": true}),
            vec![
                "hermes_session_at=; Path=/; HttpOnly; Max-Age=0".to_string(),
                "hermes_session_rt=; Path=/; HttpOnly; Max-Age=0".to_string(),
            ],
        ),
        ("GET", "/api/profiles") => {
            if has_session_cookie {
                (200, profiles_body(), Vec::new())
            } else {
                (401, json!({"detail": "unauthorized"}), Vec::new())
            }
        }
        _ => (404, json!({"detail": "no such route"}), Vec::new()),
    }
}

fn login(body: &str, state: &Arc<GatewayState>) -> (u16, Value, Vec<String>) {
    let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let provider = parsed.get("provider").and_then(Value::as_str).unwrap_or("");
    if provider != "basic" {
        return (404, json!({"detail": "unknown provider"}), Vec::new());
    }
    let username = parsed.get("username").and_then(Value::as_str).unwrap_or("");
    let password = parsed.get("password").and_then(Value::as_str).unwrap_or("");
    if username != state.config.user || password != state.config.password {
        return (401, json!({"detail": "invalid credentials"}), Vec::new());
    }
    (
        200,
        json!({"ok": true, "next": ""}),
        vec![
            "hermes_session_at=fake-access-token; Path=/; HttpOnly".to_string(),
            "hermes_session_rt=fake-refresh-token; Path=/; HttpOnly".to_string(),
        ],
    )
}

fn profiles_body() -> Value {
    json!({
        "profiles": [
            {
                "name": "default",
                "is_default": true,
                "model": "fake-model-a",
                "description": "the default fake profile",
            },
            {
                "name": "second",
                "is_default": false,
                "model": "fake-model-b",
                "description": "a second fake profile",
            },
        ]
    })
}

async fn write_http_response(
    stream: &mut TcpStream,
    status: u16,
    cookies: &[String],
    body: &Value,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "",
    };
    let body_text = body.to_string();
    let mut head = format!("HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\n");
    for cookie in cookies {
        head.push_str("Set-Cookie: ");
        head.push_str(cookie);
        head.push_str("\r\n");
    }
    head.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        body_text.len()
    ));
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body_text.as_bytes()).await?;
    stream.flush().await
}
