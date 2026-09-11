//! Fake-server integration tests for the T2 GatewayClient (PLAN §4 T2 item 4).
//!
//! An in-process WebSocket server built on the accept side of
//! tokio-tungstenite drives the client end-to-end. The heartbeat deadline is
//! shortened via `ClientConfig::for_tests()` — the injected-clock rule: no
//! test sleeps 45 real seconds.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use hermes_core::rpc::client::{ClientConfig, ConnectionState, GatewayClient, GatewayEvent};

/// A running fake gateway: accept loop + a handle for scripted replies.
struct FakeServer {
    addr: std::net::SocketAddr,
    /// Send a raw JSON line to the most recently accepted connection.
    send: tokio::sync::mpsc::UnboundedSender<String>,
    /// Every inbound line the client sent, in order.
    inbound: tokio::sync::mpsc::UnboundedReceiver<String>,
}

impl FakeServer {
    /// Spawn the accept loop. Every accepted connection gets the standard
    /// `gateway.ready` event first; then the `script` future drives the rest.
    fn spawn<F, Fut>(script: F) -> FakeServer
    where
        F: FnOnce(ScriptedConn) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        Self::spawn_inner(script, false)
    }

    /// Like [`spawn`] but the server never answers heartbeats — for the
    /// heartbeat-timeout test the connection must stay truly silent.
    fn spawn_silent<F, Fut>(script: F) -> FakeServer
    where
        F: FnOnce(ScriptedConn) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        Self::spawn_inner(script, true)
    }

    fn spawn_inner<F, Fut>(script: F, quiet: bool) -> FakeServer
    where
        F: FnOnce(ScriptedConn) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        // Bind on the caller's thread (inside the test runtime) so the addr
        // is known before returning; the accept loop itself is spawned.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let listener = TcpListener::from_std(listener).expect("tokio listener");
        let addr = listener.local_addr().expect("addr");
        let (send_tx, mut send_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let (in_tx, in_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let ws = tokio_tungstenite::accept_async(stream).await.expect("handshake");
            run_conn(ws, &mut send_rx, in_tx.clone(), script, quiet).await;
        });

        FakeServer {
            addr,
            send: send_tx,
            inbound: in_rx,
        }
    }

    fn url(&self) -> String {
        format!("ws://{}", self.addr)
    }

    fn push(&self, value: Value) {
        let _ = self.send.send(value.to_string());
    }

    /// Next inbound line from the client that is NOT a periodic heartbeat
    /// ping, with a test-failure timeout that covers the whole wait.
    async fn next_inbound(&mut self) -> String {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let line = tokio::time::timeout_at(deadline, self.inbound.recv())
                .await
                .expect("timed out waiting for a client request")
                .expect("client connection ended");
            if !is_heartbeat_ping(&line) {
                return line;
            }
        }
    }
    /// The `gateway.ready` frame every connection starts with.
    fn ready(&self) {
        self.push(json!({
            "jsonrpc": "2.0",
            "method": "event",
            "params": {
                "type": "gateway.ready",
                "payload": {"heartbeat": true, "replay_epoch": "e-fake", "skin": {}, "change_events": true}
            }
        }));
    }
}

