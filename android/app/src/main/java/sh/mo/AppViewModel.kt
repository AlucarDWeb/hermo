package sh.mo

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import uniffi.hermes_core.SlashOutcome

/**
 * Root view model (PLAN §4 T6 item 5): every phase transition comes from the
 * [GatewayRepository]'s flows — never inferred in the composable.
 */
class AppViewModel(app: Application) : AndroidViewModel(app) {

    val repo: GatewayRepository = GatewayRepository(HermesApp.core())

    val phase: StateFlow<AppPhase> = repo.phase
    val sessions: StateFlow<Map<String, SessionUiState>> = repo.sessions
    val currentKey: StateFlow<String?> = repo.currentKey
    val errorText: StateFlow<String> = repo.errorText
    /** T16b: the ordered open-tab set behind the session strip. */
    val tabs: StateFlow<TabSet> = repo.tabs

    /** Text typed/pasted into the manual fallback field. */
    private val _pairingPayload = MutableStateFlow("")
    val pairingPayload: StateFlow<String> = _pairingPayload

    // ── T10b: slash completion + outcome state ──────────────────────────

    private val _slashCompletions = MutableStateFlow<List<SlashCompletionRow>>(emptyList())
    val slashCompletions: StateFlow<List<SlashCompletionRow>> = _slashCompletions.asStateFlow()

    /** `replace_from` of the live completion payload (1 when `-1`/absent). */
    private val _slashReplaceFrom = MutableStateFlow(1L)
    val slashReplaceFrom: StateFlow<Long> = _slashReplaceFrom.asStateFlow()

    /** Ephemeral banner line above the composer (slash Output, local copy). */
    private val _slashBanner = MutableStateFlow("")
    val slashBanner: StateFlow<String> = _slashBanner.asStateFlow()

    /** One-shot composer directives the local draft cannot own (Prefill). */
    sealed interface ComposerEvent {
        data class Prefill(val text: String) : ComposerEvent
    }

    private val _composerEvents = MutableSharedFlow<ComposerEvent>(extraBufferCapacity = 4)
    val composerEvents: SharedFlow<ComposerEvent> = _composerEvents.asSharedFlow()

    private var completionJob: Job? = null
    private var bannerGeneration = 0

    /**
     * The model of the last Ready state, for T11's overlay ask: a re-login
     * sheet must render over the transcript it interrupted, and the
     * NeedsPassword phase carries no model — the last known one is shown
     * until the login lands a fresh header.
     */
    var lastReadyModel: String = ""
        private set

    init {
        // Track the last Ready model for the T11 overlay ask (see
        // [lastReadyModel]). The flow read runs on Main (the repository's
        // scope is Main.immediate), so a plain field write is safe.
        viewModelScope.launch {
            phase.collect { if (it is AppPhase.Ready) lastReadyModel = it.model }
        }
        // Relaunch path (checklist: 1 h / 25 h later must not ask for the
        // password): saved endpoint + cookie jar first.
        viewModelScope.launch {
            val resumed = repo.tryResume(cols())
            if (!resumed) _phaseFallbackUnpaired()
        }
    }

    /**
     * Every composer edit (T10b decision 1): a leading `/` with no space
     * completes after a 150 ms debounce — `complete.slash` on the live draft;
     * anything else hides the popup. The composable only forwards the text.
     */
    fun onDraftChanged(draft: String) {
        completionJob?.cancel()
        if (!SlashPolicy.shouldComplete(draft)) {
            _slashCompletions.value = emptyList()
            return
        }
        completionJob = viewModelScope.launch {
            delay(SLASH_DEBOUNCE_MS)
            val dto = repo.completeSlash(draft) ?: return@launch
            _slashReplaceFrom.value = dto.replaceFrom
            _slashCompletions.value = dto.items.map {
                SlashCompletionRow(text = it.text, display = it.display, kind = it.kind, meta = it.meta)
            }
        }
    }

    fun dismissCompletions() {
        completionJob?.cancel()
        _slashCompletions.value = emptyList()
    }

    /**
     * Send a chat line from the Ready screen, split per T10b decision 3:
     * local commands never reach the core, a leading `/` runs the slash
     * ladder on the repository-owned key, everything else is the normal
     * prompt (T7c: still no optimistic echo).
     */
    fun send(text: String) {
        if (text.isBlank()) return
        val command = text.trim()
        if (SlashPolicy.isLocalCommand(command)) {
            handleLocalCommand(command)
            return
        }
        if (SlashPolicy.isSlashSubmit(command)) {
            viewModelScope.launch {
                val outcome = repo.runSlash(command)
                if (outcome != null) applySlashOutcome(outcome)
            }
            return
        }
        viewModelScope.launch { repo.send(text) }
    }

