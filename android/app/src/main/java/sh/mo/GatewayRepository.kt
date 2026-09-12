package sh.mo

import androidx.core.net.toUri
import uniffi.hermes_core.ConnectionStatus
import uniffi.hermes_core.EndpointDto
import uniffi.hermes_core.EventSink
import uniffi.hermes_core.HermesCore
import uniffi.hermes_core.SessionSummary
import uniffi.hermes_core.TranscriptChangeDto
import uniffi.hermes_core.TranscriptChangeKind
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import org.json.JSONObject

/** Transcript state for one session key (plain text rows — the T7 chat screen refines this). */
data class SessionUiState(
    val key: String,
    val title: String = "",
    val model: String = "",
    val rows: List<String> = emptyList(),
)

/**
 * The adapter between [HermesCore] and the UI (PLAN §4 T6 item 5).
 *
 * UniFFI delivers `EventSink` callbacks on a Rust thread: everything the
 * callbacks produce is posted into a [Channel.UNLIMITED] and drained on
 * `Dispatchers.Main.immediate` — UI state is only ever touched on the main
 * thread. Every core call is suspend and invoked from a coroutine.
 */
class GatewayRepository(private val core: HermesCore) : EventSink {

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    /** Events arriving from Rust threads; drained on Main.immediate. */
    private sealed interface CoreEvent {
        data class Transcript(val change: TranscriptChangeDto) : CoreEvent
        data class Connection(val status: ConnectionStatus) : CoreEvent
    }

    private val events = Channel<CoreEvent>(Channel.UNLIMITED)

    private val _phase = MutableStateFlow<AppPhase>(AppPhase.Unpaired)
    val phase: StateFlow<AppPhase> = _phase.asStateFlow()

    private val _sessions = MutableStateFlow<Map<String, SessionUiState>>(emptyMap())
    val sessions: StateFlow<Map<String, SessionUiState>> = _sessions.asStateFlow()

    /** Last error, surfaced as one line of text in whatever phase is shown. */
    private val _errorText = MutableStateFlow("")
    val errorText: StateFlow<String> = _errorText.asStateFlow()

    /** Surface a UI-level message (permission denial, local validation). */
    fun noteError(text: String) {
        _errorText.value = text
    }

    /** The paired endpoint, as display text (empty when unpaired). */
    private var endpointText: String = ""

    /**
     * The session key the UI is driving. The repository owns it so a caller
     * never passes an absent/stale key and silently does nothing (the device
     * acceptance run found `send` no-oping for exactly that reason).
     */
    private var currentKey: String? = null

    init {
        // The single drain point: core events -> phase + transcript state.
        scope.launch {
            for (ev in events) {
                when (ev) {
                    is CoreEvent.Connection -> onConnectionStatus(ev.status)
                    is CoreEvent.Transcript -> onTranscriptChange(ev.change)
                }
            }
        }
    }

    // ── EventSink (called on Rust threads — no state touch here) ────────

    override fun onTranscript(change: TranscriptChangeDto) {
        events.trySend(CoreEvent.Transcript(change))
    }

    override fun onConnection(status: ConnectionStatus) {
        events.trySend(CoreEvent.Connection(status))
    }

    // ── main-thread reduction of core events ────────────────────────────

    private fun onConnectionStatus(status: ConnectionStatus) {
        _phase.value = when (status) {
            is ConnectionStatus.Connecting ->
                PhaseMachine.reduce(_phase.value, PhaseMachine.ConnEvent.Connecting, endpointText)
            is ConnectionStatus.Open ->
                PhaseMachine.reduce(_phase.value, PhaseMachine.ConnEvent.Open, endpointText)
            is ConnectionStatus.Closed ->
                PhaseMachine.reduce(_phase.value, PhaseMachine.ConnEvent.Closed(status.reason), endpointText)
            is ConnectionStatus.NeedsPassword ->
                PhaseMachine.reduce(_phase.value, PhaseMachine.ConnEvent.NeedsPassword, endpointText)
        }
    }

    private fun onTranscriptChange(change: TranscriptChangeDto) {
        val current = _sessions.value[change.key] ?: SessionUiState(key = change.key)
        val updated = when (change.kind) {
            uniffi.hermes_core.TranscriptChangeKind.ROW_APPENDED ->
                current.copy(rows = current.rows + rowText(change.rowJson))
            uniffi.hermes_core.TranscriptChangeKind.ROW_UPDATED -> {
                val rows = current.rows.toMutableList()
                val idx = change.index.toInt()
                if (idx in rows.indices) rows[idx] = rowText(change.rowJson)
                current.copy(rows = rows)
            }
            uniffi.hermes_core.TranscriptChangeKind.RESET ->
                current.copy(rows = emptyList())
            uniffi.hermes_core.TranscriptChangeKind.HEADER_UPDATED -> {
                // The DTO carries no header payload (`row_json` is empty for
                // this kind), so the header must be re-read — off the drain
                // loop, never inline on the main thread.
                scope.launch { refreshHeader(change.key) }
                current
            }
        }
        _sessions.value = _sessions.value + (change.key to updated)
    }

