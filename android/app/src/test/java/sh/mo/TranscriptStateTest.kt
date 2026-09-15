package sh.mo

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Pins the transcript-fidelity fix (PI_TASK_T7A deliverable 3).
 *
 * The defect on the device: during a turn the transcript collapsed — tool/
 * assistant rows visible one moment, one row seconds later, while the
 * gateway held 39 messages. Root cause, app-side (the change stream is the
 * app's only transcript source: the core emits `Reset` on the resume/epoch
 * rebuild but does NOT publish the rebuilt rows — `reducer.rs:235` returns
 * exactly one Reset, `core.rs::resume_all` forwards only that DTO, and the
 * bridge exposes no rows pull):
 *
 * - ROW_UPDATED carrying an index the local list did not reach yet was
 *   DROPPED (GatewayRepository.onTranscriptChange's `if (idx in rows.indices)
 *   rows[idx] = …` else nothing). The stream keeps indices contiguous, so
 *   from that moment every later index points at the wrong row: rows
 *   overwrite each other and the visible list collapses mid-turn.
 *
 * These tests fail against the old semantics (drop / blind overwrite) and
 * pass with `applyTranscriptChange`.
 */
class TranscriptStateTest {

    private fun appends(vararg jsons: String): List<String> {
        var rows = emptyList<String>()
        jsons.forEachIndexed { i, json ->
            rows = applyTranscriptChange(rows, "rowAppended", i.toLong(), json)
        }
        return rows
    }

    private val toolJson =
        """{"kind":"tool","tool_id":"t1","name":"terminal","complete":true,"args":{},"result":"ok","duration_s":0.4}"""
    private val assistantJson = """{"kind":"assistant","text":"hi","streaming":false}"""

    @Test
    fun `rowUpdated past the end pads and lands at the right index`() {
        val rows = appends("""{"kind":"user","text":"q"}""", assistantJson)
        // A tool card updates at index 2 while the list holds 2 rows (0,1):
        // the old code dropped it — the pinned fix pads and keeps the index.
        val out = applyTranscriptChange(rows, "rowUpdated", 2, toolJson)
        assertEquals(3, out.size)
        assertEquals(toolJson, out[2])
        assertEquals("""{"kind":"user","text":"q"}""", out[0])
    }

    @Test
    fun `a mid-turn collapse scenario keeps every row addressable`() {
        // Stream shape of a real turn: user row, tool updates racing ahead of
        // an append, assistant streaming updates. Every index must land.
        var rows = appends("""{"kind":"user","text":"do it"}""")
        rows = applyTranscriptChange(rows, "rowAppended", 1, toolJson)
        rows = applyTranscriptChange(rows, "rowUpdated", 2, """{"kind":"thinking","text":"hmm"}""")
        rows = applyTranscriptChange(rows, "rowUpdated", 3, assistantJson)
        rows = applyTranscriptChange(rows, "rowUpdated", 1, toolJson) // re-complete
        assertEquals(4, rows.size)
        assertTrue(rows[3].contains("\"assistant\""))
        assertTrue(rows[1].contains("\"tool\""))
    }

    @Test
    fun `reset clears the rows and the core's RowAppendeds rebuild them`() {
        val coreRows = listOf("""{"kind":"user","text":"q"}""", assistantJson)
        // RESET honours the clear…
        var rows = applyTranscriptChange(coreRows, "reset", Long.MAX_VALUE, "")
        assertEquals(0, rows.size)
        // …and T7c: the rebuilt rows arrive right after as one RowAppended
        // each FROM THE CORE (`ingest_resume_messages` emits Reset first, then
        // a RowAppended per row). No app-side snapshot replay — the app used
        // to replay its own last-known rows here, which duplicated whatever
        // the core now delivers. Rebuild from the core's row JSON only.
        coreRows.forEachIndexed { i, json ->
            rows = applyTranscriptChange(rows, "rowAppended", i.toLong(), json)
        }
        assertEquals(coreRows, rows)
    }

    @Test
    fun `a replayed append does not duplicate its row`() {
        val rows = appends("""{"kind":"user","text":"q"}""")
        val again = applyTranscriptChange(rows, "rowAppended", 0, """{"kind":"user","text":"q"}""")
        assertEquals(1, again.size)
    }

    // FIX7 (review #19 finding 1): the map may only grow for a key the
    // repository considers open-or-restoring. The resume race itself is
    // covered by the ADAPTER, which seeds pendingKeys BEFORE the core call —
    // the pure function only sees the resulting knownKeys set.

    @Test
    fun `applySessionChange keeps resume rows for a restoring key`() {
        val user = """{"kind":"user","text":"pong"}"""
        var sessions = emptyMap<String, SessionUiState>()
        sessions = applySessionChange(sessions, "k", "reset", 0, "", knownKeys = setOf("k"))
        sessions = applySessionChange(sessions, "k", "rowAppended", 0, user, knownKeys = setOf("k"))
        assertEquals(listOf(user), sessions.getValue("k").rows)
    }

    @Test
    fun `applySessionChange drops events for a key outside knownKeys`() {
        // Pins FIX7 finding 1: the pre-fix code minted a SessionUiState for
        // EVERY arriving key, so a late change for a key closed via closeTab
        // resurrected the phantom entry. Both events below target "k", which
        // is not in knownKeys — the map must stay empty.
        val user = """{"kind":"user","text":"pong"}"""
        var sessions = applySessionChange(emptyMap(), "k", "reset", 0, "", knownKeys = setOf("open"))
        assertTrue(sessions.isEmpty())
        sessions = applySessionChange(sessions, "k", "rowAppended", 0, user, knownKeys = setOf("open"))
        assertTrue(sessions.isEmpty())
    }

    @Test
    fun `applySessionChange drops headerUpdated for an unknown key`() {
        // Pins FIX7 finding 4: a header for an unknown key must not mint an
        // empty entry either (same gate as finding 1).
        val sessions = applySessionChange(emptyMap(), "k", "headerUpdated", 0, "", knownKeys = setOf("open"))
        assertTrue(sessions.isEmpty())
    }

    @Test
    fun `applySessionChange headerUpdated with a title updates SessionUiState title`() {
        // Pins FIX8 item 5: the core's HeaderUpdated DTO now carries the
        // auto-titling rename (core FIX8 A1), and the headerUpdated branch
        // copies it into the session state. BROKEN behaviour pinned here:
        // the pre-FIX8 branch returned the entry UNCHANGED, so the tab and
        // the titlebar stayed "New session" forever — the app never saw
        // the rename the core had already applied.
        var sessions = applySessionChange(emptyMap(), "k", "rowAppended", 0, "{\"kind\":\"user\",\"text\":\"q\"}", knownKeys = setOf("k"))
        sessions = applySessionChange(
            sessions, "k", "headerUpdated", 0, "", knownKeys = setOf("k"),
            title = "my chat",
        )
        assertEquals("my chat", sessions.getValue("k").title)
    }

    @Test
    fun `applySessionChange headerUpdated with an empty title keeps the old one`() {
        // Pins FIX8 item 5's blank guard: an empty title must never BLANK a
        // known one (a header event without the rename must be a no-op on
        // the title). BROKEN behaviour pinned here: an unguarded
        // `copy(title = title)` would blank the tab label whenever the core
        // delivered a headerUpdated without a title.
        var sessions = applySessionChange(
            emptyMap(), "k", "headerUpdated", 0, "", knownKeys = setOf("k"),
            title = "my chat",
        )
        sessions = applySessionChange(sessions, "k", "headerUpdated", 0, "", knownKeys = setOf("k"), title = "")
        assertEquals("my chat", sessions.getValue("k").title)
    }

    @Test
    fun `applySessionChange headerUpdated leaves the row list untouched`() {
        // Headers are not rows: the title change must not disturb the
        // transcript the change stream built.
        val user = """{"kind":"user","text":"q"}"""
        var sessions = applySessionChange(emptyMap(), "k", "rowAppended", 0, user, knownKeys = setOf("k"))
        sessions = applySessionChange(
            sessions, "k", "headerUpdated", 0, "", knownKeys = setOf("k"),
            title = "my chat",
        )
        assertEquals(listOf(user), sessions.getValue("k").rows)
    }

    @Test
    fun `ensureSession does not wipe rows already applied`() {
        val user = """{"kind":"user","text":"pong"}"""
        var sessions = applySessionChange(emptyMap(), "k", "rowAppended", 0, user, knownKeys = setOf("k"))
        sessions = ensureSession(sessions, "k")
        assertEquals(listOf(user), sessions.getValue("k").rows)
        sessions = ensureSession(sessions, "other")
        assertEquals(listOf(user), sessions.getValue("k").rows)
        assertEquals(emptyList<String>(), sessions.getValue("other").rows)
    }
}