/// Per-connection runner: forwards server->client lines and client->server
/// lines, then hands control to the script.
async fn run_conn<F, Fut>(
    mut ws: WebSocketStream<TcpStream>,
    send_rx: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
    in_tx: tokio::sync::mpsc::UnboundedSender<String>,
    script: F,
    quiet: bool,
) where
    F: FnOnce(ScriptedConn) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let conn = ScriptedConn {
        inbound: in_tx.clone(),
    };
    let script = script(conn);
    tokio::pin!(script);
    let mut script_done = false;

    loop {
        tokio::select! {
            _ = &mut script, if !script_done => {
                // Script done: keep the forwarding loop alive for as long as
                // the client connection exists.
                script_done = true;
            }
            outbound = send_rx.recv() => {
                match outbound {
                    Some(line) => {
                        if ws.send(Message::text(line)).await.is_err() {
                            return;
                        }
                    }
                    None => return,
                }
            }
            inbound = ws.next() => {
                match inbound {
                    Some(Ok(Message::Text(text))) => {
                        for line in text.split('\n') {
                            let line = line.trim();
                            if line.is_empty() {
                                continue;
                            }
                            let _ = in_tx.send(line.to_string());
                            // Always answer a gateway.ping so the client's
                            // heartbeat is satisfied unless the test
                            // deliberately goes silent.
                            if !quiet {
                                if let Some(reply) = ping_reply(line) {
                                    if ws.send(Message::text(reply.to_string())).await.is_err() {
                                        return;
                                    }
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(_)) => {}
                    Some(Err(_)) => return,
                }
            }
        }
    }
}

/// What a script can do: observe client lines (the channel is shared) and
/// finish; server pushes go through `FakeServer::push`.
/// Marker a script can observe; scripts currently only read via the shared
/// inbound channel on FakeServer. Kept as the extension point.
struct ScriptedConn {
    #[allow(dead_code)]
    inbound: tokio::sync::mpsc::UnboundedSender<String>,
}

/// Standard answer to a `gateway.ping` request.
fn ping_reply(line: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(line).ok()?;
    if v.get("method")?.as_str()? == "gateway.ping" {
        let id = v.get("id")?.clone();
        return Some(json!({"jsonrpc": "2.0", "id": id, "result": {"ok": true}}));
    }
    None
}

/// True for the client's periodic heartbeat pings (tests never assert on
/// them; `next_inbound` skips them).
fn is_heartbeat_ping(line: &str) -> bool {
    serde_json::from_str::<Value>(line)
        .ok()
        .and_then(|v| {
            let is_ping = v.get("method")?.as_str()? == "gateway.ping";
            let is_periodic = v
                .get("id")
                .and_then(Value::as_str)
                .map(|id| id.starts_with("heartbeat-"))
                .unwrap_or(false);
            Some(is_ping && is_periodic)
        })
        .unwrap_or(false)
}

#[tokio::test]
async fn connect_resolves_after_gateway_ready() {
    let server = FakeServer::spawn(|_conn| async {});
    server.ready();
    let client = GatewayClient::connect(&server.url(), ClientConfig::for_tests())
        .await
        .expect("connect");
    assert_eq!(*client.state().borrow(), ConnectionState::Open);
    assert_eq!(client.replay_epoch(), "e-fake");
}

#[tokio::test]
async fn request_response_matching() {
    let mut server = FakeServer::spawn(|_conn| async {});
    server.ready();
    let client = GatewayClient::connect(&server.url(), ClientConfig::for_tests())
        .await
        .expect("connect");

    // Two calls in flight; the server answers them out of order. The calls
    // and the responder run concurrently — a `call()` only hits the wire
    // once polled, so the reply loop must run inside the same join.
    let c1 = client.call("session.create", json!({"cols": 48}));
    let c2 = client.call("session.list", json!({"limit": 5}));
    let responder = async {
        let mut ids = Vec::new();
        for _ in 0..2 {
            let line = server.next_inbound().await;
            let v: Value = serde_json::from_str(&line).unwrap();
            ids.push(v["id"].as_str().unwrap().to_string());
            // Reply to whichever request this was.
            let reply = if v["method"] == "session.create" {
                json!({"jsonrpc": "2.0", "id": v["id"].clone(), "result": {"session_id": "s-live", "stored_session_id": "s-store"}})
            } else {
                json!({"jsonrpc": "2.0", "id": v["id"].clone(), "result": {"sessions": []}})
            };
            server.push(reply);
        }
        ids
    };
    let (out1, out2, ids) = tokio::join!(c1, c2, responder);
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1], "ids must be unique");

    let created = out1.expect("call 1");
    assert_eq!(created["session_id"], "s-live");
    let listed = out2.expect("call 2");
    assert!(listed["sessions"].is_array());
}

#[tokio::test]
async fn rpc_error_maps_to_typed_error() {
    let mut server = FakeServer::spawn(|_conn| async {});
    server.ready();
    let client = GatewayClient::connect(&server.url(), ClientConfig::for_tests())
        .await
        .expect("connect");

    let (result, _line) = tokio::join!(
        async {
            client
                .call("prompt.submit", json!({"session_id": "s", "text": "x"}))
                .await
        },
        async {
            let line = server.next_inbound().await;
            let v: Value = serde_json::from_str(&line).unwrap();
            server.push(json!({
                "jsonrpc": "2.0",
                "id": v["id"].clone(),
                "error": {"code": 4009, "message": "session busy", "data": null}
            }));
        }
    );
    match result {
        Err(hermes_core::rpc::client::ClientError::Rpc { code, message }) => {
            assert_eq!(code, 4009);
            assert_eq!(message, "session busy");
        }
        other => panic!("expected Rpc error, got {other:?}"),
    }
}

