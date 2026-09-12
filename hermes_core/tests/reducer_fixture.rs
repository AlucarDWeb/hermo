//! T4 fixture replay (PLAN §4 T4): `tests/fixtures/events.jsonl` was
//! recorded live in T0 from a real gateway; replaying it through the
//! reducer must reproduce the row sequence observed on the wire — count,
//! kinds in order, and each assistant row's text equal to the
//! `message.complete` text. "Does not panic" is not an assertion.

use hermes_core::rpc::frames::{self, Decoded};
use hermes_core::transcript::model::RowKind;
use hermes_core::transcript::reducer::{Reducer, TranscriptChange};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/events.jsonl");

/// All decoded events of the fixture, in file order (session-bound ones
/// carry the same, redacted live session id).
fn fixture_events() -> Vec<frames::EventParams> {
    let data = std::fs::read_to_string(FIXTURE).expect("fixture readable");
    let mut events = Vec::new();
    for line in data.lines().filter(|l| !l.trim().is_empty()) {
        if let Some(Decoded::Event(ev)) = frames::decode(line) {
            events.push(ev);
        }
    }
    events
}

#[test]
fn fixture_replay_reproduces_the_row_sequence() {
    // The recorded wire facts this replay must reproduce (one session, so
    // the live sid of the fixture carries them all). Four completed turns,
    // verified `message.complete` texts in order; the r-long turn was fully
    // streamed and completed on the wire (PROVENANCE scenario coverage).
    const ASSISTANT_COMPLETE_TEXTS: [&str; 4] = [
        "hello from the fixture recording.",
        "L'output è: hermo-fixture-tool-check.",
        "Preferisci il verde.",
        "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13\n14\n15\n16\n17\n18\n19\n20",
    ];

    let events = fixture_events();
    assert!(events.len() > 50, "fixture carries many session events");

    let mut reducer = Reducer::new();
    let mut routed_keys = std::collections::BTreeSet::new();
    for ev in &events {
        for change in reducer.apply(ev) {
            routed_keys.insert(change.key.clone());
            // Every change carries its session key — never empty.
            assert!(!change.key.is_empty(), "routed change without a key");
        }
    }
    assert_eq!(routed_keys.len(), 1, "all session events share one live sid");

    let t = reducer.transcript();
    let kind_seq: Vec<&'static str> =
        t.rows.iter().map(|r| t.kind_at(r.index)).collect();

    // Header state first: session.info / session.title merge into it and
    // never create rows.
    assert_eq!(
        t.header.title, "Reply with exact fixture sentence",
        "the last session.title wins"
    );
    assert_eq!(t.header.model, "deepseek/deepseek-v4.1-flash");
    assert_eq!(t.header.cwd, "/redacted");

    // Row kinds in order (gateway events only — no user row comes from the
    // gateway; header events merge into the header, no header rows). The
    // recorded wire order: `message.start` opens the assistant row BEFORE
    // the turn's thinking deltas stream, so each turn renders as
    // [assistant, thinking, tool/clarify cards…] in first-open order.
    assert_eq!(
        kind_seq,
        vec![
            "assistant",  // turn 1: message.start (seq 2)…complete (seq 10)
            "thinking",   // turn 1 thinking deltas (seq 4…)
            "assistant",  // turn 2: message.start (seq 13)
            "thinking",   // turn 2 thinking deltas (seq 14…)
            "tool",       // terminal card (generating seq 17 → start → complete)
            "assistant",  // turn 3: message.start (seq 40)
            "thinking",   // turn 3 thinking deltas (seq 42…)
            "tool",       // clarify tool card (generating seq 44 → start → complete)
            "clarify",    // clarify.request (seq 47, batch shape)
            "assistant",  // turn 4: message.start (seq 63)
            "thinking",   // turn 4 thinking deltas (seq 65…)
        ],
        "row kinds must appear in the recorded wire order"
    );
    assert_eq!(t.rows.len(), 11, "row count of the recorded sequence");

    // Each finished turn's assistant row equals its `message.complete` text.
    let assistant_texts: Vec<(&str, bool)> = t
        .rows
        .iter()
        .filter_map(|r| match &r.kind {
            RowKind::Assistant { text, streaming, .. } => Some((text.as_str(), *streaming)),
            _ => None,
        })
        .collect();
    assert_eq!(assistant_texts.len(), 4, "four completed turns on the wire");
    for (i, expected) in ASSISTANT_COMPLETE_TEXTS.iter().enumerate() {
        assert_eq!(
            assistant_texts[i].0, *expected,
            "assistant row {i} must equal its message.complete text"
        );
        assert!(!assistant_texts[i].1, "completed turn {i} must not stream");
    }
}

/// Tool events in the fixture pair two ids (terminal + clarify) into exactly
/// two cards, both terminal after their `tool.complete`.
#[test]
fn fixture_tool_cards_pair_by_id() {
    let mut reducer = Reducer::new();
    for ev in fixture_events() {
        reducer.apply(&ev);
    }
    let t = reducer.transcript();
    let tool_rows: Vec<&RowKind> =
        t.rows.iter().map(|r| &r.kind).filter(|k| matches!(k, RowKind::Tool(_))).collect();
    assert_eq!(tool_rows.len(), 2, "two tools, two cards — no duplicates");
    for kind in tool_rows {
        match kind {
            RowKind::Tool(card) => {
                assert!(!card.tool_id.is_empty(), "paired with its tool_id");
                assert!(card.complete, "both recorded tools completed");
                assert!(
                    !card.result_json.is_empty(),
                    "tool.complete result crossed as JSON"
                );
            }
            _ => unreachable!(),
        }
    }
}

/// The clarify card in the fixture is the batch shape and stays unresolved
/// until resolve_clarify is called (the fixture's clarify.respond result is
/// an RPC reply, not an event — the reducer must not auto-resolve).
#[test]
fn fixture_clarify_card_only_resolves_via_resolve_clarify() {
    let mut reducer = Reducer::new();
    for ev in fixture_events() {
        reducer.apply(&ev);
    }
    // Snapshot the card state, then drop the borrow before resolving.
    let (request_id, index, question) = {
        let t = reducer.transcript();
        let index = t
            .rows
            .iter()
            .position(|r| matches!(r.kind, RowKind::Clarify(_)))
            .expect("the fixture carries a clarify.request");
        match &t.rows[index].kind {
            RowKind::Clarify(card) => {
                assert!(!card.resolved, "an RPC result must not resolve the card");
                assert_eq!(card.questions.len(), 1);
                (card.request_id.clone(), index, card.questions[0].question.clone())
            }
            _ => unreachable!(),
        }
    };
    assert_eq!(question, "Quale colore preferisci, blu o verde?");
    let key = fixture_events()
        .iter()
        .find(|e| e.event_type == "clarify.request")
        .expect("clarify event present")
        .session_id
        .clone();
    let changes = reducer.resolve_clarify(&key, &request_id);
    assert!(
        matches!(changes[0].change, TranscriptChange::RowUpdated { index } if index == index),
        "the card updates in place at its own index"
    );
    match &reducer.transcript().rows[index].kind {
        RowKind::Clarify(card) => assert!(card.resolved, "resolve_clarify clears it"),
        _ => unreachable!(),
    }
}