    /** One plain-text line per row (MVP bar: no markdown, no cards). */
    private fun rowText(rowJson: String): String {
        if (rowJson.isEmpty()) return ""
        return try {
            val obj = JSONObject(rowJson)
            when (obj.optString("kind")) {
                "user" -> "You: ${obj.optString("text")}"
                "assistant" -> "Agent: ${obj.optString("text")}"
                "thinking" -> "Thinking: ${obj.optString("text")}"
                "tool" -> {
                    val done = if (obj.optBoolean("complete")) "done" else "running"
                    "Tool ${obj.optString("name")} ($done)"
                }
                else -> obj.optString("kind")
            }
        } catch (_: Exception) {
            ""
        }
    }

    private fun applyError(t: Throwable) {
        _errorText.value = ErrorMessages.of(t)
    }

    // ── app-side lifecycle calls (all suspend, never main-blocked) ──────

    /**
     * Relaunch path (PLAN §4 T6 item "relaunch after 1 h / 25 h must not
     * ask"): saved endpoint + cookie jar first — no password prompt when
     * the jar is still valid. Returns true when a connect attempt started.
     */
    suspend fun tryResume(cols: Int): Boolean {
        val ep: EndpointDto = core.savedEndpoint() ?: return false
        endpointText = ep.displayText()
        _phase.value = AppPhase.Connecting
        try {
            core.connect(this)
            openMainSession(cols)
            return true
        } catch (t: Throwable) {
            onConnectFailure(t)
            return true
        }
    }

    /** QR/manual pairing: parse + persist the payload, then ask for the password. */
    suspend fun pair(payload: String) {
        try {
            val ep = core.pair(payload)
            endpointText = ep.displayText()
            _errorText.value = ""
            _phase.value = AppPhase.NeedsPassword(endpointText)
        } catch (t: Throwable) {
            applyError(t)
        }
    }

    /** Password sheet: login + connect + first session. */
    suspend fun loginAndConnect(password: String, cols: Int) {
        _phase.value = AppPhase.Connecting
        try {
            core.login(password)
            core.connect(this)
            openMainSession(cols)
        } catch (t: Throwable) {
            onConnectFailure(t)
        }
    }

    private suspend fun openMainSession(cols: Int) {
        val key = core.openSession(null, cols.toLong())
        currentKey = key
        // Register the tab immediately: the Ready screen's Send guard reads
        // this map, so an entry must exist before any header arrives.
        if (_sessions.value[key] == null) {
            _sessions.value = _sessions.value + (key to SessionUiState(key = key))
        }
        // Header (model name) arrives via open_sessions — pull it once here;
        // the HEADER_UPDATED change triggers the same read when it lands.
        refreshHeader(key)
        _errorText.value = ""
        _phase.value = PhaseMachine.ready(headerModel(key) ?: "")
    }

    private suspend fun refreshHeader(key: String) {
        val summary: SessionSummary? = core.openSessions().firstOrNull { it.key == key }
        if (summary != null) {
            val model = headerModelOf(summary.headerJson) ?: ""
            val s = _sessions.value[key] ?: SessionUiState(key = key)
            _sessions.value = _sessions.value + (key to s.copy(title = summary.title, model = model))
            if (currentKey == null) currentKey = key
            // A header that lands after Ready must reach the screen.
            if (model.isNotEmpty()) {
                _phase.value =
                    PhaseMachine.reduce(_phase.value, PhaseMachine.ConnEvent.Header(model), endpointText)
            }
        }
    }

    private fun headerModel(key: String): String? = _sessions.value[key]?.model?.takeIf { it.isNotEmpty() }

    private fun headerModelOf(headerJson: String): String? = try {
        JSONObject(headerJson).optString("model").takeIf { it.isNotEmpty() }
    } catch (_: Exception) {
        null
    }

    private fun onConnectFailure(t: Throwable) {
        applyError(t)
        _phase.value = PhaseMachine.reduce(
            _phase.value, PhaseMachine.ConnEvent.Closed("error: ${t.message ?: "?"}"), endpointText,
        )
    }

    /**
     * Submit a prompt for the open session, resolving the key HERE (the
     * repository owns it, so a caller can never pass an absent key and no-op
     * silently — the defect the device acceptance run exposed).
     *
     * The core deliberately does not create the local user row (`send` doc:
     * the wire replay drives the transcript), so the echo is added to the UI
     * rows; a resumed history replaces it after a RESET.
     */
    suspend fun send(text: String): Boolean {
        val key = currentKey
        if (key == null) {
            _errorText.value = "No open session to send into"
            return false
        }
        return try {
            core.send(key, text)
            val s = _sessions.value[key] ?: SessionUiState(key = key)
            _sessions.value = _sessions.value + (key to s.copy(rows = s.rows + "You: $text"))
            _errorText.value = ""
            true
        } catch (t: Throwable) {
            applyError(t)
            false
        }
    }

    suspend fun appDidForeground() {
        try {
            core.appDidForeground()
        } catch (_: Throwable) {
            // The probe failing on a dead socket is expected pre-connect.
        }
    }

    suspend fun disconnect() {
        try {
            core.disconnect()
        } catch (t: Throwable) {
            applyError(t)
        }
    }

    private fun EndpointDto.displayText(): String =
        "${displayName} (${username}) — ${baseUrl}"

    /**
     * Cancel this repository's scope. The view model calls it from
     * `onCleared()`, so the drain loop and any in-flight header refresh do not
     * outlive the screen.
     */
    fun close() {
        scope.cancel()
    }
}