    // ── T11: lifecycle / reconnect / session picker ─────────────────────

    private val _remoteSessions = MutableStateFlow<List<RemoteSessionRow>>(emptyList())
    val remoteSessions: StateFlow<List<RemoteSessionRow>> = _remoteSessions.asStateFlow()

    /** The picker sheet's visibility (phone sheet, not Desktop's sidebar). */
    private val _pickerOpen = MutableStateFlow(false)
    val pickerOpen: StateFlow<Boolean> = _pickerOpen.asStateFlow()

    /** The picker's rows are being refreshed (first open of the sheet). */
    private val _pickerLoading = MutableStateFlow(false)
    val pickerLoading: StateFlow<Boolean> = _pickerLoading.asStateFlow()

    /**
     * ON_RESUME (decision 3): the core's probe ping + reconnect. `ON_STOP`
     * does nothing on purpose — the server parks the socket 20 s, and a
     * disconnect here would drop a live turn.
     */
    fun onAppForeground() {
        viewModelScope.launch { repo.appDidForeground() }
    }

    /**
     * Decision 4: retry once when the network returns while Offline. No
     * busy-loop — the callback fires at most per network transition, and a
     * callback arriving outside Offline is ignored by the guard below.
     */
    fun onNetworkAvailable() {
        if (repo.phase.value is AppPhase.Offline) retryResume()
    }

    /** Open the session picker sheet and (re)load its rows. */
    fun openSessionPicker() {
        if (!repo.hasEndpoint()) return
        _pickerOpen.value = true
        viewModelScope.launch {
            _pickerLoading.value = true
            _remoteSessions.value = repo.listRemoteSessions().orEmpty()
            _pickerLoading.value = false
        }
    }

    fun dismissSessionPicker() {
        _pickerOpen.value = false
    }

    /** Picker tap on an existing session: resume, then close the sheet. */
    fun resumeSession(storedId: String) {
        viewModelScope.launch {
            if (repo.openExistingSession(storedId, cols())) _pickerOpen.value = false
        }
    }

    /** Picker "New chat": `open_session(null)`, then close the sheet. */
    fun newChat() {
        viewModelScope.launch {
            if (repo.openNewSession(cols())) _pickerOpen.value = false
        }
    }

    // ── T16b: the session tab strip ─────────────────────────────────────

    /** Strip tap: switch current tab (repository-side no-op if unknown). */
    fun selectTab(key: String) {
        viewModelScope.launch { repo.switchTab(key) }
    }

    /** Strip ×: close the tab (the repository guards the last-tab rule). */
    fun closeTab(key: String) {
        viewModelScope.launch { repo.closeTab(key) }
    }

    /**
     * FIX8 item 1: name the current session (titlebar edit dialog). The
     * dialog refuses the empty name; the VM guards blank too so a whitespace
     * submit never reaches the core.
     */
    fun setSessionTitle(title: String) {
        if (title.isBlank()) return
        viewModelScope.launch { repo.setSessionTitle(title.trim()) }
    }

    // ── T16c: the bot drawer ───────────────────────────────────────────

    /** The drawer's rows / loading / error state (the pure [DrawerUiState]). */
    private val _drawerState = MutableStateFlow<DrawerUiState>(DrawerUiState.Loading)
    val drawerState: StateFlow<DrawerUiState> = _drawerState.asStateFlow()

    /**
     * Drawer opened (hamburger tap or retry): start from Loading — no silent
     * empty drawer — then load the profiles through the adapter. A failed
     * RPC is `null` (NOT an empty list) and reduces to Failed, per the pure
     * [onProfilesLoaded].
     */
    fun openBotDrawer() {
        // T16c review nit 2: a Ready drawer is NOT reset to Loading — the
        // known rows stay on screen while the background reload runs (no
        // progress flash); Loading only on the first open / after a failure.
        if (_drawerState.value !is DrawerUiState.Ready) {
            _drawerState.value = DrawerUiState.Loading
        }
        viewModelScope.launch {
            val rows = repo.profiles()
            _drawerState.value = _drawerState.value.onProfilesLoaded(
                rows,
                message = repo.errorText.value.ifBlank { "Could not load profiles" },
            )
        }
    }

    /** The drawer's Retry row: same load path again. */
    fun retryBotDrawer() {
        _drawerState.value = _drawerState.value.onRetry()
        openBotDrawer()
    }

