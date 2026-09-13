package sh.mo.ui

import android.annotation.SuppressLint
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import sh.mo.ChatRow
import sh.mo.SessionUiState
import sh.mo.formatElapsed
import sh.mo.parseChatRow
import sh.mo.ui.LocalFonts
import sh.mo.ui.LocalHermoTokens
import sh.mo.ui.LocalTurnRunning
import sh.mo.ui.hermoRadius

/**
 * The shared session window (PI_TASK_T7A deliverable 2 + T8): the Ready phase
 * is the Desktop-shaped transcript — a panel titlebar (model + session title,
 * DESIGN.md "Panel titlebars"), the row list (user / assistant / thinking /
 * tool / status / error, DESIGN.md "Chat, tools & boot surfaces"), the
 * status strip (DESIGN.md "Feedback & empty/error/loading states") and the
 * composer.
 *
 * T7 divergences closed here:
 * 3. Titlebar insets — the titlebar now sits below the Android status bar
 *    (the system-bar inset pads the header, the way Desktop's titlebar
 *    clears its own window chrome);
 * 4. Thinking row — [TranscriptRow]'s ThinkingRow is the Desktop disclosure.
 *
 * Auto-scroll follows Desktop's stick-to-bottom rule: it only follows while
 * the reader is already at the bottom, never yanks the viewport while they
 * scroll history (styles.css `[data-slot=aui_thread-viewport]` semantics).
 */
@SuppressLint("ConfigurationScreenWidthHeight")
@Composable
fun ChatScreen(viewModel: sh.mo.AppViewModel, model: String) {
    val t = LocalHermoTokens.current
    val sessions by viewModel.sessions.collectAsState()
    val currentKey by viewModel.currentKey.collectAsState()

    // The repository OWNS the screen's session. A second entry in the sessions
    // map (a foreign key nobody opened) must never steal the view, so there is
    // no map-order fallback at all: no current key means no session to show.
    // (Review #8, nit 2 — the old `keys.firstOrNull()` survived as a fallback.)
    val key = currentKey
    val state: SessionUiState = key?.let { sessions[it] } ?: SessionUiState(key = "")
    val rows: List<ChatRow> = state.rows
        .filter { it.isNotEmpty() }
        .mapIndexed { i, json -> parseChatRow(i, json) }

    CompositionLocalProvider(LocalTurnRunning provides state.running) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .background(t.background)
                .imePadding(),
        ) {
            ChatTitlebar(model = model.ifEmpty { "unknown" }, title = state.title)
            TranscriptList(rows = rows, running = state.running, modifier = Modifier.weight(1f))
            StatusStrip(state = state, rows = rows)
            Composer(
                running = state.running,
                onSend = { text -> viewModel.send(text) },
                onStop = { key?.let { viewModel.interrupt(it) } },
            )
        }
    }
}

/**
 * Panel titlebar: one hairline under the model + session title. The row sits
 * BELOW the Android status bar — Desktop's titlebar clears the window chrome,
 * the phone's equivalent is the system-bar inset (T7 divergence 3; the badge
 * previously drew under the status bar).
 */
@Composable
private fun ChatTitlebar(model: String, title: String) {
    val t = LocalHermoTokens.current
    val statusBarPadding = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
    Column {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(top = statusBarPadding)
                .padding(horizontal = 16.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Box(
                modifier = Modifier
                    .clip(hermoRadius(t.radiusSm))
                    .background(t.primarySolid)
                    .padding(horizontal = 6.dp, vertical = 2.dp),
            ) {
                Text(
                    text = model,
                    style = androidx.compose.ui.text.TextStyle(
                        fontFamily = LocalFonts.current.sans,
                        fontSize = 11.sp,
                        fontWeight = FontWeight.Medium,
                    ),
                    color = t.onPrimary,
                )
            }
            if (title.isNotBlank()) {
                Text(
                    text = title,
                    style = androidx.compose.ui.text.TextStyle(
                        fontFamily = LocalFonts.current.sans,
                        fontSize = t.convFontSize.sp,
                    ),
                    color = t.textSecondary,
                    maxLines = 1,
                )
            }
        }
        Box(
            Modifier
                .fillMaxWidth()
                .height(1.dp)
                .background(t.strokeTertiary),
        )
    }
}

