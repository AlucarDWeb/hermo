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

    init {
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

    /** Decision 3's local branch: phone-native equivalents, declared. */
    private fun handleLocalCommand(command: String) {
        when (command) {
            // /clear: clear the composer draft only (the composer already
            // does) — the transcript rows stay: the stream is the source (T7c).
            "/clear" -> Unit
            // /sessions is T11's picker: intercepted with a one-line copy,
            // no invented sheet.
            "/sessions" -> showSlashBanner("The session picker arrives with T11 — nothing to list here yet.")
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
