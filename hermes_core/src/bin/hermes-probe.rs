//! hermes-probe — desktop integration gate binary (PLAN.md §4 T2).
//!
//! Connects to the gateway (loopback `?token=` auth; ticket/password is T3),
//! creates a session, submits the prompt and prints every inbound event as
//! one JSON line until `message.complete`. The token is read from the env or
//! `/tmp/hermo-session-token` and never written anywhere.
//!
//! Flow per the brief: `--record` writes the same JSONL to a path (the
//! authoritative fixture path stays scripts/record_fixture.py), `--busy`
//! sends a second prompt ~2 s into the first turn and prints the reply
//! ({"status":"redirected"} — verified, an ordinary result), `--data-dir` is
//! accepted for forward-compatibility with the T3 cookie jar.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::json;

use hermes_core::rpc::api;
use hermes_core::rpc::client::{ClientConfig, ClientError, ConnectionState, GatewayClient, GatewayEvent};

#[derive(Debug)]
struct Args {
    url: String,
    token: Option<String>,
    prompt: String,
    record: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    busy: bool,
    /// Stay connected after `message.complete` and print the connection
    /// state until it goes `Closed(reason)` — used by the heartbeat gate
    /// (connect, finish a turn, kill the backend, observe the timeout).
    watch_close: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        url: "http://127.0.0.1:9119".into(),
        token: None,
        prompt: String::new(),
        record: None,
        data_dir: None,
        busy: false,
        watch_close: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut take = |name: &str| -> Result<String, String> {
            it.next()
                .ok_or_else(|| format!("{name} requires a value"))
        };
        match flag.as_str() {
            "--url" => args.url = take("--url")?,
            "--token" => args.token = Some(take("--token")?),
            "--prompt" => args.prompt = take("--prompt")?,
            "--record" => args.record = Some(PathBuf::from(take("--record")?)),
            "--data-dir" => args.data_dir = Some(PathBuf::from(take("--data-dir")?)),
            "--busy" => args.busy = true,
            "--watch-close" => args.watch_close = true,
            other => return Err(format!("unknown flag {other}")),
        }
    }
    if args.prompt.is_empty() {
        return Err("--prompt is required".into());
    }
    Ok(args)
}

fn read_token(args: &Args) -> Result<String, String> {
    if let Some(t) = &args.token {
        return Ok(t.clone());
    }
    if let Ok(t) = std::env::var("HERMO_SESSION_TOKEN") {
        if !t.is_empty() {
            return Ok(t);
        }
    }
    std::fs::read_to_string("/tmp/hermo-session-token")
        .map(|s| s.trim().to_string())
        .map_err(|_| "--token, HERMO_SESSION_TOKEN or /tmp/hermo-session-token required".into())
}

