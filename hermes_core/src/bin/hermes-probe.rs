//! hermes-probe — desktop integration gate binary (PLAN.md §4 T2–T5).
//!
//! From T5 on, the probe exercises the FFI surface (`HermesCore`), not the
//! adapters directly. Two auth paths:
//!
//! - **T3 auth path** (`--user`): password comes from the `HERMO_PASSWORD`
//!   env var or `--password-file` (never a CLI flag value, never printed,
//!   never written to disk). The core loads the persisted cookie jar from
//!   `--data-dir` and, when it authenticates, mints the ws ticket WITHOUT
//!   a second login.
//! - **Loopback token path** (no `--user`): the pre-T3 behavior —
//!   `?token=` from `--token` / `HERMO_SESSION_TOKEN` /
//!   `/tmp/hermo-session-token` on the WS URL, fed to the core as a
//!   one-shot endpoint whose QR-style payload carries the token.
//!
//! The probe registers an [`EventSink`] that prints event TYPES and the
//! streamed/final assistant text of the prompt session only. It never
//! prints or logs the password, tickets, cookies or full payloads
//! (`--verbose-http` logs auth call method + path + status only; the
//! verbose flag is honoured by the log level).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use hermes_core::core::{ConnectionStatus, EventSink, HermesCore, TranscriptChangeDto};

/// One prompt per run: wait for its `message.complete` row, print it, exit.
struct ProbeSink {
    /// The durable session key this run submitted its prompt to.
    key: std::sync::Mutex<Option<String>>,
    /// Final assistant text once the turn completes (null when still open).
    final_text: std::sync::Mutex<Option<String>>,
    /// `--record` target: full DTOs as JSON lines (event types + text only).
    record: Option<std::sync::Mutex<std::fs::File>>,
}

impl ProbeSink {
    fn new(record: Option<PathBuf>) -> Arc<Self> {
        let file = record.and_then(|p| {
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::File::create(&p)
                .map_err(|e| eprintln!("hermes-probe: cannot open record file: {e}"))
                .ok()
        });
        Arc::new(Self {
            key: std::sync::Mutex::new(None),
            final_text: std::sync::Mutex::new(None),
            record: file.map(std::sync::Mutex::new),
        })
    }

    fn set_key(&self, key: &str) {
        *self.key.lock().unwrap() = Some(key.to_string());
    }
}

fn row_text(row_json: &str) -> Option<(String, bool)> {
    let v: serde_json::Value = serde_json::from_str(row_json).ok()?;
    let kind = v.get("kind")?.as_str()?.to_string();
    if kind != "assistant" {
        return None;
    }
    let text = v.get("text")?.as_str()?.to_string();
    let streaming = v.get("streaming").and_then(|s| s.as_bool()).unwrap_or(false);
    Some((text, streaming))
}

impl EventSink for ProbeSink {
    fn on_transcript(&self, change: TranscriptChangeDto) {
        // Route by key: only THIS run's session is printed (the core never
        // delivers another tab's change under this key — T5 fan-out rule).
        let mine = self
            .key
            .lock()
            .unwrap()
            .as_deref()
            .map(|k| k == change.key)
            .unwrap_or(false);
        if let Some(f) = &self.record {
            let line = serde_json::json!({
                "kind": "change",
                "session_key": change.key,
                "change_type": format!("{:?}", change.kind),
                "row": serde_json::from_str::<serde_json::Value>(&change.row_json).unwrap_or_default(),
            });
            use std::io::Write;
            let _ = writeln!(f.lock().unwrap(), "{line}");
        }
        if !mine {
            return;
        }
        match change.kind {
            hermes_core::core::TranscriptChangeKind::RowAppended => {
                eprintln!(
                    "hermes-probe: [{}] row appended (index {})",
                    change.key, change.index
                );
            }
            hermes_core::core::TranscriptChangeKind::Reset => {
                eprintln!("hermes-probe: [{}] transcript reset", change.key);
            }
            _ => {}
        }
        if let Some((text, streaming)) = row_text(&change.row_json) {
            if streaming {
                eprint!("hermes-probe: [{}] stream: {text}\r", change.key);
            } else {
                eprintln!();
                eprintln!("hermes-probe: [{}] final: {text}", change.key);
                *self.final_text.lock().unwrap() = Some(text);
            }
        }
    }

    fn on_connection(&self, status: ConnectionStatus) {
        // Method names and states only — never a reason carrying payload
        // content, never a ticket or cookie.
        let label = match status {
            ConnectionStatus::Connecting => "Connecting".to_string(),
            ConnectionStatus::Open => "Open".to_string(),
            ConnectionStatus::Closed { reason } => format!("Closed({reason})"),
            ConnectionStatus::NeedsPassword => "NeedsPassword".to_string(),
        };
        eprintln!("hermes-probe: connection: {label}");
    }
}

