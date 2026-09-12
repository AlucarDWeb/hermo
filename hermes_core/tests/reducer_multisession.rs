//! T4 multi-session routing test (PLAN §4 T4 + decision 10): one reducer
//! per open chat; two reducers fed INTERLEAVED event streams must each keep
//! their own transcript, every emitted change must carry its own key, and a
//! `reset_from_resume` on one session must leave the other untouched.

use hermes_core::rpc::frames::EventParams;
use hermes_core::transcript::model::RowKind;
use hermes_core::transcript::reducer::{Reducer, SessionChange, TranscriptChange};
use serde_json::json;

/// An event for `sid` with `seq`.
fn ev(sid: &str, event_type: &str, payload: serde_json::Value) -> EventParams {
    EventParams {
        event_type: event_type.to_string(),
        session_id: sid.to_string(),
        seq: None,
        payload,
    }
}

#[test]
fn interleaved_sessions_route_every_change_to_its_own_key() {
    // Rule pinned: with two open chats (two reducers), interleaved events
    // never cross: every change carries the key of the session that caused
    // it, and each transcript only contains its own rows.
    let mut a = Reducer::new();
    let mut b = Reducer::new();

    // Interleave, alternating sessions per step, several rounds — with the
    // events in the recorded wire order (message.start before its deltas).
    let rounds = 4u32;
    let mut a_changes: Vec<SessionChange> = Vec::new();
    let mut b_changes: Vec<SessionChange> = Vec::new();
    for _ in 0..rounds {
        a_changes.extend(a.apply(&ev("sess-A", "thinking.delta", json!({"text": "a"}))));
        b_changes.extend(b.apply(&ev("sess-B", "message.start", json!({}))));
        a_changes.extend(a.apply(&ev("sess-A", "message.start", json!({}))));
        b_changes.extend(b.apply(&ev("sess-B", "message.delta", json!({"text": "b"}))));
        a_changes.extend(a.apply(&ev("sess-A", "message.delta", json!({"text": "x"}))));
        b_changes.extend(b.apply(&ev("sess-B", "message.complete", json!({"text": "B-reply"}))));
        a_changes.extend(a.apply(&ev("sess-A", "message.complete", json!({"text": "A-reply"}))));
    }
    // Every change routed under its own key — no exceptions.
    assert!(
        a_changes.iter().all(|c| c.key == "sess-A"),
        "all reducer-A changes carry sess-A"
    );
    assert!(
        b_changes.iter().all(|c| c.key == "sess-B"),
        "all reducer-B changes carry sess-B"
    );

    // Each transcript holds exactly its own rows (rounds = 4). A new turn's
    // `message.start` closes the previous thinking streak, so A accumulates
    // one short thinking row per round; B's deltas join into one row per
    // round because `complete` closes the row before the next round.
    let ta = a.transcript();
    assert_eq!(ta.rows.len(), 4 * 2, "A: one thinking row + one assistant row per round");
    let tb = b.transcript();
    assert_eq!(tb.rows.len(), 4, "B: one assistant row per round (deltas joined)");

    // A's content stays A's; B's stays B's — nothing crossed.
    for row in &ta.rows {
        match &row.kind {
            RowKind::Thinking { text } => assert_eq!(text, "a", "fresh streak per turn"),
            RowKind::Assistant { text, .. } => assert_eq!(text, "A-reply"),
            other => panic!("unexpected row in A: {:?}", other),
        }
    }
    for row in &tb.rows {
        match &row.kind {
            RowKind::Assistant { text, .. } => assert_eq!(text, "B-reply"),
            other => panic!("unexpected row in B: {:?}", other),
        }
    }
}

#[test]
fn reset_from_resume_on_one_session_leaves_the_other_untouched() {
    // Rule pinned: reset_from_resume takes the key it belongs to, wipes only
    // that session's transcript, and the sibling chat keeps every row.
    let mut a = Reducer::new();
    let mut b = Reducer::new();
    a.apply(&ev("sess-A", "message.start", json!({})));
    a.apply(&ev("sess-A", "message.complete", json!({"text": "A old"})));
    b.apply(&ev("sess-B", "message.start", json!({})));
    b.apply(&ev("sess-B", "message.complete", json!({"text": "B old"})));

    let changes = a.reset_from_resume("sess-A");
    assert_eq!(changes.len(), 1, "exactly one Reset");
    assert_eq!(changes[0].key, "sess-A", "the reset carries its own key");
    assert!(matches!(changes[0].change, TranscriptChange::Reset));

    assert!(a.transcript().rows.is_empty(), "A is wiped");
    assert_eq!(b.transcript().rows.len(), 1, "B untouched by A's reset");
    match &b.transcript().rows[0].kind {
        RowKind::Assistant { text, .. } => assert_eq!(text, "B old"),
        other => panic!("expected Assistant in B, got {:?}", other),
    }

    // Resetting an unknown key must not disturb either transcript.
    let stray = a.reset_from_resume("sess-other");
    assert_eq!(stray[0].key, "sess-other");
    assert!(b.transcript().rows.len() == 1);
    a.apply(&ev("sess-A", "message.start", json!({})));
    assert_eq!(a.transcript().rows.len(), 1, "A reusable after reset");
}