    /**
     * Drawer tap on a profile: open that bot's canonical chat through the
     * core verb; [onOpened] (the composable closes the drawer) only fires on
     * success — a failed open keeps the drawer with the surfaced error.
     * T16c review nit 3: re-entrancy guard — rapid double taps must not
     * launch two overlapping opens.
     */
    private var botOpenInFlight = false

    fun openBotChat(profile: String, onOpened: () -> Unit) {
        if (botOpenInFlight) return
        botOpenInFlight = true
        viewModelScope.launch {
            try {
                if (repo.openBotChat(profile, cols())) onOpened()
            } finally {
                botOpenInFlight = false
            }
        }
    }

    /**
     * T14 dead-end: "Reset sessions" on the offline surface — wipes the
     * local session registry (the pairing survives) and mints one fresh
     * chat. The only way out when every stored resume fails.
     */
    fun resetSessionsAndRestart() {
        viewModelScope.launch { repo.resetSessionsAndRestart(cols()) }
    }

    /** Decision 3's local branch: phone-native equivalents, declared. */
    private fun handleLocalCommand(command: String) {
        when (command) {
            // /clear: clear the composer draft only (the composer already
            // does) — the transcript rows stay: the stream is the source (T7c).
            "/clear" -> Unit
            // /sessions opens T11's picker sheet (same surface as the
            // titlebar entry point).
            "/sessions" -> openSessionPicker()
            // /quit: do NOT finish() the Activity — one-line copy, stay Ready.
            "/quit" -> showSlashBanner("Quitting is a desktop command — the phone stays ready.")
        }
    }

    /** Decision 4: map the core's SlashOutcome to composer-side state. */
    private fun applySlashOutcome(outcome: SlashOutcome) {
        when (outcome) {
            // NOT a transcript row (that would invent an index): an ephemeral
            // banner above the composer instead (declared vs Desktop's inline
            // slash output in the transcript).
            is SlashOutcome.Output -> showSlashBanner(outcome.text)
            // Set the draft, no submit.
            is SlashOutcome.Prefill -> _composerEvents.tryEmit(ComposerEvent.Prefill(outcome.text))
            // Draft cleared (the composer does on send); the user row comes
            // from the stream — no local echo (T7c).
            is SlashOutcome.Submitted -> Unit
            // Draft cleared, no banner.
            is SlashOutcome.Empty -> Unit
        }
    }

    private fun showSlashBanner(text: String) {
        bannerGeneration += 1
        val gen = bannerGeneration
        _slashBanner.value = text
        viewModelScope.launch {
            delay(SLASH_BANNER_MS)
            if (bannerGeneration == gen) _slashBanner.value = ""
        }
    }

    private fun getApp(): Application = getApplication()

    /** `cols` for the transcript area, from this device's own screen. */
    private fun cols(): Int = Cols.from(
        getApp().resources.displayMetrics.widthPixels,
        getApp().resources.displayMetrics.density,
    )

    /** Re-run the resume path — the Offline banner's Retry. */
    fun retryResume() {
        viewModelScope.launch { repo.tryResume(cols()) }
    }

    /** The runtime CAMERA permission was denied: say so, never fail silently. */
    fun onCameraPermissionDenied() {
        repo.noteError("Camera permission is needed to scan the QR — or paste the payload below")
    }

    override fun onCleared() {
        repo.close()
        super.onCleared()
    }

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
        viewModelScope.launch { repo.loginAndConnect(password, cols()) }
    }

    /** Scan screen result: a decoded `hermes://connect?...` payload. */
    fun onQrDecoded(payload: String) {
        viewModelScope.launch { repo.pair(payload) }
    }

    /** The composer's Stop (Desktop parity): interrupt the running turn. */
    fun interrupt(key: String) {
        viewModelScope.launch { repo.interrupt(key) }
    }

    /** Approval card tap (T9): the repository owns the session key. */
    fun respondApproval(requestId: String, choice: String) {
        viewModelScope.launch { repo.respondApproval(requestId, choice) }
    }

    /** Clarify card answer (T9): one lock per call, `questionId` for batch. */
    fun respondClarify(requestId: String, answer: String, questionId: String? = null) {
        viewModelScope.launch { repo.respondClarify(requestId, answer, questionId) }
    }

    companion object {
        /** T10b decision 1: the completion debounce. */
        const val SLASH_DEBOUNCE_MS = 150L

        /** The ephemeral Output banner's lifetime. */
        const val SLASH_BANNER_MS = 5_000L
    }
}
