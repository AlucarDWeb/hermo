package sh.mo

import androidx.lifecycle.ViewModel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * App phases. Stream B adds NeedsPassword/Connecting/Ready/Offline and the
 * pairing logic; stream A ships only the real [Unpaired] phase — the state
 * the UI renders always comes from this view model, never a literal in the
 * composable.
 */
sealed interface AppPhase {
    /** No gateway paired yet: show the "pair this device" call to action. */
    data object Unpaired : AppPhase
}

/**
 * Root view model: owns the app phase. Stream B will source the phase from
 * the paired-endpoint store and the GatewayRepository; for now the phase
 * starts at [AppPhase.Unpaired] and the action is a no-op hook that stream B
 * replaces with the pairing flow.
 */
class AppViewModel : ViewModel() {

    private val _phase = MutableStateFlow<AppPhase>(AppPhase.Unpaired)

    /** The current app phase; observed by MainActivity's composable. */
    val phase: StateFlow<AppPhase> = _phase.asStateFlow()

    private val _pairingPayload = MutableStateFlow("")

    /**
     * The pairing text the user entered (a `hermes://connect?...` payload,
     * or a gateway URL + username). Stream B consumes it.
     */
    val pairingPayload: StateFlow<String> = _pairingPayload.asStateFlow()

    /** Called by the Unpaired screen's text field. */
    fun onPairingPayloadChanged(value: String) {
        _pairingPayload.value = value
    }

    /**
     * Called by the Unpaired screen's call-to-action button.
     *
     * NOT wired yet (stream B): [pairingEnabled] is false until the pairing
     * flow exists, so the button is visibly disabled rather than a control
     * that silently does nothing (review #6, nit). When it lands this calls
     * HermesCore.pair / parse_qr_payload with the entered payload.
     */
    val pairingEnabled: Boolean = false

    fun onPairRequested() {
        // Reached only when pairingEnabled becomes true (stream B).
    }
}