#[tokio::test]
async fn event_fanout_to_subscriber() {
    let server = FakeServer::spawn(|_conn| async {});
    server.ready();
    let client = GatewayClient::connect(&server.url(), ClientConfig::for_tests())
        .await
        .expect("connect");

    let mut sub1 = client.events();
    let mut sub2 = client.events();

    // The client issues a call; meanwhile two events flow. Both subscribers
    // must see both events — fan-out, not queue-stealing.
    let call = client.call("gateway.ping", Value::Null);
    server.push(json!({
        "jsonrpc": "2.0", "method": "event",
        "params": {"type": "message.delta", "session_id": "s1", "seq": 41, "payload": {"text": "Hel"}}
    }));
    server.push(json!({
        "jsonrpc": "2.0", "method": "event",
        "params": {"type": "message.delta", "session_id": "s1", "seq": 42, "payload": {"text": "lo"}}
    }));

    let _ = call.await; // answered by the ping reply below

    // Drain both subscribers.
    let mut got1 = Vec::new();
    let mut got2 = Vec::new();
    for _ in 0..2 {
        let e1 = tokio::time::timeout(Duration::from_secs(2), sub1.recv())
            .await
            .expect("sub1 event")
            .expect("sub1");
        let e2 = tokio::time::timeout(Duration::from_secs(2), sub2.recv())
            .await
            .expect("sub2 event")
            .expect("sub2");
        got1.push(e1);
        got2.push(e2);
    }
    assert_eq!(got1.len(), 2);
    assert_eq!(got2.len(), 2);
    for (a, b) in got1.iter().zip(got2.iter()) {
        assert_eq!(a, b, "both subscribers see the same events");
    }
    // Watermarks tracked per session.
    assert_eq!(client.last_seq("s1"), 42);
    assert_eq!(client.last_seq("s-other"), 0);
}

#[tokio::test]
async fn seq_watermarks_track_max_per_session() {
    let server = FakeServer::spawn(|_conn| async {});
    server.ready();
    let client = GatewayClient::connect(&server.url(), ClientConfig::for_tests())
        .await
        .expect("connect");

    // Out-of-order seq: watermark must keep the max, not the last.
    for seq in [5, 2, 9] {
        server.push(json!({
            "jsonrpc": "2.0", "method": "event",
            "params": {"type": "message.delta", "session_id": "sx", "seq": seq, "payload": {}}
        }));
    }
    let mut events = client.events();
    for _ in 0..3 {
        let _ = tokio::time::timeout(Duration::from_secs(2), events.recv())
            .await
            .expect("event")
            .expect("no lag");
    }
    assert_eq!(client.last_seq("sx"), 9);
}

