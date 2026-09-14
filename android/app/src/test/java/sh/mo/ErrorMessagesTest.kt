package sh.mo

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Pins the error→message mapping on the STABLE Rust error strings (the
 * UniFFI Throwable messages come straight from `#[error(...)]`). If a
 * variant's message changes or the mapping is removed, these fail.
 */
class ErrorMessagesTest {

    private fun err(msg: String): Throwable = RuntimeException(msg)

    @Test
    fun `invalid qr`() {
        assertEquals("That pairing payload is not valid.", ErrorMessages.of(err("invalid QR payload")))
    }

    @Test
    fun `invalid credentials`() {
        assertEquals("Wrong username or password.", ErrorMessages.of(err("invalid credentials")))
    }

    @Test
    fun `rate limited`() {
        assertEquals("Too many attempts — wait a moment and retry.", ErrorMessages.of(err("rate limited")))
    }

    @Test
    fun `session expired`() {
        assertEquals(
            "Session expired — enter the password again.",
            ErrorMessages.of(err("session expired — re-login required")),
        )
    }

    @Test
    fun `upgrade rejected`() {
        assertEquals(
            "The gateway rejected the connection (host/origin guard).",
            ErrorMessages.of(err("upgrade rejected by gateway")),
        )
    }

    @Test
    fun `network`() {
        assertEquals("Network error — check the connection.", ErrorMessages.of(err("network failure: boom")))
    }

    @Test
    fun `invalid endpoint`() {
        assertEquals("That gateway URL is not valid.", ErrorMessages.of(err("invalid endpoint url: nope")))
    }

    @Test
    fun `unknown error keeps detail`() {
        assertEquals("Error: mystery", ErrorMessages.of(err("mystery")))
    }

    // Named like the UniFFI CoreException nested classes so
    // classNameEndsWith("SessionExpired") hits. Message is EMPTY — the
    // empty-jar shape. A transport Throwable with an empty message must
    // NOT look like auth.
    private class SessionExpired : RuntimeException("")
    private class InvalidCredentials : RuntimeException("")
    private class UpgradeRejected : RuntimeException("")
    private class Timeout : RuntimeException("")

    @Test
    fun `empty-message SessionExpired is auth-shaped`() {
        assertEquals(true, ErrorMessages.isAuthShape(SessionExpired()))
        assertEquals(
            "Session expired — enter the password again.",
            ErrorMessages.of(SessionExpired()),
        )
    }

    @Test
    fun `empty-message transport is not auth-shaped`() {
        assertEquals(false, ErrorMessages.isAuthShape(Timeout()))
        assertEquals(false, ErrorMessages.isAuthShape(RuntimeException("")))
        assertEquals(true, ErrorMessages.isAuthShape(InvalidCredentials()))
        assertEquals(true, ErrorMessages.isAuthShape(UpgradeRejected()))
    }
}
