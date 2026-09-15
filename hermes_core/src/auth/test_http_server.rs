//! Minimal in-process HTTP server for the auth tests (test-only).
//!
//! Serves the five auth routes over a real TCP socket (reqwest talks to
//! `127.0.0.1:<ephemeral>`) and COUNTS requests per route kind, so tests
//! can assert server-side facts — e.g. "the second client minted a ticket
//! WITHOUT a second password-login" — instead of trusting client-side
//! bookkeeping. No new dependency: plain tokio I/O, `Connection: close`
//! (one request per connection keeps the parser trivial and reqwest-compatible).

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use reqwest::StatusCode;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Route kinds the fake backend understands. The counters are per kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RouteKind {
    Status,
    Login,
    Ticket,
    Me,
    Logout,
    /// A request a gated route refused for a missing cookie.
    Denied,
    /// `GET /api/profiles` (T16a).
    Profiles,
}

/// Static route config: kind → (status, body, optional Set-Cookie).
#[derive(Clone)]
pub struct Route {
    pub kind: RouteKind,
    pub status: StatusCode,
    pub body: Value,
    pub set_cookie: Option<String>,
    /// Wire-level gate (PR #3 finding 1): when set, a request whose
    /// `Cookie` header does not contain this substring is answered 401
    /// regardless of `status` — the fake backend then inspects the wire
    /// like the real dashboard does instead of answering every ticket
    /// request blind. Test-only cookie values, never real secrets.
    pub require_cookie: Option<String>,
}

impl Route {
    pub fn status(status: StatusCode, body: Value) -> Self {
        Self { kind: RouteKind::Status, status, body, set_cookie: None, require_cookie: None }
    }

    pub fn login_ok() -> Self {
        Self {
            kind: RouteKind::Login,
            status: StatusCode::OK,
            body: json!({"ok": true, "next": "/"}),
            // Test-only fixed value, never a real session secret.
            set_cookie: Some("hermes_session_at=test-access-cookie; Path=/".into()),
            require_cookie: None,
        }
    }

    pub fn login_status(status: StatusCode) -> Self {
        Self {
            kind: RouteKind::Login,
            status,
            body: json!({"detail": "invalid credentials"}),
            set_cookie: None,
            require_cookie: None,
        }
    }

    /// The gated ws-ticket route: answers 401 unless the request carries
    /// the access cookie the login route set. This is what makes the
    /// cookie-reuse test non-tautological — a ticket hit proves the jar
    /// actually carried the cookie over the wire. (The old ungated
    /// `ticket_ok()` was removed: a ticket route that answers blind is
    /// exactly what PR #3 finding 1 forbids.)
    pub fn ticket_gated() -> Self {
        Self {
            kind: RouteKind::Ticket,
            status: StatusCode::OK,
            body: json!({"ticket": "test-ticket-1", "ttl_seconds": 30}),
            set_cookie: None,
            require_cookie: Some("hermes_session_at=test-access-cookie".into()),
        }
    }

    pub fn ticket_status(status: StatusCode) -> Self {
        Self {
            kind: RouteKind::Ticket,
            status,
            body: json!({"detail": "unauthorized"}),
            set_cookie: None,
            require_cookie: None,
        }
    }

    pub fn me_ok() -> Self {
        Self {
            kind: RouteKind::Me,
            status: StatusCode::OK,
            body: json!({"username": "hermo-test", "provider": "basic"}),
            set_cookie: None,
            require_cookie: None,
        }
    }

    pub fn logout_ok() -> Self {
        Self {
            kind: RouteKind::Logout,
            status: StatusCode::OK,
            body: json!({"ok": true}),
            set_cookie: None,
            require_cookie: None,
        }
    }

    /// `GET /api/profiles` with the verified wire shape (extra keys included
    /// on purpose: the DTO must ignore them).
    pub fn profiles_ok() -> Self {
        Self {
            kind: RouteKind::Profiles,
            status: StatusCode::OK,
            body: json!({"profiles": []}),
            set_cookie: None,
            require_cookie: None,
        }
    }

    /// Profiles with an explicit status (error mapping tests).
    pub fn profiles_status(status: StatusCode) -> Self {
        Self {
            kind: RouteKind::Profiles,
            status,
            body: json!({"detail": "unauthorized"}),
            set_cookie: None,
            require_cookie: None,
        }
    }
}

/// Handle to a running fake backend.
pub struct HttpTestServer {
    base_url: String,
    counters: Arc<Mutex<HashMap<RouteKind, u64>>>,
}

