package sh.mo

/**
 * T10b composer slash policy (PI_TASK_T10B "Behaviour", closed decisions),
 * pure Kotlin — no Android imports, JVM-tested. The composable renders what
 * it is handed; the view model and repository call these answers.
 */
object SlashPolicy {

    /** Commands the phone answers locally, never `core.send` / `run_slash`. */
    private val LOCAL_COMMANDS = setOf("/clear", "/sessions", "/quit")

    /**
     * Decision 1: complete on a leading `/` with NO space (the argument stage
     * is out of scope). Empty draft hides.
     */
    fun shouldComplete(draft: String): Boolean =
        draft.startsWith("/") && !draft.contains(' ')

    /**
     * Decision 2: a picked row rewrites the draft from `replaceFrom` (1 when
     * `-1`/absent) and adds a trailing space, then the popup hides.
     * `replaceFrom` is the 1-based character position the replacement starts
     * at (Desktop's use-slash-completions.ts: `text.slice(0, replaceFrom)` is
     * the kept prefix, so the kept length is `replaceFrom - 1`).
     */
    fun insertCompletion(draft: String, itemText: String, replaceFrom: Long): String {
        val from = if (replaceFrom < 1) 1L else replaceFrom
        val keepEnd = (from - 1).toInt().coerceIn(0, draft.length)
        return draft.take(keepEnd) + itemText + " "
    }

    /**
     * Decision 3: `/clear` `/sessions` `/quit` are local. The match is the
     * whole token — `/clears` or `/clear now` are NOT the local command.
     */
    fun isLocalCommand(draft: String): Boolean = draft.trim() in LOCAL_COMMANDS

    /** Decision 3: a leading `/` routes the submit through the slash ladder. */
    fun isSlashSubmit(draft: String): Boolean = draft.startsWith("/")
}

/** One popup row: the DTO fields the phone renders (T10b decision 2). */
data class SlashCompletionRow(
    val text: String,
    val display: String,
    val kind: String,
    val meta: String,
)
