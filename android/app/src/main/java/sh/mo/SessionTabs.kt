package sh.mo

/**
 * T16b: the ordered set of LOCALLY OPEN session tabs plus the tab verbs —
 * pure Kotlin, no Android imports (the JVM suite links this file directly).
 *
 * This is the ENTITY behind the session tab strip. It is deliberately NOT
 * the gateway's `session.list`: the strip is the chats this device has open
 * (an ordered list the repository owns), and resuming anything else stays
 * on the T11 picker sheet.
 *
 * Each verb is a pure function returning the next [TabSet]; the repository
 * (adapter layer) applies the result and performs the core calls. Invariants
 * the tests pin:
 *  - `select` of a key outside the set is a no-op (never steals the screen);
 *  - `add` of an already-open key only SELECTS it — it never grows the list
 *    (the picker-already-open case; re-issuing `open_session` would rebuild
 *    the LiveSession and RESET-clear the transcript);
 *  - `close` of the last remaining tab is a no-op (the strip omits the ×
 *    instead — Desktop `pane-tab.tsx:53-59` makes a tab uncloseable the
 *    same way);
 *  - closing the current tab selects the LEFT neighbour, or the new first
 *    when index 0 closed — never `firstOrNull()` on a map.
 */
data class TabSet(
    val keys: List<String> = emptyList(),
    val current: String? = null,
)

/**
 * The T5 restart contract, as a plan the adapter executes: resume every
 * registry key in order (skipping failures), with [current] = the last-active
 * session when it is still in the registry, else the last registry key. An
 * empty registry yields an empty plan — the caller mints `open_session(null)`.
 */
data class RestorePlan(
    val resumeKeys: List<String>,
    val current: String?,
)

/** Switch to an open tab. A key not in the set changes nothing. */
fun TabSet.select(key: String): TabSet =
    if (key in keys) copy(current = key) else this

/**
 * Open a tab: a new key appends at the end and becomes current; a key that
 * is already open only becomes current (list length unchanged).
 */
fun TabSet.add(key: String): TabSet =
    if (key in keys) copy(current = key)
    else copy(keys = keys + key, current = key)

/**
 * Close a tab. No-ops: unknown key, or the last remaining tab (uncloseable).
 * Closing the current tab selects the left neighbour, or the new first when
 * the closed tab was index 0; the remaining order is preserved.
 */
fun TabSet.close(key: String): TabSet {
    val index = keys.indexOf(key)
    if (index < 0 || keys.size <= 1) return this
    val remaining = keys.filterIndexed { i, _ -> i != index }
    val nextCurrent = when {
        current != key -> current
        index > 0 -> remaining[index - 1]
        else -> remaining.first()
    }
    return TabSet(keys = remaining, current = nextCurrent)
}

/** The launch/connect restore plan — see [RestorePlan]. */
fun restorePlan(keys: List<String>, lastActive: String?): RestorePlan =
    RestorePlan(
        resumeKeys = keys,
        current = lastActive?.takeIf { it in keys } ?: keys.lastOrNull(),
    )
