//! Layering guard: the entity layer must not depend on adapters or frameworks.
//!
//! The recurring defect in this project is an inward-pointing edge nobody
//! notices at review time (PR #3: `error.rs` importing `rpc::client`; T4:
//! `transcript/reducer.rs` importing `rpc::frames`). A grep is a blunt
//! instrument, but it is a *mechanical* one: it fails the build the moment the
//! edge is reintroduced, which is exactly when the mistake is cheapest to fix.
//!
//! Comment lines are ignored on purpose — the module docs are allowed to name
//! the adapter modules they must not import.

use std::fs;
use std::path::{Path, PathBuf};

/// Files that belong to the entity layer, with the layers they must not know.
fn entity_sources() -> Vec<PathBuf> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![
        // Pure entity: transcript model, reducer and markdown splitter.
        src.join("transcript/mod.rs"),
        src.join("transcript/model.rs"),
        src.join("transcript/reducer.rs"),
        src.join("transcript/markdown.rs"),
        // Domain-facing error enum (PR #3 finding 6).
        src.join("error.rs"),
        // Pure value objects: the DTO both the codec and the entity import.
        src.join("protocol.rs"),
        // QR payload parsing: entity, no I/O (PLAN §4 T3).
        src.join("auth/endpoint.rs"),
    ];
    for f in &files {
        assert!(f.exists(), "entity source missing: {}", f.display());
    }
    files.sort();
    files.dedup();
    files
}

/// Patterns that mean "this entity now knows an adapter or a framework".
const FORBIDDEN: &[(&str, &str)] = &[
    ("crate::rpc::", "adapter (frame codec / WebSocket client)"),
    ("crate::auth::client", "adapter (HTTP + cookie jar)"),
    ("reqwest", "framework (HTTP client)"),
    ("tokio", "framework (async runtime)"),
    ("tungstenite", "framework (WebSocket)"),
    ("std::fs", "I/O"),
    ("std::net", "I/O"),
    ("std::process", "process control"),
    ("uniffi", "framework (FFI boundary)"),
    ("SystemTime", "clock"),
    ("Instant", "clock"),
];

fn code_lines(text: &str) -> Vec<(usize, &str)> {
    text.lines()
        .enumerate()
        .take_while(|(_, l)| l.trim() != "#[cfg(test)]")
        .map(|(i, l)| (i + 1, l.trim()))
        // Drop doc/line comments: naming the layers in prose is fine.
        .filter(|(_, l)| !l.starts_with("//"))
        .collect()
}

#[test]
fn entity_layer_imports_no_adapter_or_framework() {
    let mut violations = Vec::new();
    for file in entity_sources() {
        let text = fs::read_to_string(&file).expect("entity source is readable");
        for (line_no, line) in code_lines(&text) {
            for (pattern, what) in FORBIDDEN {
                if line.contains(pattern) {
                    violations.push(format!(
                        "{}:{} uses `{}` ({}) -> {}",
                        file.display(),
                        line_no,
                        pattern,
                        what,
                        line
                    ));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "Dependency Rule violated (entity -> adapter/framework):\n{}",
        violations.join("\n")
    );
}

#[test]
fn guard_actually_reads_the_entity_sources() {
    // A guard that silently scans nothing passes forever. Pin that it sees the
    // files, the real rules, and at least one non-comment line per file.
    for file in entity_sources() {
        let text = fs::read_to_string(&file).unwrap();
        assert!(
            !code_lines(&text).is_empty(),
            "{} has no code lines — the guard would be vacuous",
            file.display()
        );
    }
    assert!(FORBIDDEN.len() >= 10, "the rule list must stay substantive");

    // Sanity: the same scan DOES flag an adapter line, so the patterns work.
    let sample = "    use crate::rpc::frames::EventParams;\n";
    assert!(
        code_lines(sample)
            .iter()
            .any(|(_, l)| FORBIDDEN.iter().any(|(p, _)| l.contains(p))),
        "the scan must flag an adapter import"
    );
}