/// Build `ws://host/api/ws?token=…` from the dashboard base URL.
fn ws_url_with_token(base: &str, token: &str) -> Result<String, String> {
    // Reuse the client's rewrite via a helper: build `?token=` on whatever
    // path the URL ends up with.
    let url = if base.starts_with("ws://") || base.starts_with("wss://") {
        base.to_string()
    } else {
        // http(s) base: swap the scheme, keep the rest.
        let stripped = base
            .strip_prefix("https://")
            .map(|r| format!("wss://{r}"))
            .or_else(|| base.strip_prefix("http://").map(|r| format!("ws://{r}")))
            .ok_or_else(|| format!("unsupported url: {base}"))?;
        if stripped.rsplit('/').next().map(|p| p.is_empty()).unwrap_or(true) {
            format!("{stripped}api/ws")
        } else if !stripped.contains("/api/ws") {
            format!("{stripped}/api/ws")
        } else {
            stripped
        }
    };
    let sep = if url.contains('?') { '&' } else { '?' };
    Ok(format!("{url}{sep}token={token}"))
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("hermes-probe: {e}\nusage: hermes-probe --url <base> [--token T] --prompt \"…\" [--record PATH] [--data-dir DIR] [--busy]");
            std::process::exit(2);
        }
    };
    let token = match read_token(&args) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("hermes-probe: {e}");
            std::process::exit(2);
        }
    };
    let ws_url = match ws_url_with_token(&args.url, &token) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("hermes-probe: {e}");
            std::process::exit(2);
        }
    };

    let mut writer = RecordWriter::new(args.record.clone());
    let client = match GatewayClient::connect(&ws_url, ClientConfig::default()).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("hermes-probe: connect failed: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("hermes-probe: connected, replay_epoch={:?}", client.replay_epoch());

    let mut events = client.events();
    let mut state = client.state();

    // Session + prompt.
    let created = match api::create_session(&client, 48).await {
        Ok(c) => c,
        Err(e) => die(&client, &e),
    };
    eprintln!(
        "hermes-probe: session {} (stored {})",
        created.session_id, created.stored_session_id
    );

    let sid = created.session_id.clone();
    let sid_for_loop = sid.clone();
    let busy_client = client.clone();
    let busy_prompt = args.prompt.clone();
    let busy_task = if args.busy {
        Some(tokio::spawn(async move {
            // ~2 s into the first turn, per the brief.
            tokio::time::sleep(Duration::from_secs(2)).await;
            match api::submit(&busy_client, &sid, &busy_prompt).await {
                Ok(status) => {
                    eprintln!("hermes-probe: busy submit status={status:?}");
                    status
                }
                Err(e) => {
                    eprintln!("hermes-probe: busy submit failed: {e}");
                    return;
                }
            };
            let _ = busy_prompt; // moved above; silence potential unused warnings
        }))
    } else {
        None
    };

    if let Err(e) = api::submit(&client, &sid_for_loop, &args.prompt).await {
        die(&client, &e);
    }

    // Stream every event as one JSON line until message.complete for our sid.
    loop {
        tokio::select! {
            ev = events.recv() => match ev {
                Ok(GatewayEvent::Event(params)) => {
                    let line = serde_json::json!({
                        "type": params.event_type,
                        "session_id": params.session_id,
                        "seq": params.seq,
                        "payload": params.payload,
                    });
                    let line = line.to_string();
                    println!("{line}");
                    writer.write(&line);
                    if params.event_type == "message.complete"
                        && (params.session_id.is_empty() || params.session_id == sid_for_loop)
                    {
                        break;
                    }
                }
                Ok(GatewayEvent::Watermark { .. }) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("hermes-probe: lagged, dropped {n} events");
                }
                Err(_) => {
                    eprintln!("hermes-probe: event stream ended");
                    break;
                }
            },
            st = state.changed() => {
                if st.is_ok() {
                    let s = state.borrow().clone();
                    if let ConnectionState::Closed(reason) = s {
                        eprintln!("hermes-probe: connection closed: {reason}");
                        std::process::exit(1);
                    }
                }
            }
        }
    }

    if let Some(task) = busy_task {
        let _ = task.await;
    }

    // Heartbeat gate mode: hold the connection open (heartbeat keeps
    // pinging) until the state watch reports Closed, then report the reason
    // and the elapsed wall-clock seconds since message.complete.
    if args.watch_close {
        let done = std::time::Instant::now();
        eprintln!("hermes-probe: holding connection open (watch-close mode)");
        loop {
            if state.changed().await.is_err() {
                break;
            }
            if let ConnectionState::Closed(reason) = state.borrow().clone() {
                eprintln!(
                    "hermes-probe: Closed({reason:?}) after {:.1}s",
                    done.elapsed().as_secs_f32()
                );
                if reason == "heartbeat timeout" {
                    std::process::exit(0);
                }
                std::process::exit(1);
            }
        }
        eprintln!("hermes-probe: state watch ended without a Closed report");
        std::process::exit(1);
    }

    client.close("probe done");
}

fn die(client: &GatewayClient, e: &ClientError) -> ! {
    client.close("probe error");
    eprintln!("hermes-probe: {e}");
    std::process::exit(1);
}

/// Optional `--record` sink: appends the printed JSONL lines verbatim.
struct RecordWriter {
    file: Option<std::fs::File>,
}

impl RecordWriter {
    fn new(path: Option<PathBuf>) -> Self {
        let file = path.and_then(|p| {
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::File::create(&p)
                .map_err(|e| eprintln!("hermes-probe: cannot open record file: {e}"))
                .ok()
        });
        Self { file }
    }

    fn write(&mut self, line: &str) {
        if let Some(f) = self.file.as_mut() {
            use std::io::Write;
            let _ = writeln!(f, "{line}");
        }
    }
}

// json! is used by the event line construction above.
#[allow(dead_code)]
fn _assert_json_in_scope(v: serde_json::Value) {
    let _ = json!(null) == v;
}