@Composable
private fun TranscriptList(rows: List<ChatRow>, running: Boolean, modifier: Modifier) {
    val t = LocalHermoTokens.current
    val listState = rememberLazyListState()
    val scope = rememberCoroutineScope()

    // Stick-to-bottom only when already there (Desktop's following rule):
    // nearBottom is derived, the jump fires only while the reader parked at
    // the bottom, and user scrolls up disable it until they return.
    val nearBottom by remember {
        derivedStateOf {
            val last = listState.layoutInfo.visibleItemsInfo.lastOrNull()
            last == null || last.index >= listState.layoutInfo.totalItemsCount - 2
        }
    }
    LaunchedEffect(rows.size) {
        if (rows.isNotEmpty() && nearBottom) listState.animateScrollToItem(rows.size - 1)
    }

    CompositionLocalProvider(LocalTurnRunning provides running) {
        if (rows.isEmpty()) {
            Box(modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text(
                    text = "Ask anything — the transcript starts here",
                    style = androidx.compose.ui.text.TextStyle(
                        fontFamily = LocalFonts.current.sans,
                        fontSize = t.convFontSize.sp,
                    ),
                    color = t.textTertiary,
                )
            }
            return@CompositionLocalProvider
        }

        LazyColumn(
            state = listState,
            modifier = modifier.fillMaxWidth(),
            contentPadding = PaddingValues(
                start = 16.dp,
                end = 16.dp,
                top = t.turnBlockGapDp.dp,
                // aui_composer-clearance: room under the last message so the
                // floating composer never covers it (styles.css 1484).
                bottom = t.turnBlockGapDp.dp + 16.dp,
            ),
            verticalArrangement = Arrangement.spacedBy(t.turnGapDp.dp),
        ) {
            items(rows, key = { it.id }) { row ->
                TranscriptRow(row)
            }
        }
    }
}

/**
 * The status strip (T8 scope 4), Desktop's: a connection dot (StatusPulse —
 * pulsing while the turn runs), the status text / "Hermes is working", the
 * elapsed time (ActivityTimerText) and the last usage tokens (the assistant
 * row's `usage` payload, Desktop's usageContextLabel).
 */
@Composable
private fun StatusStrip(state: SessionUiState, rows: List<ChatRow>) {
    val t = LocalHermoTokens.current
    if (state.key.isEmpty()) return

    // Elapsed timer: counts while the turn runs, holds the final read once it
    // settles (Desktop's ActivityTimerText from the turn's origin). The reset
    // keys on the session too (review #9 nit): re-keying the LaunchedEffect on
    // state.key alone cancels the old counter loop on a session switch even
    // while the new session's turn is not running yet — the old code keyed on
    // `running` first and the stale count survived the switch.
    var elapsed by remember { mutableStateOf(0L) }
    LaunchedEffect(state.key) {
        elapsed = 0
    }
    LaunchedEffect(state.key, state.running) {
        if (state.running) {
            elapsed = 0
            while (true) {
                delay(1000)
                elapsed += 1
            }
        }
    }

    // Last usage tokens: the most recent assistant row's usage payload.
    val usageLabel = rows.lastOrNull { it is ChatRow.Assistant }
        ?.let { (it as ChatRow.Assistant).usageJson }
        ?.let { sh.mo.usageLabel(it) }
        .orEmpty()

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 2.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Connection dot: midground pulse while the turn runs (StatusPulse),
        // quiet success-green once settled; a dead socket is the Offline
        // phase, so this strip never renders disconnected.
        Box(
            modifier = Modifier
                .size(6.dp)
                .clip(CircleShape)
                .background(if (state.running) t.midground else successDot(t)),
        )
        Text(
            text = if (state.running) "Hermes is working" else "Ready",
            style = androidx.compose.ui.text.TextStyle(
                fontFamily = LocalFonts.current.sans,
                fontSize = t.convToolFontSize.sp,
                lineHeight = t.convLineHeight.sp,
            ),
            color = t.scaffoldText,
        )
        Spacer(modifier = Modifier.weight(1f))
        if (usageLabel.isNotEmpty()) {
            Text(
                text = usageLabel,
                style = androidx.compose.ui.text.TextStyle(
                    fontFamily = LocalFonts.current.sans,
                    fontSize = 10.sp,
                ),
                color = t.scaffoldMeta,
            )
        }
        if (state.running || elapsed > 0) {
            Text(
                text = formatElapsed(elapsed),
                style = androidx.compose.ui.text.TextStyle(
                    fontFamily = LocalFonts.current.sans,
                    fontSize = 9.sp,
                    letterSpacing = 0.2.sp,
                ),
                color = t.midground.copy(alpha = 0.55f),
            )
        }
    }
}

