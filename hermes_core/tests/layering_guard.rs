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
        // T5 pure use-case helpers: a backoff table and the durable tab list
        // (serialization over in-memory strings only, the file write lives in
        // core.rs). Policy without I/O — the guard keeps them that way.
        src.join("reconnect.rs"),
        src.join("session_registry.rs"),
        // T16a pure policy: the Bot Chat list-before-create resolution
        // (Desktop Bot Mode invariant, `(profile, "Bot Chat")`). Pure rules
        // over (title, id) pairs — no I/O, no clock, no framework.
        src.join("bot_chat.rs"),
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
        // T5 (PLAN §4): `CoreError` crosses the FFI boundary, so its derive
        // carries `uniffi::Error`. That single documented exception is
        // allowed; every other `uniffi` mention — a `use uniffi::…`, an
        // `#[uniffi::export]`, or a `#[derive(uniffi::Object)]` on an entity
        // — stays flagged. Blanket-exempting the whole `#[derive(...)]` line
        // would have hidden exactly the violation this guard exists for.
        let scanned = if visible.starts_with("#[derive(") {
            visible.replace("uniffi::Error", "")
        } else {
            visible.to_string()
        };
        out.push((i + 1, scanned));
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

/// Rule pinned (T5): the FFI boundary exception is exactly ONE derive —
/// `uniffi::Error` on the domain error type. A derive that pulls the FFI
/// framework into an entity some other way is still a violation, and this test
/// fails if the exemption is ever broadened to the whole attribute.
#[test]
fn only_uniffi_error_derive_is_exempt() {
    let allowed = "#[derive(Debug, Clone, Error, uniffi::Error)]\n";
    assert!(
        !code_lines(allowed)
            .iter()
            .any(|(_, l)| FORBIDDEN.iter().any(|(p, _)| l.contains(p))),
        "the boundary error derive is allowed"
    );

    let object = "#[derive(uniffi::Object)]\n";
    assert!(
        code_lines(object)
            .iter()
            .any(|(_, l)| FORBIDDEN.iter().any(|(p, _)| l.contains(p))),
        "an uniffi::Object derive on an entity must stay flagged"
    );

    let imported = "use uniffi::setup_scaffolding;\n";
    assert!(
        code_lines(imported)
            .iter()
            .any(|(_, l)| FORBIDDEN.iter().any(|(p, _)| l.contains(p))),
        "a plain uniffi import must stay flagged"
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

// ── coverage (review #4 nit 3, T5) ──────────────────────────────────────────

/// Recursively collect every `src/**/*.rs` relative to `src/` (POSIX
/// separators, sorted for stable output). No new dependency: a plain walk.
fn all_src_files() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("src dir readable") {
            let entry = entry.expect("dir entry readable");
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    walk(&src, &mut files);
    files.sort();
    files
}

/// The adapter / framework files that legitimately name the outside world.
/// `core.rs` is NOT here: it is classified as the USE CASE (see below).
fn adapter_allowlist() -> Vec<PathBuf> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = vec![
        // Frame codec + WS client + typed RPC wrappers (adapter layer).
        src.join("rpc/mod.rs"),
        src.join("rpc/frames.rs"),
        src.join("rpc/client.rs"),
        src.join("rpc/api.rs"),
        // The slash-ladder policy: maps wire shapes (verified against
        // methods_tools.py) onto the try-then-fallback plan. It sits with the
        // adapters (it speaks wire code/DTO vocabulary), holds no I/O of its
        // own, and is unit-tested without a socket (T10).
        src.join("rpc/slash.rs"),
        // HTTP auth adapter (login, cookie jar, ticket minting).
        src.join("auth/client.rs"),
        // The probe binary: an executable adapter that drives the use case
        // from the command line (frameworks allowed, no business rules).
        src.join("bin/hermes-probe.rs"),
        // `cfg(test)`-only fake HTTP backend: test scaffolding compiled into
        // the crate, never shipped (src/auth/mod.rs gates it behind cfg(test)).
        src.join("auth/test_http_server.rs"),
    ];
    files
}

/// Crate-root module declarations (`lib.rs`) and bare `mod` files
/// (`auth/mod.rs`): a few lines of `pub mod` each, no logic, no layer.
/// Named explicitly so the classification covers the whole tree — a mod
/// file that grows logic must move into a real bucket.
fn crate_skeleton_files() -> Vec<PathBuf> {
    vec![
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/auth/mod.rs"),
    ]
}

/// The use case (PLAN §4 T5): the ONLY layer allowed to know adapters AND
/// entities. It is neither guarded (it legitimately names adapters) nor a
/// plain adapter (it holds no wire/HTTP rules), so it gets its own bucket —
/// named explicitly, so the classification is a decision, not an omission.
fn use_case_files() -> Vec<PathBuf> {
    vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src/core.rs")]
}

/// Coverage rule (T5, review #4 nit 3): EVERY `src/**/*.rs` must be either
/// guarded by the inner-layer scan, explicitly allowlisted as an
/// adapter/framework file, or named as the use case. A brand-new module can
/// therefore no longer quietly accumulate I/O: it lands in one of the three
/// buckets or the build fails. A stale entry (a deleted file) fails too, so
/// neither list can rot into a dumping ground.
#[test]
fn every_source_file_is_guarded_or_explicitly_allowlisted() {
    let guarded: Vec<PathBuf> = guarded_sources().into_iter().map(|(p, _)| p).collect();
    let allowlist = adapter_allowlist();
    let skeleton = crate_skeleton_files();
    let use_case = use_case_files();

    let mut problems = Vec::new();

    // 1. No stale entries: a listed file that no longer exists is an error,
    //    otherwise deleting a guarded file would silently shrink the guard's
    //    scope.
    for path in &allowlist {
        if !path.exists() {
            problems.push(format!(
                "allowlist entry does not exist (stale, remove or rename it): {}",
                path.display()
            ));
        }
    }
    for path in &use_case {
        if !path.exists() {
            problems.push(format!("use case file missing: {}", path.display()));
        }
    }
    for path in &skeleton {
        if !path.exists() {
            problems.push(format!("skeleton file missing: {}", path.display()));
        }
    }
    // Same for the guarded list itself (double-checks guarded_sources' asserts).
    for (path, _) in guarded_sources() {
        if !path.exists() {
            problems.push(format!("guarded source missing: {}", path.display()));
        }
    }

    // 2. Every existing src/**/*.rs is classified in EXACTLY ONE bucket.
    let all = all_src_files();
    assert!(!all.is_empty(), "the walk found no sources — the guard is vacuous");
    for file in &all {
        let in_guarded = guarded.contains(file);
        let in_allowlist = allowlist.contains(file);
        let in_use_case = use_case.contains(file);
        let in_skeleton = skeleton.contains(file);
        let buckets = in_guarded as u8 + in_allowlist as u8 + in_use_case as u8 + in_skeleton as u8;
        if buckets > 1 {
            problems.push(format!(
                "classified in more than one bucket: {}",
                file.display()
            ));
        } else if buckets == 0 {
            problems.push(format!(
                "unclassified source file (add it to guarded_sources(), the \
                 adapter allowlist or use_case_files() in tests/layering_guard.rs): {}",
                file.display()
            ));
        }
    }

    assert!(
        problems.is_empty(),
        "layering-guard coverage incomplete:\n{}",
        problems.join("\n")
    );
}

/// The classification must not drift silently: the adapters named in the
/// brief (PI_TASK_T5B §7) are allowlisted and never guarded, `core.rs` is
/// the named use case, and the pure use-case helpers (`reconnect.rs`,
/// `session_registry.rs`) stay guarded — they hold no I/O and must never
/// grow any.
#[test]
fn classification_covers_the_real_tree() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    // The adapters named in the brief are allowlisted.
    for expected in ["rpc/client.rs", "rpc/frames.rs", "rpc/api.rs", "auth/client.rs", "bin/hermes-probe.rs"] {
        let path = src.join(expected);
        assert!(
            adapter_allowlist().contains(&path),
            "{expected} must be in the adapter allowlist"
        );
        assert!(
            !guarded_sources().iter().any(|(p, _)| *p == path),
            "{expected} is an adapter and must not be scanned by the inner-layer guard"
        );
    }
    // `core.rs` is the use case: neither guarded nor a "mere adapter".
    let core = src.join("core.rs");
    assert!(use_case_files().contains(&core), "core.rs is the named use case");
    assert!(!guarded_sources().iter().any(|(p, _)| *p == core));
    assert!(!adapter_allowlist().contains(&core));
    // Pure helpers keep the entity-grade guard: policy without I/O.
    for expected in ["reconnect.rs", "session_registry.rs", "bot_chat.rs"] {
        let path = src.join(expected);
        assert!(
            guarded_sources().iter().any(|(p, _)| *p == path),
            "{expected} is pure policy and must stay guarded"
        );
    }
}
