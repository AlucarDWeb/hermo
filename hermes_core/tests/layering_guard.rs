//! Layering guard: production code in the inner layers must not depend on
//! adapters, frameworks, I/O or clocks.
//!
//! The recurring defect in this project is an inward-pointing edge nobody
//! notices at review time (PR #3: `error.rs` importing `rpc::client`; T4:
//! `transcript/reducer.rs` importing `rpc::frames`; review #4: the entity
//! importing `json` while that module still called itself an adapter).
//!
//! A grep is a blunt instrument, but it is a *mechanical* one: it fails the
//! build the moment the edge is reintroduced, which is when the mistake is
//! cheapest to fix. The scan covers two kinds of file:
//!
//! - **entity**: `transcript/*`, `error.rs`, `auth/endpoint.rs` — pure rules
//!   and data;
//! - **neutral**: `json.rs`, `protocol.rs` — helpers and DTOs both the
//!   adapters and the entity import, which therefore must stay free of I/O
//!   and framework types too (PLAN §3: cross-boundary data is plain DTOs).
//!
//! What it does NOT cover, on purpose: `rpc::*` and `auth::client` are
//! adapters and are expected to name frameworks. Comment lines are ignored
//! (the docs are allowed to explain what must not be imported), block
//! comments are stripped, and everything from a file's first `#[cfg(test)]`
//! onward is out of scope (tests may reference adapters to assert against
//! them).

use std::fs;
use std::path::{Path, PathBuf};

/// Inner-layer sources, with the layer each one belongs to.
fn guarded_sources() -> Vec<(PathBuf, &'static str)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let entity = [
        src.join("transcript/mod.rs"),
        src.join("transcript/model.rs"),
        src.join("transcript/reducer.rs"),
        src.join("transcript/markdown.rs"),
        src.join("error.rs"),
        // QR payload parsing: entity, no I/O (PLAN §4 T3).
        src.join("auth/endpoint.rs"),
    ];
    let neutral = [src.join("protocol.rs"), src.join("json.rs")];
    let mut files: Vec<(PathBuf, &'static str)> = entity
        .into_iter()
        .map(|p| (p, "entity"))
        .chain(neutral.into_iter().map(|p| (p, "neutral")))
        .collect();
    files.sort();
    files.dedup();
    for (f, _) in &files {
        assert!(f.exists(), "guarded source missing: {}", f.display());
    }
    files
}

/// Patterns that mean "this inner layer now knows an adapter, a framework, the
/// outside world or the clock". `crate::json` is deliberately absent: `json.rs`
/// is a neutral helper module (pure `serde_json` accessors, no I/O), which is
/// what makes it legitimate for the entity to use (review #4, should 1) — and
/// why `json.rs` is itself scanned below.
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

/// Production code lines of `text`, with line numbers.
///
/// Drops doc/line comments, strips block comments, and stops at the file's
/// first `#[cfg(test)]` (test modules are allowed to reference adapters).
fn code_lines(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut in_block_comment = false;
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line == "#[cfg(test)]" {
            break;
        }

        // Strip /* ... */ spans (they may open and close on one line).
        let mut visible = String::new();
        let bytes: Vec<char> = line.chars().collect();
        let mut j = 0;
        while j < bytes.len() {
            if in_block_comment {
                if bytes[j] == '*' && bytes.get(j + 1) == Some(&'/') {
                    in_block_comment = false;
                    j += 2;
                    continue;
                }
                j += 1;
                continue;
            }
            if bytes[j] == '/' && bytes.get(j + 1) == Some(&'*') {
                in_block_comment = true;
                j += 2;
                continue;
            }
            visible.push(bytes[j]);
            j += 1;
        }

        let visible = visible.trim();
        if visible.is_empty() || visible.starts_with("//") {
            continue;
        }
        out.push((i + 1, visible.to_string()));
    }
    out
}

fn scan(path: &Path) -> Vec<String> {
    let text = fs::read_to_string(path).expect("guarded source is readable");
    let mut violations = Vec::new();
    for (line_no, line) in code_lines(&text) {
        for (pattern, what) in FORBIDDEN {
            if line.contains(pattern) {
                violations.push(format!(
                    "{}:{} uses `{}` ({}) -> {}",
                    path.display(),
                    line_no,
                    pattern,
                    what,
                    line
                ));
            }
        }
    }
    violations
}

#[test]
fn inner_layers_import_no_adapter_framework_io_or_clock() {
    let mut violations = Vec::new();
    for (file, _layer) in guarded_sources() {
        violations.extend(scan(&file));
    }
    assert!(
        violations.is_empty(),
        "Dependency Rule violated (inner layer -> adapter/framework/I-O/clock):\n{}",
        violations.join("\n")
    );
}

#[test]
fn guard_actually_reads_the_sources() {
    // A guard that silently scans nothing passes forever. Pin that it sees the
    // files, the real rules, and at least one non-comment code line per file.
    for (file, _) in guarded_sources() {
        let text = fs::read_to_string(&file).unwrap();
        assert!(
            !code_lines(&text).is_empty(),
            "{} has no code lines — the guard would be vacuous",
            file.display()
        );
    }
    assert!(FORBIDDEN.len() >= 10, "the rule list must stay substantive");

    // Sanity: the scan DOES flag an adapter line, and it does NOT flag a
    // neutral module the entity is allowed to use.
    let flagged = "    use crate::rpc::frames::EventParams;\n";
    assert!(
        code_lines(flagged)
            .iter()
            .any(|(_, l)| FORBIDDEN.iter().any(|(p, _)| l.contains(p))),
        "the scan must flag an adapter import"
    );
    let allowed = "    use crate::json::str_at;\n";
    assert!(
        !code_lines(allowed)
            .iter()
            .any(|(_, l)| FORBIDDEN.iter().any(|(p, _)| l.contains(p))),
        "`crate::json` is a neutral helper and must stay allowed"
    );
}

#[test]
fn code_lines_ignores_comments_and_test_modules() {
    let sample = "\
//! doc mentioning reqwest and tokio
/* block mentioning reqwest */
use crate::json;
#[cfg(test)]
use reqwest::Client;
";
    let lines = code_lines(sample);
    assert_eq!(lines.len(), 1, "only the production import survives: {lines:?}");
    assert!(lines[0].1.contains("crate::json"));

    // A one-line block comment is stripped even when code follows it.
    let inline = "use crate::json; /* std::fs */\n";
    let lines = code_lines(inline);
    assert_eq!(lines.len(), 1);
    assert!(!lines[0].1.contains("std::fs"), "inline block comment stripped: {lines:?}");
}
