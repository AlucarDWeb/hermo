package sh.mo

/**
 * Error → human message mapping for the UI (one language, consistent:
 * PLAN §4 T6 item "Errors must land in the UI as text"). Pure Kotlin: the
 * core error arrives at the FFI boundary as a `Throwable` whose message is
 * the Rust `#[error(...)]` string; we classify on those stable messages and
 * on the UniFFI class names, never by parsing details.
 */
object ErrorMessages {

    /** Map any Throwable from the core bindings to a one-line UI message. */
    fun of(t: Throwable): String = when {
        t.message?.contains("invalid QR payload") == true -> "That pairing payload is not valid."
        t.message?.contains("invalid endpoint url") == true -> "That gateway URL is not valid."
        t.message?.contains("invalid credentials") == true -> "Wrong username or password."
        t.message?.contains("session expired") == true -> "Session expired — enter the password again."
        t.message?.contains("rate limited") == true -> "Too many attempts — wait a moment and retry."
        t.message?.contains("upgrade rejected") == true -> "The gateway rejected the connection (host/origin guard)."
        t.message?.contains("not connected") == true -> "Not connected to the gateway yet."
        t.message?.contains("operation timed out") == true -> "The gateway did not answer in time."
        t.message?.contains("network failure") == true -> "Network error — check the connection."
        t.message?.contains("unknown auth provider") == true -> "The gateway has no password auth enabled."
        else -> "Error: ${t.message ?: t::class.simpleName ?: "unknown"}"
    }
}