/// The injected-clock heartbeat test: deadline shortened to 300 ms in
/// `ClientConfig::for_tests()`; the fake server goes silent (no pings
/// answered) and the client must close with "heartbeat timeout" well under a
/// real 45 s.
#[tokio::test]
async fn heartbeat_timeout_with_shortened_deadline() {
    let server = FakeServer::spawn_silent(|_conn| async {
        // Script never answers anything — the connection stays silent.
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    server.ready();
    let client = GatewayClient::connect(&server.url(), ClientConfig::for_tests())
        .await
        .expect("connect");
    let mut state = client.state();

    let started = std::time::Instant::now();
    let deadline = Duration::from_secs(10);
    loop {
        match tokio::time::timeout(deadline, state.changed()).await {
            Err(_) => panic!("no heartbeat timeout within {deadline:?}"),
            Ok(Err(_)) => panic!("state channel dropped"),
            Ok(Ok(())) => {
                let s = state.borrow().clone();
                if let ConnectionState::Closed(reason) = s {
                    assert_eq!(reason, "heartbeat timeout");
                    break;
                }
            }
        }
    }
    let elapsed = started.elapsed();
    // Injected deadline is 300 ms; allow generous CI slack but stay far
    // below the production 45 s.
    assert!(
        elapsed < Duration::from_secs(5),
        "heartbeat must fire on the injected deadline, took {elapsed:?}"
    );
}

/// Brief item (d): a response arriving after the caller's timeout must leave
/// the client usable — a follow-up call still works.
#[tokio::test]
async fn late_response_leaves_client_usable() {
    let mut server = FakeServer::spawn(|_conn| async {});
    server.ready();
    // 200 ms call timeout via a custom config.
    let cfg = ClientConfig {
        call_timeout: Duration::from_millis(200),
        ..ClientConfig::for_tests()
    };
    let client = GatewayClient::connect(&server.url(), cfg).await.expect("connect");

    // First call: the server withholds the answer until after the timeout.
    // The call and the line reader run concurrently (a call() only hits the
    // wire once polled).
    let reader = async {
        let line = server.next_inbound().await;
        serde_json::from_str::<Value>(&line).unwrap()["id"].clone()
    };
    let slow = client.call("session.list", json!({"limit": 1}));
    let (slow_result, slow_id) = tokio::join!(slow, reader);

    match slow_result {
        Err(hermes_core::rpc::client::ClientError::Timeout(_)) => {}
        other => panic!("expected Timeout, got {other:?}"),
    }

    // The late reply arrives now — it must not confuse the next call.
    server.push(json!({"jsonrpc": "2.0", "id": slow_id, "result": {"sessions": []}}));

    // Second call works and gets its own answer.
    let second = client.call("session.list", json!({"limit": 2}));
    let responder = async {
        let line2 = server.next_inbound().await;
        let v2: Value = serde_json::from_str(&line2).unwrap();
        assert_ne!(v2["id"], slow_id, "a fresh id, not the timed-out one");
        server.push(json!({
            "jsonrpc": "2.0", "id": v2["id"].clone(),
            "result": {"sessions": [{"id": "a"}]}
        }));
    };
    let (result, _) = tokio::join!(second, responder);
    let result = result.expect("second call must succeed");
    assert_eq!(result["sessions"][0]["id"], "a");
    assert_eq!(*client.state().borrow(), ConnectionState::Open);
}

/// One slow RPC must not stall event delivery (brief item 1, last bullet).
#[tokio::test]
async fn slow_rpc_does_not_block_events() {
    let mut server = FakeServer::spawn(|_conn| async {});
    server.ready();
    let client = GatewayClient::connect(&server.url(), ClientConfig::for_tests())
        .await
        .expect("connect");
    let mut events = client.events();

    // Fire a call the server will not answer yet. Spawned so it is polled
    // (hits the wire) while the test keeps running.
    let call_client = client.clone();
    let slow = tokio::spawn(async move {
        call_client
            .call("prompt.submit", json!({"session_id": "s", "text": "long turn"}))
            .await
    });
    let request = server.next_inbound().await;
    let request_id: Value = serde_json::from_str::<Value>(&request).unwrap()["id"].clone();

    // While the call is pending, events must still arrive.
    server.push(json!({
        "jsonrpc": "2.0", "method": "event",
        "params": {"type": "status.update", "session_id": "s", "payload": {"kind": "status", "text": "thinking"}}
    }));
    let ev = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("event must arrive while the RPC is pending")
        .expect("no lag");
    match ev {
        GatewayEvent::Event(p) => assert_eq!(p.event_type, "status.update"),
        other => panic!("expected Event, got {other:?}"),
    }

    // Clean up: answer the slow call (already read above) so nothing dangles.
    server.push(json!({"jsonrpc": "2.0", "id": request_id, "result": {"status": "streaming"}}));
    let _ = slow.await;
}

/// `call()` on a closed client fails with Closed, never hangs.
#[tokio::test]
async fn call_after_close_fails_fast() {
    let server = FakeServer::spawn_silent(|_conn| async {
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    server.ready();
    let client = GatewayClient::connect(&server.url(), ClientConfig::for_tests())
        .await
        .expect("connect");
    client.close("test close");
    match client.call("gateway.ping", Value::Null).await {
        Err(hermes_core::rpc::client::ClientError::Closed(reason)) => {
            assert_eq!(reason, "test close")
        }
        other => panic!("expected Closed, got {other:?}"),
    }
}