impl HttpTestServer {
    /// Bind `127.0.0.1:0` and spawn the accept loop.
    pub async fn spawn(routes: Vec<Route>) -> Self {
        let counters: Arc<Mutex<HashMap<RouteKind, u64>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let routes = Arc::new(routes);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test server binds an ephemeral port");
        let port = listener.local_addr().expect("local addr").port();
        let counters_loop = counters.clone();
        let routes_loop = routes.clone();
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let counters = counters_loop.clone();
                        let routes = routes_loop.clone();
                        tokio::spawn(async move {
                            let _ = serve_one(stream, &routes, &counters).await;
                        });
                    }
                    Err(_) => return,
                }
            }
        });
        Self {
            base_url: format!("http://127.0.0.1:{port}"),
            counters,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Server-side request count for a route kind (the assertion source
    /// of truth: clients can lie, the server counter cannot).
    pub fn count(&self, kind: RouteKind) -> u64 {
        self.counters.lock().get(&kind).copied().unwrap_or(0)
    }
}

fn kind_for_path(path: &str) -> Option<RouteKind> {
    match path {
        "/api/status" => Some(RouteKind::Status),
        "/auth/password-login" => Some(RouteKind::Login),
        "/api/auth/ws-ticket" => Some(RouteKind::Ticket),
        "/api/auth/me" => Some(RouteKind::Me),
        "/auth/logout" => Some(RouteKind::Logout),
        "/api/profiles" => Some(RouteKind::Profiles),
        _ => None,
    }
}

/// Read one request (headers + body per Content-Length), answer from the
/// matching route, close. Returns after exactly one exchange.
async fn serve_one(
    mut stream: TcpStream,
    routes: &[Route],
    counters: &Mutex<HashMap<RouteKind, u64>>,
) -> std::io::Result<()> {
    let mut data: Vec<u8> = Vec::new();
    let mut buf = [0u8; 4096];
    let header_end;
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Ok(()); // client went away mid-request
        }
        data.extend_from_slice(&buf[..n]);
        if let Some(pos) = find_header_end(&data) {
            header_end = pos;
            break;
        }
    }
    let head = String::from_utf8_lossy(&data[..header_end]).into_owned();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_string();
    let content_length = head
        .split("\r\n")
        .filter_map(|l| l.strip_prefix("Content-Length: ").or_else(|| l.strip_prefix("content-length: ")))
        .next()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    // Drain the declared body so the client's write side never errors.
    let body_start = header_end + 4;
    while data.len() < body_start + content_length {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
    }

    let path_only = path.split('?').next().unwrap_or_default();
    let kind = kind_for_path(path_only);
    // Wire inspection (PR #3 finding 1): the raw `Cookie` header, so a
    // route can refuse requests that did not carry the login cookie.
    let cookie_header = head
        .split("\r\n")
        .find_map(|l| l.strip_prefix("Cookie: ").or_else(|| l.strip_prefix("cookie: ")))
        .unwrap_or_default()
        .to_string();
    let (status_line, extra_headers, body) = match kind.and_then(|k| {
        routes.iter().find(|r| r.kind == k).map(|r| (k, r))
    }) {
        Some((kind, route)) => {
            if let Some(required) = &route.require_cookie {
                if !cookie_header.contains(required.as_str()) {
                    // Missing cookie on the wire: 401, exactly like the
                    // real dashboard's gated ws-ticket route. Not counted
                    // as a "handled" hit for the OK route.
                    *counters.lock().entry(RouteKind::Denied).or_insert(0) += 1;
                    return write_response(
                        stream,
                        "HTTP/1.1 401 Unauthorized\r\n",
                        "",
                        &json!({"detail": "unauthorized: missing session cookie"}).to_string(),
                    )
                    .await;
                }
            }
            *counters.lock().entry(kind).or_insert(0) += 1;
            let set_cookie = route
                .set_cookie
                .as_ref()
                .map(|c| format!("Set-Cookie: {c}\r\n"))
                .unwrap_or_default();
            (
                format!("HTTP/1.1 {} {}\r\n", route.status.as_u16(), route.status.canonical_reason().unwrap_or("")),
                set_cookie,
                route.body.to_string(),
            )
        }
        None => (
            "HTTP/1.1 404 Not Found\r\n".to_string(),
            String::new(),
            json!({"detail": "no such route"}).to_string(),
        ),
    };
    write_response(stream, &status_line, &extra_headers, &body).await
}

async fn write_response(
    mut stream: TcpStream,
    status_line: &str,
    extra_headers: &str,
    body: &str,
) -> std::io::Result<()> {
    let response = format!(
        "{status_line}Content-Type: application/json\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

fn find_header_end(data: &[u8]) -> Option<usize> {
    data.windows(4).position(|w| w == b"\r\n\r\n")
}
