//! Drives a real `HermesCore` through pair, login, connect, open_session and
//! send against the in-process fake, and asserts the assistant text the core
//! surfaces is the text recorded in the fixture.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use hermes_core::core::{ConnectionStatus, EventSink, HermesCore, TranscriptChangeDto};
use hermes_fake_gateway::server::{self, Config};
use tokio::sync::Notify;

const CORE_FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../hermes_core/tests/fixtures");

#[derive(Default)]
struct Collected {
    assistant_final: Option<String>,
    connected: bool,
}

struct Collector {
    state: parking_lot::Mutex<Collected>,
    changed: Notify,
}

impl Collector {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            state: parking_lot::Mutex::new(Collected::default()),
            changed: Notify::new(),
        })
    }

    async fn wait_until<T>(&self, mut pick: impl FnMut(&Collected) -> Option<T>) -> T {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(value) = pick(&self.state.lock()) {
                return value;
            }
            let notified = self.changed.notified();
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                panic!("timed out waiting for the expected state");
            }
        }
    }
}

impl EventSink for Collector {
    fn on_transcript(&self, change: TranscriptChangeDto) {
        let row: serde_json::Value = match serde_json::from_str(&change.row_json) {
            Ok(row) => row,
            Err(_) => return,
        };
        if row.get("kind").and_then(|k| k.as_str()) != Some("assistant") {
            return;
        }
        if row.get("streaming").and_then(|s| s.as_bool()).unwrap_or(false) {
            return;
        }
        let Some(text) = row.get("text").and_then(|t| t.as_str()) else {
            return;
        };
        self.state.lock().assistant_final = Some(text.to_string());
        self.changed.notify_waiters();
    }

    fn on_connection(&self, status: ConnectionStatus) {
        if matches!(status, ConnectionStatus::Open) {
            self.state.lock().connected = true;
            self.changed.notify_waiters();
        }
    }
}

/// The assistant text of the plain turn, read back from the fixture so the
/// assertion cannot drift from the recording.
fn fixture_plain_text() -> String {
    let path = PathBuf::from(CORE_FIXTURES).join("events.jsonl");
    let body = std::fs::read_to_string(&path).expect("fixture readable");
    for line in body.lines() {
        let Ok(frame) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let params = &frame["params"];
        if params["type"] == "message.complete" {
            if let Some(text) = params["payload"]["text"].as_str() {
                return text.to_string();
            }
        }
    }
    panic!("no message.complete with text in {}", path.display());
}

fn temp_data_dir() -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("hermo-fake-gateway-smoke-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp data dir");
    dir
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn core_pairs_logs_in_and_streams_a_turn() {
    let running = server::start(Config {
        port: 0,
        user: "hermo".to_string(),
        password: "hermo".to_string(),
        fixture: PathBuf::from(CORE_FIXTURES).join("events.jsonl"),
        synthetic: Some(PathBuf::from(CORE_FIXTURES).join("events_synthetic.jsonl")),
        drop_after: None,
    })
    .await
    .expect("fake gateway starts");

    let core = HermesCore::new(temp_data_dir().to_string_lossy().to_string());
    let sink = Collector::new();

    let endpoint = core
        .clone()
        .pair(running.pairing_payload.clone())
        .await
        .expect("pairing payload accepted");
    assert!(endpoint.base_url.starts_with(&running.base_url));

    core.clone().login("hermo".to_string()).await.expect("password login");
    core.clone().connect(sink.clone()).await.expect("connect");
    sink.wait_until(|s| s.connected.then_some(())).await;

    let key = core.clone().open_session(None, 80).await.expect("open session");
    core.clone()
        .send(key, "hello from the smoke test".to_string())
        .await
        .expect("prompt submitted");

    let text = sink.wait_until(|s| s.assistant_final.clone()).await;
    assert_eq!(text, fixture_plain_text());

    running.shutdown();
}
