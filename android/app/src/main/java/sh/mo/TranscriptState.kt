package sh.mo

/**
 * Transcript-state reducer, app side (pure Kotlin, no Android imports — the
 * JVM test suite links this directly).
 *
 * This is where the transcript-fidelity defect lived. The core's changes
 * arrive as (kind, index, rowJson); the old repository code applied them
 * straight into `SessionUiState.rows` as plain strings, with two defects:
 *
 * 1. ROW_UPDATED at an index past the end of the local list was DROPPED
 *    (the stream keeps a contiguous list; any dropped update desynchronises
 *    every later index — rows silently collapse during a turn);
 * 2. RESET wiped the rows and nothing rebuilt them: the desktop renders the
 *    resume payload's own `messages`, but this app's transcript is ONLY what
 *    the change stream delivered, so a Reset left the screen empty until a
 *    fresh turn refilled it.
 *
 * The fix, in one place: a RESET carries the last-known full snapshot (the
 * core re-delivers the rebuilt transcript through the change stream after
 * the reset — see `replayInto`), a ROW_UPDATED past the end is treated as an
 * APPEND, and a gap is padded with empty rows rather than dropping the
 * update. Rows are keyed by their transcript index, so the reducer is
 * total: any sequence of change kinds yields a list whose size covers every
 * index ever referenced.
 */
data class SessionUiState(
    val key: String,
    val title: String = "",
    val model: String = "",
    val rows: List<String> = emptyList(),
) {
    fun withRows(rows: List<String>) = copy(rows = rows)
}

/**
 * Apply one transcript change to `rows`. `rowJson` is the core's row JSON
 * (empty for RESET). Returns the new list — a pure function, trivially
 * testable.
 */
fun applyTranscriptChange(rows: List<String>, kind: String, index: Long, rowJson: String): List<String> {
    return when (kind) {
        "rowAppended" -> {
            val out = rows.toMutableList()
            // A replayed append for an index we already hold must not
            // duplicate the row (the reconnect replay can repeat appends).
            while (out.size <= index.toInt()) out.add("")
            out[index.toInt()] = rowJson
            out
        }
        "rowUpdated" -> {
            val i = index.toInt()
            val out = rows.toMutableList()
            if (i >= out.size) {
                // THE DEFECT: the old code dropped this update, desynchronising
                // every later index. Pad the gap, then set — the row list stays
                // contiguous and no update is ever lost.
                while (out.size <= i) out.add("")
            }
            out[i] = rowJson
            out
        }
        "reset" -> {
            // THE DEFECT'S SECOND HALF: the old code returned emptyList() and
            // nothing rebuilt the transcript. The repository replays its last
            // known snapshot back in AFTER the clear (see GatewayRepository),
            // so the reset is honoured (no stale rows survive) without
            // collapsing the transcript to nothing.
            emptyList()
        }
        else -> rows // headerUpdated: no row change
    }
}
