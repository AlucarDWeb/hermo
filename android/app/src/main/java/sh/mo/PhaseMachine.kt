package sh.mo

/**
 * Pure reduction of connection/error events to the app phase (PLAN §4 T6
 * item 5: transitions come from the repository's flows, never inferred in
 * the composable). No Android imports; [PhaseMachineTest] pins the exact
 * sequence.
 */
object PhaseMachine {

    /** A connection event delivered by the core through the EventSink. */
    sealed interface ConnEvent {
        data object Connecting : ConnEvent
        data object Open : ConnEvent
        data class Closed(val reason: String) : ConnEvent
        data object NeedsPassword : ConnEvent

        /**
         * T11: a repository call failed with an auth-shaped error (the
         * cookie jar is empty/expired — `CoreError::SessionExpired` class,
         * 401, `InvalidCredentials`). Matched on the CLASS, never on a
         * substring of `t.message` (the UniFFI `message` is often EMPTY —
         * the empty-jar dead-end).
         */
        data object AuthFailed : ConnEvent

        /**
         * The session header arrived or changed (`session.info` / `session.title`).
         * It commonly lands AFTER `open_session`, so a Ready phase must be
         * refreshed instead of staying on an empty/stale model name.
         */
        data class Header(val model: String) : ConnEvent
    }

    /**
     * One transition step. `phase` is the current phase, `savedEndpoint` the
     * display string of the paired endpoint (empty when unpaired), and
     * `hasLiveSession` whether a transcript session was open (decides the
     * [AppPhase.NeedsPassword] overlay flag).
     */
    fun reduce(phase: AppPhase, event: ConnEvent, savedEndpoint: String, hasLiveSession: Boolean = false): AppPhase =
        when (event) {
            ConnEvent.Connecting -> AppPhase.Connecting
            ConnEvent.Open ->
                // Open alone does not mean Ready: the first open_session lands
                // after. Keep Connecting until the session comes up.
                if (phase is AppPhase.Ready) phase else AppPhase.Connecting
            is ConnEvent.Closed ->
                // Auth-style closes (session expiry, deliberate logout) ask for
                // the password; anything else is a connection loss -> Offline.
                when {
                    savedEndpoint.isEmpty() -> AppPhase.Unpaired
                    event.reason.contains("user logout") || event.reason.contains("session expired") ->
                        AppPhase.NeedsPassword(savedEndpoint, hasLiveSession)
                    else -> AppPhase.Offline(event.reason)
                }
            ConnEvent.NeedsPassword ->
                if (savedEndpoint.isEmpty()) AppPhase.Unpaired
                else AppPhase.NeedsPassword(savedEndpoint, hasLiveSession)
            ConnEvent.AuthFailed ->
                when {
                    savedEndpoint.isEmpty() -> AppPhase.Unpaired
                    // The class-matched auth failure: NeedsPassword with the
                    // overlay when a transcript was open, never Offline.
                    else -> AppPhase.NeedsPassword(savedEndpoint, hasLiveSession)
                }
            is ConnEvent.Header ->
                // Only a Ready phase is refreshed, and an empty model never wipes
                // the one already shown.
                if (phase is AppPhase.Ready && event.model.isNotEmpty()) AppPhase.Ready(event.model) else phase
        }

    /** `open_session` succeeded: Ready with the session header's model. */
    fun ready(model: String): AppPhase = AppPhase.Ready(model)

    /** Terminal network failure surfaced by a repository call. */
    fun offline(reason: String): AppPhase = AppPhase.Offline(reason)
}
