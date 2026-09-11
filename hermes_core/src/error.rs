//! Domain-facing error enum (entity layer — PLAN.md §4 T3 item 3).
//!
//! `CoreError` is the error type the use-case layer (T5) and the FFI
//! boundary speak. It is framework-free: nothing in it mentions `reqwest`,
//! `tungstenite` or any other adapter type — adapter errors are *mapped*
//! into it. It is shaped for `#[derive(uniffi::Error)]` (flat variants,
//! primitive fields) but is not yet exported at the FFI boundary: no
//! exported interface references it in this stream, so the derive would
//! only add unused scaffolding.

use thiserror::Error;

use crate::rpc::client::ClientError;
use crate::rpc::frames::RpcError;

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

impl From<RpcError> for CoreError {
    fn from(e: RpcError) -> Self {
        CoreError::Rpc {
            code: e.code,
            message: e.message,
        }
    }
}

impl From<ClientError> for CoreError {
    fn from(e: ClientError) -> Self {
        match e {
            ClientError::Rpc { code, message } => CoreError::Rpc { code, message },
            ClientError::Timeout(_) => CoreError::Timeout,
            ClientError::Closed(_) => CoreError::NotConnected,
            ClientError::Transport(s) => CoreError::Network(s),
        }
    }
}

impl From<std::io::Error> for CoreError {
    fn from(e: std::io::Error) -> Self {
        CoreError::Io(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_error_converts() {
        let e: CoreError = RpcError {
            code: 4009,
            message: "session busy".into(),
            data: serde_json::Value::Null,
        }
        .into();
        assert!(matches!(e, CoreError::Rpc { code: 4009, .. }));
    }

    #[test]
    fn client_error_maps_losslessly_where_it_matters() {
        let timeout: CoreError = ClientError::Timeout(std::time::Duration::from_secs(1)).into();
        assert!(matches!(timeout, CoreError::Timeout));
        let closed: CoreError = ClientError::Closed("heartbeat timeout".into()).into();
        assert!(matches!(closed, CoreError::NotConnected));
        let transport: CoreError = ClientError::Transport("reset".into()).into();
        assert!(matches!(transport, CoreError::Network(_)));
        let rpc: CoreError = ClientError::Rpc {
            code: 5000,
            message: "boom".into(),
        }
        .into();
        assert!(matches!(rpc, CoreError::Rpc { code: 5000, .. }));
    }

    #[test]
    fn no_reqwest_or_tungstenite_type_in_display_paths() {
        // Framework-free rule: the enum must format without any adapter
        // type in scope. Compile-time proof is the file's import list;
        // this pins the display strings the FFI boundary will show.
        assert_eq!(CoreError::InvalidQr.to_string(), "invalid QR payload");
        assert_eq!(CoreError::SessionExpired.to_string(), "session expired — re-login required");
    }
}
