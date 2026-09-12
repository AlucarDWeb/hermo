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
    fun `reset clears the rows and the app replays its snapshot - empty is the alternative`() {
        val snapshot = appends("""{"kind":"user","text":"q"}""", assistantJson)
        // RESET honours the clear…
        var rows = applyTranscriptChange(snapshot, "reset", Long.MAX_VALUE, "")
        assertEquals(0, rows.size)
        // …and the repository replays its last-known snapshot back in, because
        // the core emits this Reset WITHOUT the rebuilt rows (`reducer.rs:235`
        // returns exactly one Reset and `core.rs::resume_all` forwards only that
        // DTO; the bridge has no rows pull), so clearing alone would leave the
        // resume path showing an empty transcript. This is a mitigation, not the
        // fix: the fix is core-side (see PLAN "RESET/resume contract").
        snapshot.forEachIndexed { i, json ->
            rows = applyTranscriptChange(rows, "rowAppended", i.toLong(), json)
        }
        assertEquals(snapshot, rows)
    }

    @Test
    fun `a replayed append does not duplicate its row`() {
        val rows = appends("""{"kind":"user","text":"q"}""")
        val again = applyTranscriptChange(rows, "rowAppended", 0, """{"kind":"user","text":"q"}""")
        assertEquals(1, again.size)
    }
}
