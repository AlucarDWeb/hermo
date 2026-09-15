//! Session registry (entity layer, PLAN §4 T5 item 3): the durable list of
//! the app's open chat tabs.
//!
//! One entry per open session: the durable `stored_id` (what `session.list`
//! returns and what `session.resume` takes), the per-session `last_seen_seq`
//! watermark, the server `replay_epoch` it was seen under, the terminal
//! `cols` the session was created with, and an optional display `title`.
//! Plus which tab was last active, so the app's tabs survive a restart.
//!
//! Pure and deterministic: no I/O here. Serialization is
//! [`SessionRegistry::to_json`] / [`SessionRegistry::from_json`] over an
//! in-memory string; the file write (`<data_dir>/sessions.json`, atomic
//! tmp+rename) belongs to the use case (`core.rs`), which may do I/O.
//! Parsing is tolerant per PI_ROLE: missing or mistyped fields degrade to
//! defaults, never panic; unknown fields are ignored.

use crate::error::CoreError;
use crate::json;
use serde_json::{json, Value};

/// One open chat tab, persisted.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionRecord {
    /// Durable id (`session.list`'s `id`) — the resume key.
    pub stored_id: String,
    /// Highest seq applied to this session's transcript.
    pub last_seen_seq: i64,
    /// Server replay epoch the watermark was taken under (PLAN §1.5: an
    /// epoch change forces a transcript rebuild).
    pub replay_epoch: String,
    /// Terminal cols the session was created with.
    pub cols: i64,
    /// Display title, when known (`session.title` / `session.list`).
    pub title: String,
    /// Which Hermes profile owns the chat, when known — the create/resume
    /// `info.profile_name` (T16a: tabs must know which bot a chat belongs
    /// to). Empty when unknown; never a guess.
    pub profile_name: String,
}

impl SessionRecord {
    /// The profile to forward on `session.resume` (review #18 follow-up):
    /// a known profile scopes the resume to THAT profile's db on the
    /// gateway; an unknown one (old registry file, launch-profile chat)
    /// stays a bare resume — `None` OMITS the wire key, byte-for-byte what
    /// the pre-T16a client sent.
    pub fn resume_profile(&self) -> Option<&str> {
        (!self.profile_name.is_empty()).then_some(self.profile_name.as_str())
    }
}

