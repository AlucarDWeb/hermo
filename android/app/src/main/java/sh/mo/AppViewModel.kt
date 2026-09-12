package sh.mo

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch

/**
 * Root view model (PLAN §4 T6 item 5): every phase transition comes from the
 * [GatewayRepository]'s flows — never inferred in the composable.
 */
class AppViewModel(app: Application) : AndroidViewModel(app) {

    val repo: GatewayRepository = GatewayRepository(HermesApp.core())

    val phase: StateFlow<AppPhase> = repo.phase
    val sessions: StateFlow<Map<String, SessionUiState>> = repo.sessions
    val errorText: StateFlow<String> = repo.errorText

    /** Text typed/pasted into the manual fallback field. */
    private val _pairingPayload = MutableStateFlow("")
    val pairingPayload: StateFlow<String> = _pairingPayload

    init {
        // Relaunch path (checklist: 1 h / 25 h later must not ask for the
        // password): saved endpoint + cookie jar first.
        viewModelScope.launch {
            val cols = Cols.from(
                (getApp().resources.displayMetrics.widthPixels),
                getApp().resources.displayMetrics.density,
            )
            val resumed = repo.tryResume(cols)
            if (!resumed) _phaseFallbackUnpaired()
        }
    }

    private fun getApp(): Application = getApplication()

    private fun _phaseFallbackUnpaired() {
        // tryResume returned false: no saved endpoint -> Unpaired.
        // The repository already left phase at Unpaired; nothing to do.
    }

    fun onPairingPayloadChanged(value: String) {
        _pairingPayload.value = value
    }

    /** The manual fallback path: same payload the QR carries. */
    fun pairFromFallback() {
        val payload = _pairingPayload.value.trim()
        if (payload.isEmpty()) return
        viewModelScope.launch { repo.pair(payload) }
    }

    /** PasswordSheet submit. */
    fun submitPassword(password: String) {
        viewModelScope.launch {
            val cols = Cols.from(
                getApp().resources.displayMetrics.widthPixels,
                getApp().resources.displayMetrics.density,
            )
            repo.loginAndConnect(password, cols)
        }
    }

    /** Scan screen result: a decoded `hermes://connect?...` payload. */
    fun onQrDecoded(payload: String) {
        viewModelScope.launch { repo.pair(payload) }
    }

    /** Send a chat line from the Ready screen (T7 refines the composer). */
    fun send(text: String) {
        if (text.isBlank()) return
        viewModelScope.launch { repo.send(text) }
    }
}
