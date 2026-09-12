//! Round-trip check for the host QR script (T6b/T12): the payload printed by
//! `scripts/hermes-qr.sh` MUST parse through the core's `parse_qr_payload` —
//! a QR the app cannot parse is worthless. Run directly on the host:
//!   cargo test --test qr_roundtrip
//! (a host test: no FFI, no network — pure `parse_qr_payload`).

use hermes_core::auth::endpoint::parse_qr_payload;

/// Exactly what `scripts/hermes-qr.sh --url http://192.168.1.48:9123 --user
/// hermo --name hermo-lan` prints (first line of its output).
#[test]
fn script_payload_round_trips() {
    let payload = "hermes://connect?v=1&url=http%3A%2F%2F192.168.1.48%3A9123&user=hermo&name=hermo-lan";
    let ep = parse_qr_payload(payload).expect("script payload must parse");
    assert_eq!(ep.base_url.as_str(), "http://192.168.1.48:9123/");
    assert_eq!(ep.username, "hermo");
    assert_eq!(ep.display_name, "hermo-lan");
}

/// Percent-encoding is the contract: a URL with path and query must survive
/// (the whole value is percent-encoded by the script, `safe=""`).
#[test]
fn encoded_path_and_query_survive() {
    let payload = "hermes://connect?v=1&url=http%3A%2F%2Fh.example%3A9123%2Fapi%3Fx%3D1&user=u&name=n";
    let ep = parse_qr_payload(payload).expect("encoded path/query must parse");
    assert_eq!(ep.base_url.as_str(), "http://h.example:9123/api?x=1");
}

/// A QR built with embedded credentials must be rejected (parse_qr_payload's
/// rule) — the script cannot produce one, and the app must not accept it.
#[test]
fn embedded_credentials_rejected() {
    let payload = "hermes://connect?v=1&url=http%3A%2F%2Fuser%3Apass%40h%3A9123&user=u&name=n";
    assert!(parse_qr_payload(payload).is_err());
}
