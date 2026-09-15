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

    /**
     * T16b: the ordered set of LOCALLY OPEN tabs (the strip's model) — an
     * entity the repository owns, never `Map.keys`: a strip over the map's
     * keys would show tabs nobody opened. The verbs live in SessionTabs.kt
     * (pure, JVM-tested); this flow only CARRIES the set.
     */
    private val _tabs = MutableStateFlow(TabSet())
    val tabs: StateFlow<TabSet> = _tabs.asStateFlow()

    /**
     * FIX7 (review #19 finding 1): keys whose stream events may mint a map
     * entry — the live tab set plus the keys being opened/restoring right
     * now. Seeded BEFORE the core call so a resume's Reset+rows racing tab
     * registration still land; drained once the tab is registered.
     */
    private val pendingKeys: MutableSet<String> =
        java.util.concurrent.ConcurrentHashMap.newKeySet()

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
                PhaseMachine.reduce(
                    _phase.value,
                    PhaseMachine.ConnEvent.NeedsPassword,
                    endpointText,
                    hasLiveSession(),
                )
        }
    }

    private fun onTranscriptChange(change: TranscriptChangeDto) {
        // FIX7 (review #19 finding 1): the map may only grow for a key the
        // repository considers open-or-restoring — live tabs plus
        // [pendingKeys], which a key enters BEFORE its open_session call.
        // That covers the resume race (Reset+rows landing before tab
        // registration) without resurrecting closed tabs. currentKey and
        // the strip still come only from open/switch/restore.
        val kind = when (change.kind) {
            TranscriptChangeKind.ROW_APPENDED -> "rowAppended"
            TranscriptChangeKind.ROW_UPDATED -> "rowUpdated"
            TranscriptChangeKind.RESET -> "reset"
            TranscriptChangeKind.HEADER_UPDATED -> "headerUpdated"
        }
        _sessions.value = applySessionChange(
            _sessions.value, change.key, kind, change.index.toLong(), change.rowJson,
            knownKeys = _tabs.value.keys.toSet() + pendingKeys,
        )
        if (change.kind == TranscriptChangeKind.HEADER_UPDATED) {
            scope.launch { refreshHeader(change.key) }
        }
    }

    /**
     * Run one core `open_session` shape, then add the key as a tab.
     * Rows the stream already delivered for that key are kept (ensureSession
     * must not clobber them with an empty SessionUiState).
     */
    private suspend fun openAndRegister(openCall: suspend () -> String): String? {
        return try {
            val key = openCall()
            // FIX7: the key is known from the moment open_session answers —
            // cover the sink race until the tab registration two lines down.
            pendingKeys.add(key)
            _sessions.value = ensureSession(_sessions.value, key)
            _tabs.value = _tabs.value.add(key)
            pendingKeys.remove(key)
            key
        } catch (t: Throwable) {
            applyError(t)
            null
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

    /**
     * T16b restore (T5 contract): resume EVERY registry tab, not just the
     * last-active one. The plan is pure (SessionTabs.kt, JVM-tested); this
     * is its adapter side: `open_session` per registry key, skip failures,
     * current = last-active if IT resumed, else the last one that did. A
     * failed `open_sessions()` is NOT an empty registry (FIX7, review #19
     * finding 2): it surfaces the error and opens nothing. An empty registry
     * that was READ successfully (or all-resumes-failed, guarded below)
     * still routes to the mint / Closed paths as before.
     */
    private suspend fun openMainSession(cols: Int) {
        val summaries: List<SessionSummary>? = try {
            core.openSessions()
        } catch (t: Throwable) {
            // FIX7 (review #19 finding 2): a failed list RPC is NOT an empty
            // registry — the pre-fix `emptyList()` conflated the two and
            // minted a brand-new session on every failed launch. Surface the
            // error and open nothing.
            applyError(t)
            null
        }
        if (summaries == null) {
            _phase.value = PhaseMachine.reduce(
                _phase.value,
                PhaseMachine.ConnEvent.Closed("error: session registry unreachable"),
                endpointText,
            )
            return
        }
        val lastActive = try {
            core.lastActiveSession()
        } catch (_: Throwable) {
            // Tolerant on purpose: null only demotes the plan's current
            // choice to "last successfully resumed" — it never mints anything.
            null
        }
        val plan = when (val step = launchStep(summaries.map { it.key }, lastActive)) {
            is LaunchStep.RunPlan -> step.plan
            // Unreachable here: summaries != null was checked above.
            LaunchStep.ListFailed -> return
        }
        // FIX7: pre-register the restore keys so resume Reset+rows that land
        // before tab registration are still applied (finding 1's race).
        pendingKeys.addAll(plan.resumeKeys)
        val resumed = mutableListOf<String>()
        for (key in plan.resumeKeys) {
            val opened = openAndRegister { core.openSession(key, cols.toLong()) }
            if (opened != null) resumed.add(opened)
        }
        pendingKeys.removeAll(plan.resumeKeys.toSet())
        // Titles/profiles/running flags from the registry snapshot we took
        // (refreshHeader only covers the current tab).
        applySummaries(summaries)
        val key = if (resumed.isEmpty()) {
            if (plan.resumeKeys.isNotEmpty()) {
                // Registry had tabs but every resume failed: do NOT mint a
                // blank chat (08_restart_restore looked like a successful
                // empty New session). Leave the error from the last failure.
                _phase.value = PhaseMachine.reduce(
                    _phase.value,
                    PhaseMachine.ConnEvent.Closed("error: no session could be resumed"),
                    endpointText,
                )
                return
            }
            val minted = openAndRegister { core.openSession(null, cols.toLong()) }
            if (minted == null) {
                // Everything failed (connection up, open_session down): say so
                // instead of sticking on Connecting — the Offline banner + Retry.
                _phase.value = PhaseMachine.reduce(
                    _phase.value,
                    PhaseMachine.ConnEvent.Closed("error: no session could be resumed"),
                    endpointText,
                )
                return
            }
            minted
        } else if (plan.current != null && plan.current in resumed) {
            plan.current
        } else {
            resumed.last()
        }
        _tabs.value = TabSet(keys = _tabs.value.keys, current = key)
        _currentKey.value = key
        afterOpen(key)
        _errorText.value = ""
    }

    /** Seed title/profile/running from an `open_sessions()` snapshot. */
    private fun applySummaries(summaries: List<SessionSummary>) {
        if (summaries.isEmpty()) return
        val updates = _sessions.value.toMutableMap()
        for (s in summaries) {
            val existing = updates[s.key] ?: continue
            updates[s.key] = existing.copy(title = s.title, profile = s.profileName, running = s.running)
        }
        _sessions.value = updates
    }

    private suspend fun refreshHeader(key: String) {
        val summary: SessionSummary? = core.openSessions().firstOrNull { it.key == key }
        if (summary != null) {
            val model = headerModelOf(summary.headerJson) ?: ""
            val s = _sessions.value[key] ?: SessionUiState(key = key)
            _sessions.value = _sessions.value +
                (key to s.copy(title = summary.title, model = model, running = summary.running, profile = summary.profileName))
            // T16b: no current claim here — refreshHeader runs after the
            // caller has selected the tab (open/switch/restore own current).
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
        // T11 (empty-jar dead-end): classify on the UniFFI CLASS, never on
        // `t.message` — the generated `SessionExpired` carries an EMPTY
        // message, so `Closed("error: ")` could only ever reduce to Offline
        // and an empty cookie jar stuck the app on Retry forever.
        if (ErrorMessages.isAuthShape(t)) {
            _phase.value = PhaseMachine.reduce(
                _phase.value, PhaseMachine.ConnEvent.AuthFailed, endpointText, hasLiveSession(),
            )
            return
        }
        _phase.value = PhaseMachine.reduce(
            _phase.value, PhaseMachine.ConnEvent.Closed("error: ${t.message ?: "?"}"), endpointText,
        )
    }

    /** Whether a transcript session is currently open (the overlay decision). */
    private fun hasLiveSession(): Boolean = _currentKey.value != null

    // ── T11: lifecycle / session picker ─────────────────────────────────

    /**
     * The remote sessions for the picker: title + preview + count. `null`
     * on failure (the sheet shows its own empty/error copy) — the failure
     * text is already in [errorText].
     */
    suspend fun listRemoteSessions(): List<RemoteSessionRow>? = try {
        core.listRemoteSessions().map {
            RemoteSessionRow(id = it.id, title = it.title, preview = it.preview, messageCount = it.messageCount)
        }
    } catch (t: Throwable) {
        applyError(t)
        null
    }

    /**
     * T16b picker tap: a session already open as a tab is SWITCHED to —
     * `open_session` is NOT re-issued (it would rebuild the LiveSession,
     * bump `history_epoch` and RESET-clear the transcript). Only a not-open
     * id goes through the core.
     */
    suspend fun openExistingSession(storedId: String, cols: Int): Boolean {
        if (storedId in _tabs.value.keys) {
            switchTab(storedId)
            return true
        }
        val key = openAndRegister { core.openSession(storedId, cols.toLong()) } ?: return false
        _currentKey.value = key
        afterOpen(key)
        return true
    }

    /** Picker "New chat" / strip `+`: `open_session(null)` mints a fresh
     *  session, which [TabSet.add] appends and selects. */
    suspend fun openNewSession(cols: Int): Boolean {
        val key = openAndRegister { core.openSession(null, cols.toLong()) } ?: return false
        _currentKey.value = key
        afterOpen(key)
        return true
    }

    /** Shared tail of the picker opens / tab switch: Ready refresh. */
    private suspend fun afterOpen(key: String) {
        refreshHeader(key)
        _errorText.value = ""
        _phase.value = PhaseMachine.ready(headerModel(key) ?: "")
    }

    /**
     * T16b strip tap: switching a tab changes `currentKey` ONLY — the core
     * is not called for a key already in the open set. `select` no-ops on a
     * key outside the set, so a stale tap cannot steal the screen.
     */
    suspend fun switchTab(key: String) {
        // FIX7 (review #19 nit 3): a tap on the already-current tab is a
        // no-op — no header RPC, no phase churn. `tabTap` also rejects keys
        // outside the set, so a stale tap cannot steal the screen.
        val next = tabTap(_tabs.value, _currentKey.value, key) ?: return
        _tabs.value = next
        val current = next.current ?: return
        _currentKey.value = current
        afterOpen(current)
    }

    /**
     * T16b strip ×: `close_session` in the core, drop the tab and the entry.
     * The last remaining tab is UNCLOSEABLE (the strip omits the ×, and this
     * guard backs it); closing the current tab selects the neighbour per the
     * pure `close` verb.
     */
    suspend fun closeTab(key: String) {
        if (_tabs.value.keys.size <= 1) return
        try {
            core.closeSession(key)
        } catch (t: Throwable) {
            applyError(t)
            return
        }
        val next = _tabs.value.close(key)
        _tabs.value = next
        _sessions.value = _sessions.value - key
        val current = next.current
        if (current != null && current != _currentKey.value) {
            _currentKey.value = current
            afterOpen(current)
        }
    }

    /** True when an endpoint is paired (the picker/lifecycle guards). */
    fun hasEndpoint(): Boolean = endpointText.isNotEmpty()

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
