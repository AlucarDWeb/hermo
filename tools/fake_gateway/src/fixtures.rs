//! Cuts the recorded event stream into replayable turns. The recording holds
//! two plain turns: the short one is what an ordinary prompt replays, the long
//! counting one is the slow turn interrupt and reconnect are tested against.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TurnKind {
    Plain,
    Tool,
    Clarify,
    Approval,
    Slow,
}

pub struct Fixtures {
    scenarios: HashMap<TurnKind, Vec<Value>>,
    ready: Value,
    approval_frame: Option<Value>,
}

impl Fixtures {
    pub fn load(path: &Path) -> io::Result<Fixtures> {
        let (scenarios, ready) = load_scenarios(path)?;
        Ok(Fixtures {
            scenarios,
            ready,
            approval_frame: None,
        })
    }

    pub fn load_with_synthetic(events: &Path, synthetic: &Path) -> io::Result<Fixtures> {
        let mut fixtures = Fixtures::load(events)?;
        fixtures.approval_frame = load_first_object(synthetic)?;
        Ok(fixtures)
    }

    pub fn turn(&self, kind: TurnKind) -> Vec<Value> {
        match kind {
            TurnKind::Plain => self.plain_frames(),
            TurnKind::Slow => self
                .scenarios
                .get(&TurnKind::Slow)
                .cloned()
                .unwrap_or_else(|| self.plain_frames()),
            TurnKind::Tool => self.scenarios.get(&TurnKind::Tool).cloned().unwrap_or_default(),
            TurnKind::Clarify => self
                .scenarios
                .get(&TurnKind::Clarify)
                .cloned()
                .unwrap_or_default(),
            TurnKind::Approval => {
                let mut frames = Vec::new();
                if let Some(frame) = &self.approval_frame {
                    frames.push(frame.clone());
                }
                frames.extend(self.plain_frames());
                frames
            }
        }
    }

    pub fn ready_payload(&self) -> Value {
        self.ready.clone()
    }

    fn plain_frames(&self) -> Vec<Value> {
        self.scenarios.get(&TurnKind::Plain).cloned().unwrap_or_default()
    }
}

pub fn select(prompt_text: &str) -> TurnKind {
    let lower = prompt_text.to_lowercase();
    if lower.contains("tool") {
        TurnKind::Tool
    } else if lower.contains("clarify") {
        TurnKind::Clarify
    } else if lower.contains("approve") {
        TurnKind::Approval
    } else if lower.contains("slow") {
        TurnKind::Slow
    } else {
        TurnKind::Plain
    }
}

pub fn rewrite(frame: &Value, session_id: &str, seq: i64) -> Value {
    let mut frame = frame.clone();
    if let Some(params) = frame.get_mut("params").and_then(Value::as_object_mut) {
        params.insert("session_id".to_string(), Value::String(session_id.to_string()));
        if params.contains_key("seq") {
            params.insert("seq".to_string(), Value::from(seq));
        }
    }
    frame
}

fn load_scenarios(path: &Path) -> io::Result<(HashMap<TurnKind, Vec<Value>>, Value)> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut scenarios: HashMap<TurnKind, Vec<Value>> = HashMap::new();
    let mut current: Vec<Value> = Vec::new();
    let mut has_tool = false;
    let mut has_clarify = false;
    let mut ready = Value::Null;
    let mut plain: Vec<Vec<Value>> = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let Some((value, event_type, payload)) = parse_event_frame(&line) else {
            continue;
        };
        if event_type == "gateway.ready" {
            // Connection preamble, not part of the first recorded turn.
            ready = payload;
            continue;
        }
        if event_type == "tool.start" {
            has_tool = true;
        }
        if event_type == "clarify.request" {
            has_clarify = true;
        }
        current.push(value);
        if event_type == "message.complete" {
            let frames = std::mem::take(&mut current);
            if has_clarify {
                scenarios.insert(TurnKind::Clarify, frames);
            } else if has_tool {
                scenarios.insert(TurnKind::Tool, frames);
            } else {
                plain.push(frames);
            }
            has_tool = false;
            has_clarify = false;
        }
    }

    if let Some(first) = plain.first() {
        scenarios.insert(TurnKind::Plain, first.clone());
    }
    if let Some(longest) = plain.iter().max_by_key(|frames| frames.len()) {
        scenarios.insert(TurnKind::Slow, longest.clone());
    }

    Ok((scenarios, ready))
}

fn parse_event_frame(line: &str) -> Option<(Value, String, Value)> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let value: Value = serde_json::from_str(trimmed).ok()?;
    let object = value.as_object()?;
    if object.get("method").and_then(Value::as_str) != Some("event") {
        return None;
    }
    let params = object.get("params")?.as_object()?;
    let event_type = params.get("type").and_then(Value::as_str)?.to_string();
    let payload = params.get("payload").cloned().unwrap_or(Value::Null);
    Some((value, event_type, payload))
}

fn load_first_object(path: &Path) -> io::Result<Option<Value>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        if value.is_object() {
            return Ok(Some(value));
        }
    }
    Ok(None)
}
