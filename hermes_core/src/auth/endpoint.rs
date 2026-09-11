//! Gateway endpoint entity + QR payload parsing (entity layer — PLAN.md §4
//! T3 item 1).
//!
//! Pure value objects: no I/O, no reqwest, no tokio. The QR payload format
//! is `hermes://connect?v=1&url=<percent-encoded base URL>&user=<username>&
//! name=<host>` (PLAN §3): it carries URL + username + display name only,
//! never a secret.

use std::fmt;

use percent_encoding::percent_decode_str;
use url::Url;

use crate::error::CoreError;

/// One paired gateway: base URL + username + display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayEndpoint {
    /// Base URL of the dashboard, e.g. `http://192.168.1.48:9123`. Trailing
    /// slashes are stripped; the scheme is always `http` or `https`.
    pub base_url: Url,
    /// Dashboard username (the `basic` provider's identity).
    pub username: String,
    /// Human display name (the QR's `name`, usually the host).
    pub display_name: String,
}

impl GatewayEndpoint {
    pub fn new(base: Url, username: impl Into<String>, display_name: impl Into<String>) -> Self {
        Self {
            base_url: strip_trailing_slashes(base),
            username: username.into(),
            display_name: display_name.into(),
        }
    }
}

impl fmt::Display for GatewayEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}) — {}",
            self.display_name, self.username, self.base_url
        )
    }
}

fn strip_trailing_slashes(mut url: Url) -> Url {
    let path = url.path().to_string();
    let trimmed = path.trim_end_matches('/');
    let trimmed = if trimmed.is_empty() { "/" } else { trimmed };
    if trimmed != path {
        url.set_path(trimmed);
    }
    url
}

/// Parse a `hermes://connect?...` QR payload.
///
/// Rejects: a scheme other than `hermes`, `v != 1`, a missing `url`, a
/// `url` whose scheme is not `http`/`https`, and a `url` carrying embedded
/// credentials (`http://user:pass@host`) — a dashboard base URL never has
/// userinfo, and silently accepting one would send passwords in a URL
/// (PR #3 nit). Every value is percent-decoded (a `+`-bearing display name
/// and a percent-encoded space must both survive).
pub fn parse_qr_payload(payload: &str) -> Result<GatewayEndpoint, CoreError> {
    let url = Url::parse(payload.trim()).map_err(|_| CoreError::InvalidQr)?;
    if url.scheme() != "hermes" {
        return Err(CoreError::InvalidQr);
    }
    // `hermes://connect` parses with host == "connect" and an empty path.
    if url.host_str().map(|h| h.to_ascii_lowercase()) != Some("connect".into()) {
        return Err(CoreError::InvalidQr);
    }

    let mut version: Option<String> = None;
    let mut base: Option<String> = None;
    let mut user: Option<String> = None;
    let mut name: Option<String> = None;
    for pair in url.query().unwrap_or_default().split('&') {
        // Raw query iteration with percent-only decoding: `+` must survive
        // (the QR is URL-encoded, not form-encoded), a percent-encoded
        // space must decode to a space.
        let mut parts = pair.splitn(2, '=');
        let key = percent_decode(parts.next().unwrap_or_default());
        let value = percent_decode(parts.next().unwrap_or_default());
        match key.as_str() {
            "v" => version = Some(value),
            "url" => base = Some(value),
            "user" => user = Some(value),
            "name" => name = Some(value),
            _ => {} // unknown key: ignore, never panic (defensive rule)
        }
    }

    if version.as_deref() != Some("1") {
        return Err(CoreError::InvalidQr);
    }
    let raw_base = base.ok_or(CoreError::InvalidQr)?;
    let base_url = Url::parse(&raw_base).map_err(|_| CoreError::InvalidQr)?;
    if base_url.scheme() != "http" && base_url.scheme() != "https" {
        return Err(CoreError::InvalidQr);
    }
    // Embedded credentials in the base URL: reject (PR #3 nit).
    if !base_url.username().is_empty() || base_url.password().is_some() {
        return Err(CoreError::InvalidQr);
    }

    Ok(GatewayEndpoint {
        base_url: strip_trailing_slashes(base_url),
        username: user.unwrap_or_default(),
        display_name: name.unwrap_or_default(),
    })
}

/// Percent-decode a raw component (kept public for tests / callers that
/// hold raw, not yet decoded, QR text).
pub fn percent_decode(raw: &str) -> String {
    percent_decode_str(raw).decode_utf8_lossy().into_owned()
}

