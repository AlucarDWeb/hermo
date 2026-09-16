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
 *  - closing the LAST remaining tab EMPTIES the set — a declared
 *    divergence from Desktop `pane-tab.tsx:53-59` (there the last tab is
 *    uncloseable); the adapter mints a fresh blank chat after an empty
 *    close, so the user can always leave the current session;
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
 * Close a tab. No-ops: unknown key only. Closing the LAST remaining tab
 * empties the set (the adapter mints a fresh chat after an empty close, so
 * the user can always leave the current session). Closing the
 * current tab selects the left neighbour, or the new first when the closed
 * tab was index 0; the remaining order is preserved.
 */
fun TabSet.close(key: String): TabSet {
    val index = keys.indexOf(key)
    if (index < 0) return this
    val remaining = keys.filterIndexed { i, _ -> i != index }
    val nextCurrent = when {
        current != key -> current
        index > 0 -> remaining[index - 1]
        else -> remaining.firstOrNull()
    }
    return TabSet(keys = remaining, current = nextCurrent)
}

/** The launch/connect restore plan — see [RestorePlan]. */
fun restorePlan(keys: List<String>, lastActive: String?): RestorePlan =
    RestorePlan(
        resumeKeys = keys,
        current = lastActive?.takeIf { it in keys } ?: keys.lastOrNull(),
    )

/**
 * FIX7 (review 5214081721, finding 2): the launch's FIRST decision, pure so
 * the JVM suite can pin it. A failed `open_sessions()` RPC is NOT an empty
 * registry — the pre-fix adapter conflated the two, swallowed the error and
 * fell through to `open_session(null)`: a brand-new session minted on every
 * failed launch. Only a SUCCESSFUL read of an empty registry may reach the
 * mint path (the legitimate fresh-install path).
 */
sealed interface LaunchStep {
    /** The registry list failed: surface the error, open NOTHING. */
    object ListFailed : LaunchStep

    /** The registry list succeeded: run the plan (empty → the caller mints). */
    data class RunPlan(val plan: RestorePlan) : LaunchStep
}

/**
 * @param registryKeys the `open_sessions()` keys, or `null` when the RPC
 *   FAILED (null is failure, an empty list is a genuinely empty registry).
 */
fun launchStep(registryKeys: List<String>?, lastActive: String?): LaunchStep =
    if (registryKeys == null) LaunchStep.ListFailed
    else LaunchStep.RunPlan(restorePlan(registryKeys, lastActive))

/**
 * FIX7 (finding 3): the strip-tap policy — `null` when the tap must do
 * NOTHING. The pre-fix `switchTab` fell through to `afterOpen` even when the
 * tapped key was already current: a header RPC re-issued and the phase
 * machine churned on every re-tap. A key outside the set stays a no-op too
 * (`select`'s invariant — a stale tap cannot steal the screen).
 */
fun tabTap(tabs: TabSet, currentKey: String?, key: String): TabSet? =
    // FIX7-r2 nit 2: explicit predicate instead of data-class equality —
    // self-documenting and immune to any future normalization in `select`.
    if (key == currentKey || key !in tabs.keys) null else tabs.select(key)
