package sh.mo

/**
 * App phases (PLAN §4 T6 item 5), pure Kotlin — no Android imports so the
 * plain `kt_jvm_test` can link it. Transitions are produced exclusively by
 * [PhaseMachine] from repository events; the composable only renders.
 */
sealed interface AppPhase {
    /** No gateway paired yet. */
    data object Unpaired : AppPhase

    /**
     * Paired endpoint known, but no valid cookie jar: password required.
     *
     * `overlay` (T11): the transcript already had a live session (the ask
     * arrived mid-session — cookie expiry, 401, session kill) — the sheet
     * must open OVER the existing transcript, never blank it. False on the
     * first pair (Unpaired → NeedsPassword), where there is nothing under.
     */
    data class NeedsPassword(val endpoint: String, val overlay: Boolean = false) : AppPhase

    /** connect()/login() in flight. */
    data object Connecting : AppPhase

    /** Live: at least one session open, model name in [model]. */
    data class Ready(val model: String) : AppPhase

    /** The socket died with a non-auth reason (banner + retry). */
    data class Offline(val reason: String) : AppPhase
}
