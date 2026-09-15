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

    /**
     * FIX7-r2 (review round 2, should 2): transcript changes that arrive for
     * a key OUTSIDE the tab set / [pendingKeys] while an `open_session` is
     * in flight. The core can deliver Reset+rows before `open_session`
     * answers (picker-open / mint have no pre-seeded key) — parking instead
     * of dropping keeps that history; re-applied at registration, dropped
     * for good when no open is in flight.
     */
    private val parkedChanges: java.util.concurrent.ConcurrentLinkedQueue<TranscriptChangeDto> =
        java.util.concurrent.ConcurrentLinkedQueue()

    /** Count of in-flight `open_session` calls (see [parkedChanges]). */
    private val inFlightOpens = java.util.concurrent.atomic.AtomicInteger(0)

    /** Park capacity (FIX7-r3 nit): bounded so a runaway key cannot grow it. */
    private val PARKED_CHANGES_MAX = 512

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

    private fun kindOf(change: TranscriptChangeDto): String = when (change.kind) {
        TranscriptChangeKind.ROW_APPENDED -> "rowAppended"
        TranscriptChangeKind.ROW_UPDATED -> "rowUpdated"
        TranscriptChangeKind.RESET -> "reset"
        TranscriptChangeKind.HEADER_UPDATED -> "headerUpdated"
    }

    /** FIX7-r2: one predicate for "the map may grow for this key". */
    private fun isOpenOrRestoring(key: String): Boolean =
        key in _tabs.value.keys || key in pendingKeys

    private fun onTranscriptChange(change: TranscriptChangeDto) {
        if (!isOpenOrRestoring(change.key)) {
            // FIX7-r2 (round 2, should 2): during an in-flight open_session
            // the core may deliver Reset+rows for the session being opened
            // (picker-open / mint have no pre-seeded key). Park those and
            // re-apply them at registration; with NO open in flight an
            // unknown-key change is dropped — no phantoms, no resurrection.
            // FIX7-r3 (round 3, nit): bounded queue — a runaway key must
            // not grow the park without limit; oldest entries give way.
            if (inFlightOpens.get() > 0) {
                while (parkedChanges.size >= PARKED_CHANGES_MAX) parkedChanges.poll()
                parkedChanges.add(change)
            }
            return
        }
        _sessions.value = applySessionChange(
            _sessions.value, change.key, kindOf(change), change.index.toLong(), change.rowJson,
            knownKeys = _tabs.value.keys.toSet() + pendingKeys,
        )
        if (change.kind == TranscriptChangeKind.HEADER_UPDATED) {
            // FIX7-r2 (round 2, should 1): the header RPC sits behind the
            // SAME open-or-restoring predicate as the map write above —
            // refreshHeader writes map entries too.
            scope.launch { refreshHeader(change.key) }
        }
    }

    /**
     * Run one core `open_session` shape, then add the key as a tab.
     * Rows the stream already delivered for that key are kept (ensureSession
     * must not clobber them with an empty SessionUiState). FIX7-r2 (round 2,
     * should 2): the in-flight window is COVERED — changes parked by
     * [onTranscriptChange] while `openCall` was on the stack are re-applied
     * before registration, so no history is lost for picker-open / mint
     * either (only the restore path has pre-seeded keys).
     */
    private suspend fun openAndRegister(openCall: suspend () -> String): String? {
        inFlightOpens.incrementAndGet()
        try {
            val key = openCall()
            // FIX7: the key is known from the moment open_session answers —
            // cover the sink race until the tab registration below.
            pendingKeys.add(key)
            drainParked(key)
            _sessions.value = ensureSession(_sessions.value, key)
            _tabs.value = _tabs.value.add(key)
            pendingKeys.remove(key)
            return key
        } catch (t: Throwable) {
            applyError(t)
            return null
        } finally {
            if (inFlightOpens.decrementAndGet() == 0) {
                // No open in flight anymore: whatever is still parked belongs
                // to keys that never opened — dropped, per the unknown-key
                // contract.
                parkedChanges.clear()
            }
        }
    }

    /** Re-apply the changes parked during the in-flight open of [key]. */
    private fun drainParked(key: String) {
        if (parkedChanges.isEmpty()) return
        val known = _tabs.value.keys.toSet() + pendingKeys + key
        var sessions = _sessions.value
        // FIX7-r3 (round 3, should 1): opens can OVERLAP (each picker/+ tap
        // launches its own coroutine) — changes parked for another open's
        // key must be RE-QUEUED, never discarded, or that open registers
        // with a truncated transcript.
        val otherKeys = ArrayList<TranscriptChangeDto>()
        while (true) {
            val parked = parkedChanges.poll() ?: break
            if (parked.key != key) {
                otherKeys.add(parked)
                continue
            }
            sessions = applySessionChange(
                sessions, key, kindOf(parked), parked.index.toLong(), parked.rowJson,
                knownKeys = known,
            )
        }
        for (change in otherKeys) parkedChanges.add(change)
        _sessions.value = sessions
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
            // Unreachable by construction (summaries != null above) — fail
            // loudly rather than silently swallow a future real ListFailed
            // (FIX7-r2 nit 1).
            LaunchStep.ListFailed ->
                error("launchStep returned ListFailed for a non-null registry")
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
        // FIX7-r2 (round 2, should 1): the SAME open-or-restoring predicate
        // as the map gate — a header RPC for a closed/unknown key must
        // neither resurrect a phantom entry (the write below mints one) nor
        // spend the round trip.
        if (!isOpenOrRestoring(key)) return
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

    // ── T16c: the bot drawer ───────────────────────────────────────────

    /**
     * The bot profiles for the drawer, via the core's `list_profiles`
     * (`GET /api/profiles`). `null` on failure (the drawer shows its error
     * row with a retry; the failure text is already in [errorText]) — a
     * failed RPC is NEVER an empty drawer.
     */
    suspend fun profiles(): List<BotDrawerRow>? = try {
        core.listProfiles().map { botDrawerRow(it.name, it.model, it.description) }
    } catch (t: Throwable) {
        applyError(t)
        null
    }

    /**
     * Drawer tap on a profile: the core's `open_bot_chat(profile)` —
     * create-or-resume of the canonical `(profile, "Bot Chat")` pair. The
     * returned session goes through the SAME [openAndRegister] every open
     * uses (parking/gating semantics apply for free): it joins [TabSet] and
     * becomes `currentKey`. No client-side lookup duplication, no second
     * identity — the core verb owns create-vs-resume.
     */
    suspend fun openBotChat(profile: String, cols: Int): Boolean {
        // T16c review blocker 1: an already-open bot tab is a SWITCH (the
        // T16b picker contract) — re-issuing the core verb would rebuild the
        // LiveSession and RESET-clear the transcript. The profile is stamped
        // on the entry at open time (withProfile), so the common re-tap
        // matches even before the header lands; a still-blank profile field
        // falls through to the core verb (create-or-resume: still correct,
        // just costlier).
        openTabForProfile(_sessions.value, profile)?.let { open ->
            switchTab(open)
            return true
        }
        val key = openAndRegister { core.openBotChat(profile, cols.toLong()) } ?: return false
        _sessions.value = withProfile(_sessions.value, key, profile)
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
