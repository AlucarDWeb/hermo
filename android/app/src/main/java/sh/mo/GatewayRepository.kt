package sh.mo

import androidx.core.net.toUri
import uniffi.hermes_core.ConnectionStatus
import uniffi.hermes_core.EndpointDto
import uniffi.hermes_core.EventSink
import uniffi.hermes_core.HermesCore
import uniffi.hermes_core.SessionSummary
import uniffi.hermes_core.SlashCompletionsDto
import uniffi.hermes_core.SlashOutcome
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

    /**
     * The session key the UI must render: the repository OWNS the transcript
     * screen's session (defect suspect #2 — `_sessions` can hold entries for
     * keys nobody opened, and a screen rendering `keys.firstOrNull()` flips
     * to an empty foreign session mid-turn, making the rows "vanish"). The
     * UI reads this, never the map's key order.
     */
    private val _currentKey = MutableStateFlow<String?>(null)
    val currentKey: StateFlow<String?> = _currentKey.asStateFlow()


    /** Last error, surfaced as one line of text in whatever phase is shown. */
    private val _errorText = MutableStateFlow("")
    val errorText: StateFlow<String> = _errorText.asStateFlow()

    /** Surface a UI-level message (permission denial, local validation). */
    fun noteError(text: String) {
        _errorText.value = text
    }

    /** The paired endpoint, as display text (empty when unpaired). */
    private var endpointText: String = ""

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
                current.copy(rows = applyTranscriptChange(current.rows, "rowAppended", change.index.toLong(), change.rowJson))
            uniffi.hermes_core.TranscriptChangeKind.ROW_UPDATED ->
                current.copy(rows = applyTranscriptChange(current.rows, "rowUpdated", change.index.toLong(), change.rowJson))
            uniffi.hermes_core.TranscriptChangeKind.RESET -> {
                // The Reset clears the transcript; the rebuilt rows follow
                // immediately as one RowAppended each (T7c: the core emits
                // `Reset` first, then the rows of the resumed history, and the
                // stream is the single source of rows — no app-side snapshot
                // replay; it would duplicate the core's rows).
                current.copy(rows = applyTranscriptChange(current.rows, "reset", change.index.toLong(), change.rowJson))
            }
            uniffi.hermes_core.TranscriptChangeKind.HEADER_UPDATED -> {
                // The DTO carries no header payload (`row_json` is empty for
                // this kind), so the header must be re-read — off the drain
                // loop, never inline on the main thread.
                scope.launch { refreshHeader(change.key) }
                current
            }
        }
        _sessions.value = _sessions.value + (change.key to updated)
        // A transcript change for a key nobody opened must not steal the
        // screen; but if no session is current yet, the first real transcript
        // claims it (header/open races).
        if (_currentKey.value == null) _currentKey.value = change.key
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
        val key = resumeLastOrCreate(cols)
        _currentKey.value = key
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

    /**
     * Resume the tab that was active when the app last ran, falling back to a
     * fresh session.
     *
     * Passing `null` unconditionally minted a new session on every launch: the
     * durable tab list was written and never read, so `sessions.json` grew by
     * a record per launch and every reconnect then spent two RPCs per stale
     * record resuming sessions nobody had open.
     */
    private suspend fun resumeLastOrCreate(cols: Int): String {
        val last = core.lastActiveSession()
        if (last != null) {
            try {
                return core.openSession(last, cols.toLong())
            } catch (t: Throwable) {
                // The server may have dropped it (pruned, expired, restarted):
                // a new session is the right answer, not a dead screen.
                applyError(t)
            }
        }
        return core.openSession(null, cols.toLong())
    }

    private suspend fun refreshHeader(key: String) {
        val summary: SessionSummary? = core.openSessions().firstOrNull { it.key == key }
        if (summary != null) {
            val model = headerModelOf(summary.headerJson) ?: ""
            val s = _sessions.value[key] ?: SessionUiState(key = key)
            _sessions.value = _sessions.value +
                (key to s.copy(title = summary.title, model = model, running = summary.running))
            if (_currentKey.value == null) _currentKey.value = key
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
     * No optimistic echo here (T7c): after a successful `prompt.submit` the
     * core appends the user's row and delivers it through the sink like any
     * other transcript change — the stream is the single source of rows, and
     * a local echo would land on the same index as the core's row (duplicating
     * it) or race it.
     */
    suspend fun send(text: String): Boolean {
        val key: String = _currentKey.value ?: run {
            _errorText.value = "No open session to send into"
            return false
        }
        return try {
            core.send(key, text)
            _errorText.value = ""
            true
        } catch (t: Throwable) {
            applyError(t)
            false
        }
    }

    /**
     * T10b: run a slash command through the core's ladder (`slash.exec` →
     * `command.dispatch` fallback), on the session key the repository owns —
     * the same ownership rule `send` enforces. The returned [SlashOutcome]
     * drives the composer (Output banner / Prefill draft / clear), never the
     * transcript directly.
     */
    suspend fun runSlash(command: String): SlashOutcome? {
        val key: String = _currentKey.value ?: run {
            _errorText.value = "No open session to send into"
            return null
        }
        return try {
            val outcome = core.runSlash(key, command)
            _errorText.value = ""
            outcome
        } catch (t: Throwable) {
            applyError(t)
            null
        }
    }

    /**
     * T10b: composer completion (`complete.slash`) — the popup's rows. A
     * failure yields an empty list (the popup just hides), never a crash.
     */
    suspend fun completeSlash(text: String): SlashCompletionsDto? = try {
        core.completeSlash(text)
    } catch (_: Throwable) {
        null
    }

    /** Interrupt the session's running turn (the composer's Stop). */
    suspend fun interrupt(key: String): Boolean = try {
        core.interrupt(key)
        true
    } catch (t: Throwable) {
        applyError(t)
        false
    }

    /**
     * Answer an approval card on the session [GatewayRepository] owns — the
     * repository resolves the key, so a caller can never pass an absent one
     * (the same ownership rule `send` already enforces). 4009/4018 mean the
     * card was answered elsewhere; the core still resolves the local row, so
     * those surface as the "answered elsewhere" copy, not an error.
     */
    suspend fun respondApproval(requestId: String, choice: String): Boolean {
        val key: String = _currentKey.value ?: run {
            _errorText.value = "No open session to answer in"
            return false
        }
        return try {
            core.respondApproval(key, requestId, choice)
            _errorText.value = ""
            true
        } catch (t: Throwable) {
            _errorText.value = approvalRespondErrorCopy(t) ?: ErrorMessages.of(t)
            false
        }
    }

    /**
     * Answer a clarify card (single: `questionId == null`; batch: one lock
     * per qid, exactly the per-question RPCs Desktop's confirm loop sends).
     */
    suspend fun respondClarify(requestId: String, answer: String, questionId: String? = null): Boolean {
        val key: String = _currentKey.value ?: run {
            _errorText.value = "No open session to answer in"
            return false
        }
        return try {
            core.respondClarify(key, requestId, answer, questionId)
            _errorText.value = ""
            true
        } catch (t: Throwable) {
            _errorText.value = approvalRespondErrorCopy(t) ?: ErrorMessages.of(t)
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
