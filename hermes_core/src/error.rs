//! Domain-facing error enum (entity layer — PLAN.md §4 T3 item 3).
//!
//! `CoreError` is the error type the use-case layer (T5) and the FFI
//! boundary speak. It is framework-free: nothing in it mentions `reqwest`,
//! `tungstenite` or any other adapter type — adapter errors are *mapped*
//! into it. It is shaped for `#[derive(uniffi::Error)]` (flat variants,
//! primitive fields) but is not yet exported at the FFI boundary: no
//! exported interface references it in this stream, so the derive would
//! only add unused scaffolding.
//!
//! Dependency Rule (PR #3 finding 6): the entity must not import adapter
//! types, so the `From<RpcError>` / `From<ClientError>` impls live next to
//! the types that define them (`rpc::frames`, `rpc::client`) — an impl can
//! always live on either side of the arrow.

use thiserror::Error;

/// Every error the core can surface to a foreign caller.
#[derive(Debug, Clone, Error)]
pub enum CoreError {
    /// The `hermes://connect` payload was malformed (wrong scheme,
    /// unknown version, missing url, bad url scheme).
    #[error("invalid QR payload")]
    InvalidQr,
    /// The operation needs a live gateway connection.
    #[error("not connected")]
    NotConnected,
    /// A bounded operation (RPC call, HTTP request) ran out of time.
    #[error("operation timed out")]
    Timeout,
    /// The gateway answered a JSON-RPC error object.
    #[error("rpc error {code}: {message}")]
    Rpc { code: i64, message: String },
    /// Local filesystem failure (cookie jar, session registry).
    #[error("io error: {0}")]
    Io(String),
    /// Transport-level failure of an outgoing network call.
    #[error("network failure: {0}")]
    Network(String),
    /// 401 on `password-login` — wrong username or password.
    #[error("invalid credentials")]
    InvalidCredentials,
    /// 401 on a cookie-authenticated call — re-login required.
    #[error("session expired — re-login required")]
    SessionExpired,
    /// 403 on the WebSocket handshake — the gateway rejected the upgrade
    /// (auth/host/origin guard, PLAN §1.1: pre-accept rejections surface as
    /// HTTP 403, the client never sees a close frame). Deliberately NOT
    /// [`CoreError::Network`]: the app must not retry as if it were offline,
    /// and the UI must not parse `Network(String)` (PR #3 finding 4).
    #[error("upgrade rejected by gateway")]
    UpgradeRejected,
    /// A stored endpoint URL is malformed (not a QR problem — that is
    /// [`CoreError::InvalidQr`]).
    #[error("invalid endpoint url: {0}")]
    InvalidEndpoint(String),
    /// 429 — the auth backend is rate limiting.
    #[error("rate limited")]
    RateLimited,
    /// 404 on `password-login` — unknown auth provider.
    #[error("unknown auth provider")]
    UnknownProvider,
    /// Any other non-success HTTP status.
    #[error("http status {0}")]
    Http(u16),
}

impl From<std::io::Error> for CoreError {
    fn from(e: std::io::Error) -> Self {
        CoreError::Io(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The From<…> for CoreError impls moved next to the types that define
    // them (rpc/frames.rs, rpc/client.rs) — PR #3 finding 6 (Dependency
    // Rule). The conversion behaviour they pin is unchanged.

    #[test]
    fn no_reqwest_or_tungstenite_type_in_display_paths() {
        // Framework-free rule: the enum must format without any adapter
        // type in scope. Compile-time proof is the file's import list;
        // this pins the display strings the FFI boundary will show.
        assert_eq!(CoreError::InvalidQr.to_string(), "invalid QR payload");
        assert_eq!(CoreError::SessionExpired.to_string(), "session expired — re-login required");
    }
}
