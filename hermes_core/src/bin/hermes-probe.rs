//! hermes-probe — desktop integration gate binary (PLAN.md §4 T2 + T3).
//!
//! Two auth paths:
//! - **T3 auth path** (`--user`): password comes from the `HERMO_PASSWORD`
//!   env var or `--password-file` (never a CLI flag value, never printed,
//!   never written to disk). Flow: `GET /api/status` → `login` (skipped
//!   when the persisted cookie jar already authenticates) →
//!   `mint_ticket` → `ws_url(base, ticket)` → connect → session → turn.
//!   The jar persists at `<data-dir>/cookies.json`, so a second run with
//!   the same `--data-dir` mints its ticket WITHOUT a second login.
//! - **Loopback token path** (no `--user`): the pre-T3 behavior —
//!   `?token=` from `--token` / `HERMO_SESSION_TOKEN` /
//!   `/tmp/hermo-session-token` on the WS URL.
//!
//! Prints every inbound event as one JSON line until `message.complete`.
//! `--record` writes the same JSONL to a path, `--busy` sends a second
//! prompt ~2 s into the first turn, `--verbose-http` logs every auth HTTP
//! call (method + path + status — never headers, never cookies) so the
//! cookie-reuse gate is observable.

use std::path::PathBuf;
use std::time::Duration;

use hermes_core::auth::client::AuthClient;
use hermes_core::auth::endpoint::ws_url;
use hermes_core::rpc::api;
use hermes_core::rpc::client::{ClientConfig, ClientError, ConnectionState, GatewayClient, GatewayEvent};

#[derive(Debug)]
struct Args {
    url: String,
    token: Option<String>,
    user: Option<String>,
    password_file: Option<PathBuf>,
    prompt: String,
    record: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    busy: bool,
    watch_close: bool,
    /// Log each auth HTTP call (method + path + outcome) to stderr.
    verbose_http: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        // No default: the gateway must be named explicitly (fix pass item 9
        // — never silently point at a local 9119).
        url: String::new(),
        token: None,
        user: None,
        password_file: None,
        prompt: String::new(),
        record: None,
        data_dir: None,
        busy: false,
        watch_close: false,
        verbose_http: false,
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
            "--user" => args.user = Some(take("--user")?),
            "--password-file" => args.password_file = Some(PathBuf::from(take("--password-file")?)),
            "--prompt" => args.prompt = take("--prompt")?,
            "--record" => args.record = Some(PathBuf::from(take("--record")?)),
            "--data-dir" => args.data_dir = Some(PathBuf::from(take("--data-dir")?)),
            "--busy" => args.busy = true,
            "--watch-close" => args.watch_close = true,
            "--verbose-http" => args.verbose_http = true,
            other => return Err(format!("unknown flag {other}")),
        }
    }
    if args.url.is_empty() {
        return Err("--url is required (dashboard base URL, e.g. http://192.168.1.48:9123)".into());
    }
    if args.prompt.is_empty() {
        return Err("--prompt is required".into());
    }
    if args.password_file.is_some() && args.user.is_none() {
        return Err("--password-file requires --user".into());
    }
    if args.user.is_some() && args.token.is_some() {
        return Err("--user and --token are mutually exclusive".into());
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
        .map_err(|_| "--token, HERMO_SESSION_TOKEN or /tmp/hermo-session-token required (or use --user for the auth path)".into())
}

/// The password never comes from argv: `HERMO_PASSWORD` env var or
/// `--password-file` (mode 600 suggested, /tmp), read and dropped here.
fn read_password(args: &Args) -> Result<String, String> {
    if let Ok(p) = std::env::var("HERMO_PASSWORD") {
        if !p.is_empty() {
            return Ok(p);
        }
    }
    if let Some(path) = &args.password_file {
        return std::fs::read_to_string(path)
            .map(|s| s.trim().to_string())
            .map_err(|e| format!("cannot read --password-file {path:?}: {e}"));
    }
    Err("--user requires HERMO_PASSWORD or --password-file".into())
}

