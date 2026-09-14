package sh.mo

/**
 * Error → human message mapping for the UI (one language, consistent:
 * PLAN §4 T6 item "Errors must land in the UI as text"). Pure Kotlin: the
 * core error arrives at the FFI boundary as a `Throwable` whose message is
 * the Rust `#[error(...)]` string; we classify on the UniFFI exception
 * CLASS first (T11: several variants carry an EMPTY message — the empty-jar
 * dead-end — so the message alone cannot be trusted), then on the stable
 * Rust message substrings.
 */
object ErrorMessages {

    /**
     * Map any Throwable from the core bindings to a one-line UI message.
     * The UniFFI class name is matched on `t::class` when it is a generated
     * `CoreException` (no UniFFI import here — the pure suite links this
     * file), so the JVM test drives it with the real class names.
     */
    fun of(t: Throwable): String = when {
        // T11: class-first. `SessionExpired` / `InvalidCredentials` carry an
        // empty message — the string checks below would never see them.
        t.classNameEndsWith("SessionExpired") -> "Session expired — enter the password again."
        t.classNameEndsWith("InvalidCredentials") -> "Wrong username or password."
        t.classNameEndsWith("UpgradeRejected") -> "The gateway rejected the connection (host/origin guard)."
        t.classNameEndsWith("RateLimited") -> "Too many attempts — wait a moment and retry."
        t.classNameEndsWith("NotConnected") -> "Not connected to the gateway yet."

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

    /**
     * True when the Throwable's simple class name ends with [suffix] — the
     * generated UniFFI `CoreException` nested classes read `SessionExpired`
     * at runtime. `javaClass.simpleName` on a nested Kotlin class is the
     * bare name (`SessionExpired`), so a suffix match survives both the
     * JVM and Android class layouts.
     */
    private fun Throwable.classNameEndsWith(suffix: String): Boolean =
        javaClass.simpleName.endsWith(suffix)

    /**
     * T11: is this failure auth-shaped? True for the UniFFI classes whose
     * fix is re-login (`SessionExpired` — the empty cookie jar, a 401 on a
     * gated route, `InvalidCredentials`). A transport failure is NOT
     * auth-shaped even when its message is empty. Pure Kotlin, so the JVM
     * suite drives the same classifier the repository runs.
     */
    fun isAuthShape(t: Throwable): Boolean =
        t.classNameEndsWith("SessionExpired") ||
            t.classNameEndsWith("InvalidCredentials") ||
            t.classNameEndsWith("UpgradeRejected")
}
