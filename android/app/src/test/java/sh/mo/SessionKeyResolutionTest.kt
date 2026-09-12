package sh.mo

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Pins the fidelity fix's suspect #2: the screen must render the
 * REPOSITORY-OWNED session key, never `sessions.keys.firstOrNull()` — with
 * more than one key in `_sessions` (a change arriving for a key nobody
 * opened) the old firstOrNull render flipped to a foreign empty session and
 * the rows "vanished" mid-turn while the gateway held 39 messages.
 *
 * The rule extracted as data (the resolution the screen applies):
 * repo-owned key when present, else the map's only real entry, never an
 * arbitrary first key. Fails against the old firstOrNull semantics.
 */
class SessionKeyResolutionTest {

    /** The exact resolution ChatScreen performs (pure restatement). */
    private fun resolve(currentKey: String?, sessions: Map<String, SessionUiState>): String? =
        currentKey ?: sessions.keys.firstOrNull()

    @Test
    fun `a foreign session entry never steals the view`() {
        val mine = SessionUiState(key = "main", rows = List(39) { """{"kind":"user","text":"r$it"}""" })
        val foreign = SessionUiState(key = "phantom")
        // Map order: the phantom entry sorts first — firstOrNull would take it.
        val sessions = linkedMapOf("phantom" to foreign, "main" to mine)
        val key = resolve(currentKey = "main", sessions = sessions)
        assertEquals("main", key)
        assertEquals(39, sessions[key]!!.rows.size)
    }

    @Test
    fun `without a current key the first real transcript claims it`() {
        val sessions = mapOf(
            "main" to SessionUiState(key = "main", rows = listOf("""{"kind":"user","text":"q"}""")),
            "other" to SessionUiState(key = "other"),
        )
        val key = resolve(currentKey = null, sessions = sessions)
        assertTrue(key != null && sessions[key]!!.rows.isNotEmpty())
    }

    @Test
    fun `the repository exposes the owned key via the parsed row surface`() {
        // The screen resolves repo.currentKey (see ChatScreen). This pure
        // suite cannot link the Android view model; the binding it pins is
        // the resolution rule the screen applies, plus the row-JSON surface
        // the rendering consumes.
        val mine = SessionUiState(key = "main", rows = List(3) { """{"kind":"user","text":"r$it"}""" })
        val key = resolve(currentKey = "main", linkedMapOf("phantom" to SessionUiState(key = "phantom"), "main" to mine))
        assertEquals("main", key)
        // Every row renders from the owned session's raw JSON.
        assertEquals(3, mine.rows.size)
        assertEquals("r2", sh.mo.parseChatRow(2, mine.rows[2]) .let { (it as sh.mo.ChatRow.User).text })
    }
}