/// The durable tab list: ordered records + the active tab.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionRegistry {
    active_key: Option<String>,
    sessions: Vec<SessionRecord>,
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace by `stored_id`, preserving position on replace.
    pub fn upsert(&mut self, record: SessionRecord) {
        match self.sessions.iter_mut().enumerate().find(|(_, r)| r.stored_id == record.stored_id) {
            Some((i, _slot)) => {
                self.sessions[i] = record;
            }
            None => self.sessions.push(record),
        }
    }

    /// Remove the tab with durable id `key`; true when it was present.
    pub fn close(&mut self, key: &str) -> bool {
        let before = self.sessions.len();
        self.sessions.retain(|r| r.stored_id != key);
        if self.active_key.as_deref() == Some(key) {
            self.active_key = None;
        }
        self.sessions.len() != before
    }

    /// Which tab was last active. A key not in the list is stored anyway —
    /// the record may be re-opened later; the round-trip keeps it verbatim.
    pub fn set_active(&mut self, key: Option<&str>) {
        self.active_key = key.map(str::to_string);
    }

    pub fn active_key(&self) -> Option<&str> {
        self.active_key.as_deref()
    }

    pub fn get(&self, key: &str) -> Option<&SessionRecord> {
        self.sessions.iter().find(|r| r.stored_id == key)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut SessionRecord> {
        self.sessions.iter_mut().find(|r| r.stored_id == key)
    }

    /// Update watermark and epoch for `key` (no-op when unknown).
    pub fn touch(&mut self, key: &str, last_seen_seq: i64, replay_epoch: &str) {
        if let Some(rec) = self.get_mut(key) {
            if last_seen_seq > rec.last_seen_seq {
                rec.last_seen_seq = last_seen_seq;
            }
            if !replay_epoch.is_empty() {
                rec.replay_epoch = replay_epoch.to_string();
            }
        }
    }

    pub fn sessions(&self) -> &[SessionRecord] {
        &self.sessions
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Serialize. Shape (the contract the round-trip test pins):
    /// `{"active":"<key>|absent","sessions":[{stored_id,last_seen_seq,
    /// replay_epoch,cols,title}, ...]}` in tab order.
    pub fn to_json(&self) -> String {
        let sessions: Vec<Value> = self
            .sessions
            .iter()
            .map(|r| {
                json!({
                    "stored_id": r.stored_id,
                    "last_seen_seq": r.last_seen_seq,
                    "replay_epoch": r.replay_epoch,
                    "cols": r.cols,
                    "title": r.title,
                    "profile_name": r.profile_name,
                })
            })
            .collect();
        let mut root = json!({ "sessions": sessions });
        if let Some(active) = &self.active_key {
            root["active"] = json!(active);
        }
        json::to_compact_string(&root)
    }

    /// Tolerant parse: missing fields degrade to defaults, a malformed
    /// document is a [`CoreError::Io`] (a corrupt registry must surface,
    /// not silently vanish — same policy as the cookie jar), and unknown
    /// fields are ignored.
    pub fn from_json(text: &str) -> Result<Self, CoreError> {
        let value: Value = serde_json::from_str(text)
            .map_err(|e| CoreError::Io(format!("corrupt sessions.json: {e}")))?;
        let mut reg = SessionRegistry::new();
        if let Some(list) = value.get("sessions").and_then(Value::as_array) {
            for item in list {
                if !item.is_object() {
                    continue;
                }
                let stored_id = json::str_at(item, "stored_id").to_string();
                if stored_id.is_empty() {
                    // A record without its durable key can never be resumed:
                    // skip it instead of carrying dead weight.
                    continue;
                }
                reg.sessions.push(SessionRecord {
                    stored_id,
                    last_seen_seq: json::i64_at(item, "last_seen_seq"),
                    replay_epoch: json::str_at(item, "replay_epoch").to_string(),
                    cols: json::i64_at(item, "cols"),
                    title: json::str_at(item, "title").to_string(),
                    // Written only since T16a: an older sessions.json has no
                    // such key and degrades to "" (unknown), never an error.
                    profile_name: json::str_at(item, "profile_name").to_string(),
                });
            }
        }
        let active = json::str_at(&value, "active").to_string();
        if !active.is_empty() {
            reg.active_key = Some(active);
        }
        Ok(reg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, seq: i64, epoch: &str) -> SessionRecord {
        SessionRecord {
            stored_id: id.to_string(),
            last_seen_seq: seq,
            replay_epoch: epoch.to_string(),
            cols: 48,
            title: format!("title of {id}"),
            profile_name: String::new(),
        }
    }

    /// Pins the review-#18 follow-up decision at the seam: a record with a
    /// known profile forwards THAT profile on resume (scoped db), a record
    /// with an unknown profile (old registry file, launch-profile chat)
    /// forwards NONE — `None` omits the wire key, the pre-T16a bare resume.
    /// The call site is `resume_all` (core.rs); this helper is the only
    /// profile decision it makes.
    #[test]
    fn resume_profile_known_forwards_none_for_unknown() {
        let mut bot = record("s-bot", 1, "e");
        bot.profile_name = "jn-core".into();
        assert_eq!(bot.resume_profile(), Some("jn-core"));
        bot.profile_name = String::new();
        assert_eq!(bot.resume_profile(), None, "unknown profile stays a bare resume");
    }

    /// Round-trip rule pinned: two instances over the same JSON see the same
    /// tabs, in the same order, with the same active tab and the same
    /// watermarks. This is the persistence contract of `<data_dir>/sessions.json`
    /// (PLAN §4 T5 item 3) — if persistence dropped order, active tab or
    /// watermarks, a restarted app would restore tabs wrongly, and this test
    /// fails.
    #[test]
    fn registry_round_trip_preserves_order_active_and_watermarks() {
        let mut reg = SessionRegistry::new();
        reg.upsert(record("s-alpha", 41, "e-1"));
        reg.upsert(record("s-beta", 7, "e-1"));
        reg.upsert(record("s-gamma", 0, ""));
        reg.set_active(Some("s-beta"));
        // Replace in place: order must not change on upsert of a known key.
        reg.upsert(record("s-alpha", 42, "e-2"));

        let text = reg.to_json();
        let parsed = SessionRegistry::from_json(&text).expect("round trip");

        let ids: Vec<&str> = parsed.sessions().iter().map(|r| r.stored_id.as_str()).collect();
        assert_eq!(ids, vec!["s-alpha", "s-beta", "s-gamma"], "tab order preserved");
        assert_eq!(parsed.active_key(), Some("s-beta"), "active tab preserved");
        let alpha = parsed.get("s-alpha").expect("alpha present");
        assert_eq!(alpha.last_seen_seq, 42, "watermark preserved (updated in place)");
        assert_eq!(alpha.replay_epoch, "e-2", "epoch preserved");
        assert_eq!(alpha.cols, 48);
        assert_eq!(alpha.title, "title of s-alpha");
        let beta = parsed.get("s-beta").expect("beta present");
        assert_eq!(beta.last_seen_seq, 7, "per-session watermarks are independent");
        assert_eq!(parsed.sessions().len(), 3);

        // profile_name round-trips too (T16a), and an OLD file without the
        // key degrades to "" instead of failing the parse.
        let mut bot = record("s-bot", 1, "e");
        bot.profile_name = "jn-core".into();
        let mut reg2 = SessionRegistry::new();
        reg2.upsert(bot);
        let parsed2 = SessionRegistry::from_json(&reg2.to_json()).expect("round trip");
        assert_eq!(parsed2.get("s-bot").unwrap().profile_name, "jn-core");
        let legacy = SessionRegistry::from_json(
            r#"{"sessions":[{"stored_id":"old","cols":80,"title":"t"}]}"#,
        )
        .expect("legacy shape parses");
        assert_eq!(legacy.get("old").unwrap().profile_name, "", "old file: unknown profile");
    }

    /// Rule pinned: `touch` never moves a watermark backwards and never
    /// overwrites a known epoch with an empty one.
    #[test]
    fn touch_keeps_max_seq_and_epoch() {
        let mut reg = SessionRegistry::new();
        reg.upsert(record("s1", 10, "e-1"));
        reg.touch("s1", 5, "");
        assert_eq!(reg.get("s1").unwrap().last_seen_seq, 10, "no backwards watermark");
        assert_eq!(reg.get("s1").unwrap().replay_epoch, "e-1", "empty epoch ignored");
        reg.touch("s1", 12, "e-2");
        assert_eq!(reg.get("s1").unwrap().last_seen_seq, 12);
        assert_eq!(reg.get("s1").unwrap().replay_epoch, "e-2");
        // Unknown key: no panic, no record created.
        reg.touch("ghost", 99, "e-9");
        assert!(reg.get("ghost").is_none());
    }

    /// Rule pinned: closing a tab removes exactly that record and clears the
    /// active pointer when it pointed at the closed tab.
    #[test]
    fn close_removes_record_and_active_pointer() {
        let mut reg = SessionRegistry::new();
        reg.upsert(record("a", 1, ""));
        reg.upsert(record("b", 2, ""));
        reg.set_active(Some("b"));
        assert!(reg.close("b"));
        assert!(reg.get("b").is_none());
        assert_eq!(reg.active_key(), None, "active pointer cleared with the tab");
        assert!(reg.get("a").is_some(), "other tabs untouched");
        assert!(!reg.close("b"), "closing twice is a no-op");
        // Active pointer to a still-open tab survives closing another tab.
        reg.set_active(Some("a"));
        reg.upsert(record("c", 0, ""));
        assert!(reg.close("c"));
        assert_eq!(reg.active_key(), Some("a"));
    }

    /// Rule pinned: parsing is tolerant (PI_ROLE) — missing fields degrade,
    /// records without a durable id are skipped, garbage entries never panic.
    #[test]
    fn from_json_is_tolerant_and_rejects_garbage() {
        let parsed = SessionRegistry::from_json(
            r#"{"active":"a","sessions":[
                {"stored_id":"a","last_seen_seq":9},
                {"last_seen_seq":5},
                {"stored_id":"b","replay_epoch":"e","cols":80,"title":"t"},
                "not-an-object",
                {"stored_id":123}
            ]}"#,
        )
        .expect("tolerant parse");
        assert_eq!(parsed.sessions().len(), 2, "id-less and garbage entries skipped");
        assert_eq!(parsed.active_key(), Some("a"));
        let a = parsed.get("a").unwrap();
        assert_eq!(a.last_seen_seq, 9);
        assert!(a.replay_epoch.is_empty());
        assert!(a.title.is_empty());
        let b = parsed.get("b").unwrap();
        assert_eq!(b.cols, 80);

        // A structurally broken document is an error, not an empty registry:
        // silently dropping the tab list would lose the user's sessions.
        assert!(SessionRegistry::from_json("{not json").is_err());
        // An empty object is valid: no tabs yet.
        let empty = SessionRegistry::from_json("{}").unwrap();
        assert!(empty.is_empty());
        assert_eq!(empty.active_key(), None);
    }

    /// Rule pinned: upsert of an unknown key appends at the END (new tabs go
    /// last, matching the UI's tab strip), of a known key replaces in place.
    #[test]
    fn upsert_appends_new_and_replaces_known_in_place() {
        let mut reg = SessionRegistry::new();
        reg.upsert(record("a", 1, ""));
        reg.upsert(record("b", 2, ""));
        reg.upsert(record("a", 3, ""));
        let ids: Vec<&str> = reg.sessions().iter().map(|r| r.stored_id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"], "replace keeps position, new appends");
        assert_eq!(reg.sessions().len(), 2);
        assert_eq!(reg.get("a").unwrap().last_seen_seq, 3);
    }
}