/**
 * The composer: Desktop's composer shell — full-width rounded field in
 * --radius-lg, the send action in --theme-primary — with Desktop's
 * behaviours (T8 scope 1):
 *
 * - multi-line input that GROWS to a cap (min 1 line, max --composer-input-
 *   max-height 9.375rem ≈ 7 lines; below that the field grows in place);
 * - send on the primary action, and STOP while a turn runs (Desktop's
 *   Stop button replaces Send mid-turn — one control, two states);
 * - the placeholder pool is Desktop's composer copy, re-rolled per session;
 * - the aui_composer-clearance bottom spacing above is the transcript's
 *   counterpart so the composer never covers the last message.
 */
private val PLACEHOLDERS = listOf(
    "What are we building?",
    "Give Hermes a task",
    "What's on your mind?",
    "Describe what you need",
    "What should we tackle?",
    "Ask anything",
    "Start with a goal",
)

/** --ui-success bent toward the accent (themes/context.tsx harmonize) — the
 *  settled connection dot. Computed once: the harmonize of #10b981 against
 *  the nous midground lands near the token's own green in both modes. */
private fun successDot(t: sh.mo.ui.HermoTokens): Color = Color(0xFF2AA17C)

@Composable
private fun Composer(
    running: Boolean,
    onSend: (String) -> Unit,
    onStop: () -> Unit,
) {
    val t = LocalHermoTokens.current
    var draft by remember { mutableStateOf("") }
    // Desktop re-rolls the placeholder per conversation, not per keystroke —
    // one pick per composer composition here (the phone has one session view).
    val placeholder = remember { PLACEHOLDERS.random() }
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(12.dp),
        verticalAlignment = Alignment.Bottom,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        OutlinedTextField(
            value = draft,
            onValueChange = { draft = it },
            modifier = Modifier
                .weight(1f)
                .clip(hermoRadius(t.radiusLg)),
            placeholder = { Text(placeholder, style = androidx.compose.ui.text.TextStyle(fontSize = t.convFontSize.sp)) },
            colors = OutlinedTextFieldDefaults.colors(
                focusedBorderColor = t.midground,
                unfocusedBorderColor = t.strokeTertiary,
                cursorColor = t.midground,
            ),
            minLines = 1,
            // Desktop's --composer-input-max-height: 9.375rem ≈ 7 lines of
            // --conversation-line-height (18sp) — the growth cap.
            maxLines = 7,
        )
        Button(
            onClick = {
                if (running) {
                    onStop()
                } else if (draft.isNotBlank()) {
                    onSend(draft)
                    draft = ""
                }
            },
            enabled = running || draft.isNotBlank(),
            shape = hermoRadius(t.radiusLg),
            colors = ButtonDefaults.buttonColors(
                containerColor = t.primary,
                contentColor = t.onPrimary,
                disabledContainerColor = t.softFill,
                disabledContentColor = t.textTertiary,
            ),
            modifier = Modifier.padding(bottom = 4.dp),
        ) {
            Text(if (running) "Stop" else "Send")
        }
    }
}
