package sh.mo

/**
 * T16c: the bot drawer's pure entity (no Android imports — the JVM suite
 * links this file directly), the same shape [TabSet] has for the strip.
 *
 * One drawer row is ONE Hermes profile (`GET /api/profiles` via the core's
 * `list_profiles`); a tap opens that profile's canonical Bot Chat through
 * the core's `open_bot_chat` (create-or-resume of `(profile, "Bot Chat")`).
 * The drawer shows ALL profiles the core returns — the wire has NO `hidden`
 * flag (T16c gap, declared in the PLAN): no filter is invented here, and if
 * hidden bots ever need hiding, that must come from the gateway, not from
 * this entity.
 */
data class BotDrawerRow(
    /** The profile name — the key `open_bot_chat` takes. */
    val name: String,
    /** Supporting line 1 (skipped by the UI when empty). */
    val model: String,
    /** Supporting line 2 (skipped by the UI when empty). */
    val description: String,
    /**
     * The profile the verb is called with. Deliberately separate from
     * [name]: if a display label ever diverges from the profile key, the
     * tap must still carry the KEY — a second identity can never be minted
     * client-side (create-or-resume is the core verb's job).
     */
    val profile: String,
)

/**
 * The drawer's state (loading / rows / error), reduced by [onProfilesLoaded]
 * and [onRetry]. No silent empty drawer: while `list_profiles` runs the UI
 * shows a progress row, on failure an error row with a retry.
 */
sealed interface DrawerUiState {
    /** `list_profiles` in flight — the drawer shows a progress row. */
    object Loading : DrawerUiState

    /** The profiles the core returned (possibly zero — a legit gateway). */
    data class Ready(val rows: List<BotDrawerRow>) : DrawerUiState

    /** The RPC failed — the drawer shows the error and a retry. */
    data class Failed(val message: String) : DrawerUiState
}

/**
 * Map one core profile's fields to a drawer row (the adapter calls this per
 * `ProfileSummaryDto`; the mapping is pure so the JVM suite pins it).
 * Verbatim copy: empty model/description stay empty — the UI skips them,
 * nothing invents "Unknown" copy or drops the row.
 */
fun botDrawerRow(name: String, model: String, description: String): BotDrawerRow =
    BotDrawerRow(name = name, model = model, description = description, profile = name)

/**
 * The load outcome, reduced into the next state. `rows == null` means the
 * RPC FAILED (the adapter's convention, same as `listRemoteSessions`): the
 * pre-fix shape conflated failure with an empty answer and painted a silent
 * empty drawer. A successful EMPTY list is Ready — a gateway with no
 * profiles is legitimate, not an error.
 */
fun DrawerUiState.onProfilesLoaded(rows: List<BotDrawerRow>?, message: String): DrawerUiState =
    if (rows == null) DrawerUiState.Failed(message)
    else DrawerUiState.Ready(rows)

/**
 * Retry from a failed load: back to LOADING, never to stale rows — the
 * pre-fix no-op retry kept [DrawerUiState.Failed] on screen forever.
 * Loading → Loading is the harmless re-entry (double tap).
 */
fun DrawerUiState.onRetry(): DrawerUiState =
    when (this) {
        is DrawerUiState.Failed, DrawerUiState.Loading -> DrawerUiState.Loading
        is DrawerUiState.Ready -> this
    }
