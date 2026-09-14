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
    /** True while the session's turn is streaming (SessionSummary.running). */
    val running: Boolean = false,
) {
    fun withRows(rows: List<String>) = copy(rows = rows)
}

/**
 * One row of the T11 session picker (the phone sheet over `session.list`):
 * `RemoteSessionDto` flattened into a pure value the sheet renders. Pure
 * Kotlin so the JVM suite can pin the title/preview fallbacks.
 */
data class RemoteSessionRow(
    val id: String,
    val title: String,
    val preview: String,
    val messageCount: Long,
) {
    /** Titlebar copy: the session's title, or "New session" when unnamed. */
    val displayTitle: String get() = title.ifBlank { "New session" }

    /** The one-line preview, blank when the session has no text yet. */
    val displayPreview: String get() = preview.trim()
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
            // T7c: the Reset clears the transcript and nothing app-side
            // replays a snapshot — the rebuilt rows arrive right after as
            // RowAppended changes from the core (the stream is the single
            // source of rows).
            emptyList()
        }
        else -> rows // headerUpdated: no row change
    }
}

/**
 * Incremental parse of the raw row JSON into [ChatRow]s.
 *
 * Two things this exists for.
 *
 * Identity: a row's id is its TRANSCRIPT index, not its position after the
 * empty-row filter. `applyTranscriptChange` pads gaps with empty strings, so a
 * filtered position shifts for every row after a gap the moment that gap is
 * filled — and the transcript list keys on the id, so every later row looked
 * like a brand new item: scroll position jumped and per-row state (expanded
 * tool cards, thinking disclosures) reset mid-turn.
 *
 * Cost: a streaming turn rewrites ONE row's JSON per delta while recomposition
 * happens for unrelated reasons too (the elapsed-time tick, IME insets, focus).
 * Re-parsing the whole transcript each time is O(rows) of `JSONObject` per
 * token; reusing the previous parse for every row whose JSON is unchanged
 * makes it O(changed).
 */
class TranscriptRows {
    private var lastRaw: List<String> = emptyList()
    private var lastParsed: List<ChatRow?> = emptyList()
    private var lastVisible: List<ChatRow> = emptyList()

    fun of(raw: List<String>): List<ChatRow> {
        // Reference equality, not structural: the same instance means nothing
        // moved, and a structural compare would cost the per-row string
        // comparison the incremental path below already does.
        if (raw === lastRaw) return lastVisible
        val parsed = ArrayList<ChatRow?>(raw.size)
        for (index in raw.indices) {
            val json = raw[index]
            val reusable = index < lastRaw.size && lastRaw[index] == json
            parsed.add(
                when {
                    reusable -> lastParsed[index]
                    json.isEmpty() -> null
                    else -> parseChatRow(index, json)
                },
            )
        }
        lastRaw = raw
        lastParsed = parsed
        lastVisible = parsed.filterNotNull()
        return lastVisible
    }
}
