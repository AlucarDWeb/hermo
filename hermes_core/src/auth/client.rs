//! HTTP auth client with a persisted cookie jar (adapter layer — PLAN.md
//! §4 T3 item 2).
//!
//! The only place in the crate where `reqwest` and the cookie store live.
//! No policy here: the one decision is error *mapping* (status code →
//! [`CoreError`] variant, per the brief); the jar is loaded from and saved
//! to `<data_dir>/cookies.json` so a second process resumes the session
//! without a second login (the middleware rotates an expired access cookie
//! from the 30-day refresh cookie on the ws-ticket call — PLAN §2).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cookie_store::CookieStore;

use reqwest::{Client, StatusCode};
use reqwest_cookie_store::CookieStoreMutex;
use serde_json::{json, Value};

use crate::error::CoreError;
use crate::json;

/// Where the jar lives: `<data_dir>/cookies.json`.
pub const COOKIE_JAR_FILE: &str = "cookies.json";

/// Default request timeout — a hung dashboard must surface as
/// [`CoreError::Timeout`], not block the caller forever.
pub const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// Routes per PLAN §2 (verified against Hermes v0.21.1).
pub const EP_STATUS: &str = "/api/status";
pub const EP_LOGIN: &str = "/auth/password-login";
pub const EP_WS_TICKET: &str = "/api/auth/ws-ticket";
pub const EP_ME: &str = "/api/auth/me";
pub const EP_LOGOUT: &str = "/auth/logout";

/// `GET /api/status` payload, tolerantly parsed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GatewayStatus {
    pub auth_required: bool,
    pub auth_providers: Vec<String>,
}

/// `POST /api/auth/ws-ticket` payload.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WsTicket {
    pub ticket: String,
    pub ttl_seconds: i64,
}

/// HTTP auth client. Clone is cheap (shared reqwest handle + jar).
#[derive(Clone)]
pub struct AuthClient {
    http: Client,
    jar: Arc<CookieStoreMutex>,
    jar_path: Option<PathBuf>,
}

impl AuthClient {
    /// Build a client whose cookie jar loads from (and saves to)
    /// `<data_dir>/cookies.json`. A missing or unreadable jar file is NOT
    /// an error: an empty jar just means "not logged in yet".
    pub fn new(data_dir: Option<&Path>) -> Result<Self, CoreError> {
        let jar_path = data_dir.map(|d| d.join(COOKIE_JAR_FILE));
        let store = match &jar_path {
            Some(path) if path.exists() => {
                let raw = std::fs::read(path)
                    .map_err(|e| CoreError::Io(format!("read {}: {e}", path.display())))?;
                // cookie_store::serde::json format (list of cookie structs).
                cookie_store::serde::json::load(raw.as_slice())
                    .unwrap_or_else(|_| CookieStore::default())
            }
            _ => CookieStore::default(),
        };
        if let Some(dir) = data_dir {
            std::fs::create_dir_all(dir)
                .map_err(|e| CoreError::Io(format!("create {}: {e}", dir.display())))?;
        }
        let store = Arc::new(CookieStoreMutex::new(store));
        let http = Client::builder()
            .cookie_provider(store.clone())
            .timeout(DEFAULT_HTTP_TIMEOUT)
            .build()
            .map_err(|e| CoreError::Network(e.to_string()))?;
        Ok(Self {
            http,
            jar: store,
            jar_path,
        })
    }

    /// Raw access to the live store (tests / diagnostics).
    pub fn jar(&self) -> Arc<CookieStoreMutex> {
        self.jar.clone()
    }

    fn base(&self, base: &str, path: &str) -> Result<url::Url, CoreError> {
        let mut url = url::Url::parse(base).map_err(|_| CoreError::InvalidQr)?;
        let base_path = url.path().trim_end_matches('/');
        let joined = if base_path.is_empty() {
            path.to_string()
        } else {
            format!("{base_path}{path}")
        };
        url.set_path(&joined);
        Ok(url)
    }

    /// Persist the jar after any response that may have set a cookie.
    fn save_jar(&self) {
        if let Some(path) = &self.jar_path {
            let store = self.jar.lock().unwrap();
            let mut buf = Vec::new();
            // Include non-persistent (session) cookies: the dashboard's
            // session cookie must survive a process restart even before a
            // refresh rotation made it persistent.
            match cookie_store::serde::json::save_incl_expired_and_nonpersistent(
                &store, &mut buf,
            ) {
                Ok(()) => {
                    if let Err(e) = std::fs::write(path, &buf) {
                        log::warn!("cookie jar write failed: {e}");
                    }
                }
                Err(e) => log::warn!("cookie jar serialize failed: {e}"),
            }
        }
    }