/// The T3 auth path: status → (login, only when the jar is not yet
/// authenticated) → ws-ticket. Returns the WS URL with `?ticket=`.
async fn authenticate(
    args: &Args,
    user: &str,
    password: &str,
) -> Result<String, String> {
    let client = AuthClient::new(args.data_dir.as_deref())
        .map_err(|e| format!("auth client init: {e}"))?;
    let verbose = args.verbose_http;

    let status = client
        .status(&args.url)
        .await
        .map_err(|e| format!("GET /api/status: {e}"))?;
    if verbose {
        eprintln!(
            "hermes-probe: GET /api/status -> auth_required={} providers={:?}",
            status.auth_required, status.auth_providers
        );
    }

    // Login only when the jar does not already authenticate us
    // (`me()` succeeds). A persisted jar from a previous run skips it.
    let mut logged_in = false;
    if let Ok(me) = client.me(&args.url).await {
        logged_in = true;
        if verbose {
            eprintln!(
                "hermes-probe: GET /api/auth/me -> OK (jar authenticates, skipping login), user={}",
                me.get("username").and_then(|v| v.as_str()).unwrap_or("?")
            );
        }
    }
    if !logged_in {
        match client.login(&args.url, user, password).await {
            Ok(()) => {
                if verbose {
                    eprintln!("hermes-probe: POST /auth/password-login -> 200 (logged in)");
                }
            }
            Err(e) => return Err(format!("login: {e}")),
        }
    }

    let ticket = client
        .mint_ticket(&args.url)
        .await
        .map_err(|e| format!("POST /api/auth/ws-ticket: {e}"))?;
    if verbose {
        eprintln!(
            "hermes-probe: POST /api/auth/ws-ticket -> 200 (ttl={}s)",
            ticket.ttl_seconds
        );
    }

    let base = url::Url::parse(&args.url).map_err(|e| format!("--url: {e}"))?;
    Ok(ws_url(&base, &ticket.ticket).to_string())
}

/// The error -> message mapping lives in `CoreError`'s `Display` (via
/// `From`): auth failures surface through `{e}` at the call sites below,
/// so `InvalidCredentials`, `SessionExpired`, `RateLimited` and
/// `UnknownProvider` print their distinct meanings without a probe-local
/// duplicate of the mapping.

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("hermes-probe: {e}\nusage: hermes-probe --url <base> [--user U --password-file F | HERMO_PASSWORD] [--token T] --prompt \"…\" [--record PATH] [--data-dir DIR] [--busy] [--watch-close] [--verbose-http]");
            std::process::exit(2);
        }
    };

    // ── Auth: build the WS URL either via the T3 ticket path or ?token= ──
    let ws_target = if let Some(user) = &args.user {
        let password = match read_password(&args) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("hermes-probe: {e}");
                std::process::exit(2);
            }
        };
        match authenticate(&args, user, &password).await {
            Ok(u) => u,
            Err(e) => {
                eprintln!("hermes-probe: auth failed: {e}");
                std::process::exit(1);
            }
        }
    } else {
        let token = match read_token(&args) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("hermes-probe: {e}");
                std::process::exit(2);
            }
        };
        match legacy_ws_url_with_token(&args.url, &token) {
            Ok(u) => u,
            Err(e) => {
                eprintln!("hermes-probe: {e}");
                std::process::exit(2);
            }
        }
    };

    let mut writer = RecordWriter::new(args.record.clone());
    let client = match GatewayClient::connect(&ws_target, ClientConfig::default()).await {
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

/// Loopback `?token=` path (pre-T3 behavior, kept): build
/// `ws://host/api/ws?token=…` from the dashboard base URL.
fn legacy_ws_url_with_token(base: &str, token: &str) -> Result<String, String> {
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