/// Map a dashboard base URL + ws-ticket to the gateway WS URL (PLAN §1.1):
/// `http`→`ws`, `https`→`wss`, path `/api/ws?ticket=…`, preserving a proxy
/// prefix path if present (`https://example.com/agent` keeps `/agent`).
///
/// The ticket goes in via `set_query_pairs` (PR #3 nit), so ticket
/// characters that carry query syntax meaning (`&`, `=`, `+`, `%`) are
/// percent-escaped instead of corrupting the query.
pub fn ws_url(base: &Url, ticket: &str) -> Url {
    let scheme = match base.scheme() {
        "https" => "wss",
        _ => "ws",
    };
    let mut out = base.clone();
    out.set_scheme(scheme)
        .expect("ws/wss are valid schemes for any parsed url");
    let path = out.path().trim_end_matches('/').to_string();
    let prefix = if path.is_empty() { String::new() } else { path };
    out.set_path(&format!("{prefix}/api/ws"));
    // `query_pairs_mut` percent-escapes the value (PR #3 nit): a ticket
    // carrying `&`, `=` or `+` cannot corrupt the query string the way a
    // raw `format!("ticket={t}")` could.
    {
        let mut pairs = out.query_pairs_mut();
        pairs.clear();
        pairs.append_pair("ticket", ticket);
    }
    out.set_fragment(None);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_payload_round_trips() {
        let ep = parse_qr_payload(
            "hermes://connect?v=1&url=http%3A%2F%2F192.168.1.48%3A9123&user=hermo-a1b2&name=atelier",
        )
        .expect("valid payload");
        assert_eq!(
            ep.base_url.as_str(),
            "http://192.168.1.48:9123/"
        );
        assert_eq!(ep.username, "hermo-a1b2");
        assert_eq!(ep.display_name, "atelier");
    }

    #[test]
    fn qr_plus_bearing_name_and_percent_encoded_space_survive() {
        let ep = parse_qr_payload(
            "hermes://connect?v=1&url=https%3A%2F%2Fgw.example.com&user=u&name=my+desk%20lamp",
        )
        .expect("valid payload");
        // `+` survives literally (URL decoding, not form decoding) and the
        // percent-encoded space becomes a real space.
        assert_eq!(ep.display_name, "my+desk lamp");
    }

    #[test]
    fn qr_trailing_slash_is_stripped() {
        let ep = parse_qr_payload(
            "hermes://connect?v=1&url=http%3A%2F%2Fhost%3A9119%2F&user=u&name=n",
        )
        .expect("valid payload");
        assert_eq!(ep.base_url.as_str(), "http://host:9119/");
    }

    #[test]
    fn qr_rejects_unknown_version() {
        let err = parse_qr_payload(
            "hermes://connect?v=2&url=http%3A%2F%2Fhost%3A9119&user=u&name=n",
        )
        .unwrap_err();
        assert!(matches!(err, CoreError::InvalidQr));
    }

    #[test]
    fn qr_rejects_missing_url() {
        let err = parse_qr_payload("hermes://connect?v=1&user=u&name=n").unwrap_err();
        assert!(matches!(err, CoreError::InvalidQr));
    }

    #[test]
    fn qr_rejects_ftp_base_scheme() {
        let err = parse_qr_payload(
            "hermes://connect?v=1&url=ftp%3A%2F%2Fhost%2Fpub&user=u&name=n",
        )
        .unwrap_err();
        assert!(matches!(err, CoreError::InvalidQr));
    }

    #[test]
    fn qr_rejects_wrong_outer_scheme() {
        assert!(matches!(
            parse_qr_payload("https://connect?v=1&url=http%3A%2F%2Fh&user=u&name=n").unwrap_err(),
            CoreError::InvalidQr
        ));
        assert!(matches!(
            parse_qr_payload("garbage").unwrap_err(),
            CoreError::InvalidQr
        ));
    }

    #[test]
    fn qr_rejects_base_url_with_embedded_credentials() {
        // PR #3 nit: `http://user:pass@host` must not slip through as a
        // base URL — credentials never belong in a dashboard endpoint.
        // The pre-fix parser accepted it and stored the userinfo.
        let err = parse_qr_payload(
            "hermes://connect?v=1&url=http%3A%2F%2Fuser%3Apass%40host%3A9123&user=u&name=n",
        )
        .unwrap_err();
        assert!(matches!(err, CoreError::InvalidQr));
    }

    #[test]
    fn ws_url_escapes_query_syntax_in_ticket() {
        // PR #3 nit: the ticket goes through `set_query_pairs`, so a
        // ticket carrying query-syntax characters (`&`, `=`, `+`) cannot
        // corrupt the query string. The pre-fix `format!("ticket={t}")`
        // produced `ticket=a&b=c` — two spurious query pairs.
        let base = Url::parse("http://host:9123").unwrap();
        let ws = ws_url(&base, "a&b=c+d");
        assert_eq!(ws.as_str(), "ws://host:9123/api/ws?ticket=a%26b%3Dc%2Bd");
        assert_eq!(ws.query().unwrap().matches('&').count(), 0, "exactly one query pair");
    }

    #[test]
    fn ws_url_http_becomes_ws_with_ticket() {
        let base = Url::parse("http://192.168.1.48:9123").unwrap();
        let ws = ws_url(&base, "tkt-1");
        assert_eq!(ws.as_str(), "ws://192.168.1.48:9123/api/ws?ticket=tkt-1");
    }

    #[test]
    fn ws_url_https_becomes_wss() {
        let base = Url::parse("https://gw.example.com").unwrap();
        assert_eq!(
            ws_url(&base, "t").as_str(),
            "wss://gw.example.com/api/ws?ticket=t"
        );
    }

    #[test]
    fn ws_url_preserves_proxy_prefix() {
        let base = Url::parse("https://example.com/agent").unwrap();
        assert_eq!(
            ws_url(&base, "t").as_str(),
            "wss://example.com/agent/api/ws?ticket=t"
        );
    }
}