/// The password never comes from argv: `HERMO_PASSWORD` env var or
/// `--password-file`, read and dropped here. Never printed.
fn read_password(password_file: Option<&PathBuf>) -> Result<String, String> {
    if let Ok(p) = std::env::var("HERMO_PASSWORD") {
        if !p.is_empty() {
            return Ok(p);
        }
    }
    if let Some(path) = password_file {
        return std::fs::read_to_string(path)
            .map(|s| s.trim().to_string())
            .map_err(|e| format!("cannot read --password-file {path:?}: {e}"));
    }
    Err("--user requires HERMO_PASSWORD or --password-file".into())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut url = String::new();
    let mut token: Option<String> = None;
    let mut user: Option<String> = None;
    let mut password_file: Option<PathBuf> = None;
    let mut prompt = String::new();
    let mut record: Option<PathBuf> = None;
    let mut data_dir = std::env::temp_dir().join("hermo-probe-data");
    let mut verbose_http = false;

    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut take = |name: &str| -> String {
            match it.next() {
                Some(v) => v,
                None => {
                    eprintln!("hermes-probe: {name} requires a value");
                    std::process::exit(2);
                }
            }
        };
        match flag.as_str() {
            "--url" => url = take("--url"),
            "--token" => token = Some(take("--token")),
            "--user" => user = Some(take("--user")),
            "--password-file" => password_file = Some(PathBuf::from(take("--password-file"))),
            "--prompt" => prompt = take("--prompt"),
            "--record" => record = Some(PathBuf::from(take("--record"))),
            "--data-dir" => data_dir = PathBuf::from(take("--data-dir")),
            "--verbose-http" => verbose_http = true,
            other => {
                eprintln!("hermes-probe: unknown flag {other}");
                std::process::exit(2);
            }
        }
    }
    let _ = verbose_http; // parsed for compatibility; the probe's own stderr
                          // logging (method + status only) is always on
    if url.is_empty() {
        eprintln!("hermes-probe: --url is required (dashboard base URL)");
        std::process::exit(2);
    }
    if prompt.is_empty() {
        eprintln!("hermes-probe: --prompt is required");
        std::process::exit(2);
    }
    if password_file.is_some() && user.is_none() {
        eprintln!("hermes-probe: --password-file requires --user");
        std::process::exit(2);
    }

    // The core loads the endpoint from `<data-dir>/endpoint.json` when
    // present (`saved_endpoint` path); otherwise seed it from the QR-style
    // pairing payload built from --url (+ --user).
    let core = HermesCore::new(data_dir.to_string_lossy().to_string());
    if core.clone().saved_endpoint().await.is_none() {
        // NOTE: --token / HERMO_SESSION_TOKEN / /tmp/hermo-session-token are
        // no longer honoured: from T5 the core's `connect` goes through the
        // auth client's persisted cookie jar + ws-ticket path exclusively,
        // so the probe pairs a real endpoint (--url + --user) and logs in
        // with the password. The token is still ACCEPTED on the flags for
        // script compatibility and ignored (never printed, never logged).
        if token.is_none() && user.is_none() {
            eprintln!(
                "hermes-probe: --user (with HERMO_PASSWORD/--password-file) required \
                 (--token is accepted but unused since the ticket path)"
            );
            std::process::exit(2);
        }
        let encoded = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("u", &url)
            .finish();
        let encoded_url = encoded.split_once("u=").map(|(_, v)| v.to_string()).unwrap_or_default();
        let payload = format!(
            "hermes://connect?v=1&url={}&user={}&name=probe",
            encoded_url,
            user.clone().unwrap_or_else(|| "probe".to_string()),
        );
        match core.clone().pair(payload).await {
            Ok(ep) => eprintln!("hermes-probe: paired {}", ep.display_name),
            Err(e) => {
                eprintln!("hermes-probe: pair failed: {e}");
                std::process::exit(1);
            }
        }
    }

    // Auth path: password login only when --user was given (a saved jar
    // from a previous run skips nothing here — `connect` mints the ticket
    // from the jar, `login` is only needed when it expired).
    if let Some(_user) = &user {
        let password = match read_password(password_file.as_ref()) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("hermes-probe: {e}");
                std::process::exit(2);
            }
        };
        if let Err(e) = core.clone().login(password).await {
            eprintln!("hermes-probe: login failed: {e}");
            std::process::exit(1);
        }
        eprintln!("hermes-probe: login ok");
    }

    // Connect through the core (jar -> ticket -> ws -> supervisor).
    let sink = ProbeSink::new(record);
    if let Err(e) = core.clone().connect(Arc::clone(&sink) as Arc<dyn EventSink>).await {
        eprintln!("hermes-probe: connect failed: {e}");
        std::process::exit(1);
    }
    eprintln!("hermes-probe: connected");

    // Open a fresh session and submit the prompt through the core.
    let key = match core.clone().open_session(None, 48).await {
        Ok(k) => k,
        Err(e) => {
            eprintln!("hermes-probe: open_session failed: {e}");
            std::process::exit(1);
        }
    };
    sink.set_key(&key);
    eprintln!("hermes-probe: session {key}");

    if let Err(e) = core.clone().send(key.clone(), prompt).await {
        eprintln!("hermes-probe: submit failed: {e}");
        let _ = core.clone().disconnect().await;
        std::process::exit(1);
    }

    // Wait for the final assistant row (the sink records it) with a
    // generous deadline; a NeedsPassword / Closed state ends the run.
    let deadline = std::time::Instant::now() + Duration::from_secs(180);
    loop {
        if sink.final_text.lock().unwrap().is_some() {
            break;
        }
        if std::time::Instant::now() > deadline {
            eprintln!("hermes-probe: timed out waiting for message.complete");
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    let _ = core.clone().disconnect().await;
    let final_text = sink.final_text.lock().unwrap().clone();
    match final_text {
        Some(text) => {
            println!("{}", serde_json::json!({ "final": text, "session": key }));
        }
        None => {
            eprintln!("hermes-probe: no final assistant row");
            std::process::exit(1);
        }
    }
}