    async fn send(&self, req: reqwest::RequestBuilder, ctx: ErrorCtx) -> Result<Value, CoreError> {
        let resp = req.send().await.map_err(|e| {
            if e.is_timeout() {
                CoreError::Timeout
            } else {
                CoreError::Network(e.to_string())
            }
        })?;
        let status = resp.status();
        // Persist every cookie the server may have set (login, logout,
        // ticket rotation) before the body is dropped.
        self.save_jar();
        if !status.is_success() {
            return Err(map_status(status, ctx));
        }
        resp.json::<Value>()
            .await
            .map_err(|e| CoreError::Network(format!("decode body: {e}")))
    }

    /// `GET /api/status` — public, no cookies needed.
    pub async fn status(&self, base: &str) -> Result<GatewayStatus, CoreError> {
        let url = self.base(base, EP_STATUS)?;
        let value = self.send(self.http.get(url), ErrorCtx::Other).await?;
        Ok(GatewayStatus {
            auth_required: json::bool_at(&value, "auth_required"),
            auth_providers: value
                .get("auth_providers")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }

    /// `POST /auth/password-login` with `{"provider":"basic",…}`.
    pub async fn login(
        &self,
        base: &str,
        username: &str,
        password: &str,
    ) -> Result<(), CoreError> {
        let url = self.base(base, EP_LOGIN)?;
        let body = json!({
            "provider": "basic",
            "username": username,
            "password": password,
            "next": "",
        });
        self.send(
            self.http.post(url).json(&body),
            ErrorCtx::Login,
        )
        .await
        .map(|_| ())
    }

    /// `POST /api/auth/ws-ticket` — single-use, TTL 30 s (PLAN §2).
    pub async fn mint_ticket(&self, base: &str) -> Result<WsTicket, CoreError> {
        let url = self.base(base, EP_WS_TICKET)?;
        let value = self
            .send(self.http.post(url).json(&json!({})), ErrorCtx::Cookie)
            .await?;
        Ok(WsTicket {
            ticket: json::str_at(&value, "ticket").to_string(),
            ttl_seconds: json::i64_at(&value, "ttl_seconds"),
        })
    }

    /// `GET /api/auth/me` — cheap "am I still logged in" probe.
    pub async fn me(&self, base: &str) -> Result<Value, CoreError> {
        let url = self.base(base, EP_ME)?;
        self.send(self.http.get(url), ErrorCtx::Cookie).await
    }

    /// `POST /auth/logout`.
    pub async fn logout(&self, base: &str) -> Result<(), CoreError> {
        let url = self.base(base, EP_LOGOUT)?;
        self.send(self.http.post(url), ErrorCtx::Cookie)
            .await
            .map(|_| ())
    }
}

/// Which endpoint a failing response came from — 401 means different
/// things on the login route (wrong password) and on cookie routes
/// (session expired).
#[derive(Clone, Copy)]
enum ErrorCtx {
    Login,
    Cookie,
    Other,
}

fn map_status(status: StatusCode, ctx: ErrorCtx) -> CoreError {
    match (status.as_u16(), ctx) {
        (401, ErrorCtx::Login) => CoreError::InvalidCredentials,
        (401, _) => CoreError::SessionExpired,
        (429, _) => CoreError::RateLimited,
        (404, ErrorCtx::Login) => CoreError::UnknownProvider,
        (code, _) => CoreError::Http(code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::test_http_server::{HttpTestServer, Route, RouteKind};

    fn temp_data_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hermo-t3-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[tokio::test]
    async fn status_parses_auth_required_and_providers() {
        let server = HttpTestServer::spawn(vec![Route::status(
            StatusCode::OK,
            json!({"auth_required": true, "auth_providers": ["basic"]}),
        )]).await;
        let client = AuthClient::new(None).unwrap();
        let s = client.status(server.base_url()).await.unwrap();
        assert!(s.auth_required);
        assert_eq!(s.auth_providers, vec!["basic".to_string()]);
        assert_eq!(server.count(RouteKind::Status), 1);
    }

    #[tokio::test]
    async fn login_stores_cookie_and_jar_file_appears() {
        let dir = temp_data_dir("login");
        let server = HttpTestServer::spawn(vec![
            Route::login_ok(),
            Route::ticket_ok(),
        ]).await;
        let client = AuthClient::new(Some(&dir)).unwrap();
        client.login(server.base_url(), "hermo-test", "pw").await.unwrap();
        client.mint_ticket(server.base_url()).await.unwrap();

        let jar_file = dir.join(COOKIE_JAR_FILE);
        assert!(jar_file.exists(), "jar must be persisted to {jar_file:?}");
        let raw = std::fs::read_to_string(&jar_file).unwrap();
        assert!(raw.contains("hermo_session_at"), "jar holds the access cookie");
        // Every cookie-carrying response saved the jar.
        assert_eq!(server.count(RouteKind::Login), 1);
        assert_eq!(server.count(RouteKind::Ticket), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn second_client_on_same_data_dir_skips_login() {
        let dir = temp_data_dir("reuse");
        let server = HttpTestServer::spawn(vec![
            Route::login_ok(),
            Route::ticket_ok(),
            Route::ticket_ok(),
        ]).await;
        let first = AuthClient::new(Some(&dir)).unwrap();
        first.login(server.base_url(), "hermo-test", "pw").await.unwrap();

        // A brand-new client on the SAME data dir: the loaded jar
        // authenticates, so mint_ticket must succeed and the server must
        // NOT see a second password-login. Only one ticket route is
        // configured: if the second client had to (re)login, the ticket
        // call itself would be the sole hit and this assertion would see 1.
        let second = AuthClient::new(Some(&dir)).unwrap();
        let ticket = second.mint_ticket(server.base_url()).await.unwrap();
        assert!(!ticket.ticket.is_empty());
        assert_eq!(server.count(RouteKind::Login), 1, "no second login");
        assert_eq!(server.count(RouteKind::Ticket), 1, "direct ticket mint, no re-login detour");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn ticket_401_maps_to_session_expired() {
        let server = HttpTestServer::spawn(vec![Route::ticket_status(StatusCode::UNAUTHORIZED)]).await;
        let client = AuthClient::new(None).unwrap();
        let err = client.mint_ticket(server.base_url()).await.unwrap_err();
        assert!(matches!(err, CoreError::SessionExpired));
    }

    #[tokio::test]
    async fn login_401_maps_to_invalid_credentials() {
        let server = HttpTestServer::spawn(vec![Route::login_status(StatusCode::UNAUTHORIZED)]).await;
        let client = AuthClient::new(None).unwrap();
        let err = client
            .login(server.base_url(), "u", "wrong")
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::InvalidCredentials));
    }

    #[tokio::test]
    async fn ticket_429_maps_to_rate_limited() {
        let server = HttpTestServer::spawn(vec![Route::ticket_status(StatusCode::TOO_MANY_REQUESTS)]).await;
        let client = AuthClient::new(None).unwrap();
        let err = client.mint_ticket(server.base_url()).await.unwrap_err();
        assert!(matches!(err, CoreError::RateLimited));
    }

    #[tokio::test]
    async fn login_404_maps_to_unknown_provider() {
        let server = HttpTestServer::spawn(vec![Route::login_status(StatusCode::NOT_FOUND)]).await;
        let client = AuthClient::new(None).unwrap();
        let err = client.login(server.base_url(), "u", "p").await.unwrap_err();
        assert!(matches!(err, CoreError::UnknownProvider));
    }

    #[tokio::test]
    async fn other_status_maps_to_http() {
        let server = HttpTestServer::spawn(vec![Route::ticket_status(StatusCode::SERVICE_UNAVAILABLE)]).await;
        let client = AuthClient::new(None).unwrap();
        let err = client.mint_ticket(server.base_url()).await.unwrap_err();
        assert!(matches!(err, CoreError::Http(503)));
    }

    #[tokio::test]
    async fn me_and_logout_round_trip() {
        let server = HttpTestServer::spawn(vec![
            Route::me_ok(),
            Route::logout_ok(),
        ]).await;
        let client = AuthClient::new(None).unwrap();
        let me = client.me(server.base_url()).await.unwrap();
        assert_eq!(json::str_at(&me, "username"), "hermo-test");
        client.logout(server.base_url()).await.unwrap();
        assert_eq!(server.count(RouteKind::Me), 1);
        assert_eq!(server.count(RouteKind::Logout), 1);
    }

    #[tokio::test]
    async fn transport_failure_maps_to_network() {
        // Nothing listens here: connect refused -> Network, not a panic.
        let client = AuthClient::new(None).unwrap();
        let err = client
            .status("http://127.0.0.1:1")
            .await
            .unwrap_err();
        assert!(matches!(err, CoreError::Network(_)));
    }
}
