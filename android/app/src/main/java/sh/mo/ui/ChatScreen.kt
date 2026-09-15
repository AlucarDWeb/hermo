package sh.mo.ui

import android.annotation.SuppressLint
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
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
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DrawerValue
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ModalNavigationDrawer
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberDrawerState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Menu
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
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.launch
import sh.mo.ApprovalCopy
import sh.mo.AppViewModel
import sh.mo.ChatRow
import sh.mo.SessionUiState
import sh.mo.TabSet
import sh.mo.SlashCompletionRow
import sh.mo.SlashPolicy
import sh.mo.ThemeMode
import sh.mo.TranscriptRows
import sh.mo.formatElapsed
import sh.mo.ui.LocalFonts
import sh.mo.ui.LocalHermoTokens
import sh.mo.ui.LocalTurnRunning
import sh.mo.ui.hermoRadius

/**
 * The shared session window (PI_TASK_T7A deliverable 2 + T8): the Ready phase
 * is the Desktop-shaped transcript — a panel titlebar (the session title,
 * DESIGN.md "Panel titlebars"), the row list (user / assistant / thinking /
 * tool / status / error, DESIGN.md "Chat, tools & boot surfaces"), the
 * status strip (DESIGN.md "Feedback & empty/error/loading states") and the
 * composer.
 *
 * The model name lives in the composer's control row, not the titlebar —
 * Desktop's ModelPill is "the relocated status-bar pill" (model-pill.tsx,
 * controls.tsx:110) and the phone follows it.
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
fun ChatScreen(
    viewModel: sh.mo.AppViewModel,
    model: String,
    themeMode: ThemeMode,
    onThemeModeChange: (ThemeMode) -> Unit,
) {
    val t = LocalHermoTokens.current
    val sessions by viewModel.sessions.collectAsState()
    val currentKey by viewModel.currentKey.collectAsState()
    // T16b: the ordered open-tab set (repository-owned entity, not Map.keys).
    val tabs by viewModel.tabs.collectAsState()
    // The repository's last message (errors, and the "Answered elsewhere"
    // copy a 4009/4018 approval/clarify response produces) — surfaced here
    // too, not only on Pairing/Password: on the Ready/chat surface it was
    // silently dropped (PI_TASK_FIX5 item 2).
    val errorText by viewModel.errorText.collectAsState()
    // T10b: completion rows / replace_from / ephemeral banner live in the
    // view model (testable); this composable renders what it is handed.
    val slashCompletions by viewModel.slashCompletions.collectAsState()
    val slashReplaceFrom by viewModel.slashReplaceFrom.collectAsState()
    val slashBanner by viewModel.slashBanner.collectAsState()

    // Draft lives here, not inside Composer: the popup sits *above* the
    // field, so a pick must rewrite the same state the TextField shows.
    // Inserting against "" (the first draft) dropped the typed prefix and
    // never updated the field — the completion looked like it fired and
    // the input stayed at `/he`.
    var draft by remember { mutableStateOf("") }

    // T16c: the bot drawer (Material 3, from the left). The open/close
    // gestures stay on the drawer state; the VM owns the rows / loading /
    // error state and the open_bot_chat verb. Opening starts the load —
    // never a silent empty drawer.
    val scope = rememberCoroutineScope()
    val drawerState = rememberDrawerState(DrawerValue.Closed)
    val drawerUi by viewModel.drawerState.collectAsState()
    val openDrawer: () -> Unit = {
        viewModel.openBotDrawer()
        scope.launch { drawerState.open() }
    }

    // T13: the appearance sheet opens from the titlebar's trailing control
    // and overlays the chat (same shape as the session picker). The MODE is
    // handed in from above (MainActivity + UiPrefs) — never inferred here.
    var appearanceOpen by remember { mutableStateOf(false) }

    // The repository OWNS the screen's session. A second entry in the sessions
    // map (a foreign key nobody opened) must never steal the view, so there is
    // no map-order fallback at all: no current key means no session to show.
    // (Review #8, nit 2 — the old `keys.firstOrNull()` survived as a fallback.)
    val key = currentKey
    val state: SessionUiState = key?.let { sessions[it] } ?: SessionUiState(key = "")
    // Rows keep their TRANSCRIPT index as id and are parsed incrementally —
    // see [TranscriptRows] for why both matter.
    val rowCache = remember { TranscriptRows() }
    val rows: List<ChatRow> = rowCache.of(state.rows)

    CompositionLocalProvider(LocalTurnRunning provides state.running) {
        ModalNavigationDrawer(
            drawerState = drawerState,
            drawerContent = {
                BotDrawerContent(
                    state = drawerUi,
                    onProfileTap = { row ->
                        // The drawer closes only when the bot chat actually
                        // opened; on failure the error row stays visible.
                        viewModel.openBotChat(row.profile) { scope.launch { drawerState.close() } }
                    },
                    onRetry = { viewModel.retryBotDrawer() },
                )
            },
        ) {
        Box(modifier = Modifier.fillMaxSize()) {
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .background(t.background)
                    .imePadding(),
            ) {
                ChatTitlebar(
                    title = state.title,
                    onMenuTap = openDrawer,
                    onTitleTap = { viewModel.openSessionPicker() },
                    onAppearanceTap = { appearanceOpen = true },
                )
                // T16b: the ordered open-tab strip UNDER the titlebar — the
                // title tap still opens the picker, Aa still opens appearance.
                SessionTabStrip(
                    tabs = tabs,
                    sessions = sessions,
                    onSelect = viewModel::selectTab,
                    onClose = viewModel::closeTab,
                    onNew = { viewModel.newChat() },
                )
                ErrorBanner(text = errorText)
                TranscriptList(
                    rows = rows,
                    running = state.running,
                    onApproval = { id, choice -> viewModel.respondApproval(id, choice) },
                    onClarify = { id, answer, qid -> viewModel.respondClarify(id, answer, qid) },
                    modifier = Modifier.weight(1f),
                )
                StatusStrip(state = state, rows = rows)
                SlashCompletionsPopup(
                    completions = slashCompletions,
                    onPick = { item ->
                        val next = SlashPolicy.insertCompletion(draft, item.text, slashReplaceFrom)
                        draft = next
                        viewModel.dismissCompletions()
                        viewModel.onDraftChanged(next)
                    },
                )
                SlashBanner(text = slashBanner)
                Composer(
                    model = model,
                    running = state.running,
                    draft = draft,
                    prefillEvents = viewModel.composerEvents,
                    onDraftChanged = {
                        draft = it
                        viewModel.onDraftChanged(it)
                    },
                    onDismissCompletions = { viewModel.dismissCompletions() },
                    onSend = { text -> viewModel.send(text) },
                    onStop = { key?.let { viewModel.interrupt(it) } },
                )
            }
            if (appearanceOpen) {
                AppearanceSheet(
                    mode = themeMode,
                    onModeChange = onThemeModeChange,
                    onDismiss = { appearanceOpen = false },
                )
            }
        }
        }
    }
}

/**
 * Panel titlebar: one hairline under the session title. The row sits
 * BELOW the Android status bar — Desktop's titlebar clears the window chrome,
 * the phone's equivalent is the system-bar inset (T7 divergence 3; the badge
 * previously drew under the status bar).
 *
 * The model chip used to draw here; it moved into the composer as the
 * Desktop's control-row pill (model-pill.tsx 25-28, controls.tsx:110) —
 * Desktop itself calls it "the relocated status-bar pill".
 *
 * T11: the titlebar is the picker's entry point (decision 5) — a tap on the
 * TITLE opens the session sheet; `/sessions` opens the same sheet.
 *
 * T13: the tap is SPLIT — the title text opens the session picker, and a
 * trailing appearance control ("Aa") opens the theme sheet. The whole row
 * is no longer clickable, so the appearance control cannot steal the title
 * tap and the title cannot steal the appearance one.
 *
 * T16c: a LEADING hamburger IconButton (Material 3, before the title) opens
 * the bot drawer from the left — the phone's declared equivalent of
 * Desktop's Bot Mode sidebar entry point.
 */
@Composable
private fun ChatTitlebar(
    title: String,
    onMenuTap: () -> Unit,
    onTitleTap: () -> Unit,
    onAppearanceTap: () -> Unit,
) {
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
            // T16c: hamburger (M3 default menu icon) — the bot drawer's
            // entry point, at the leading edge before the title.
            IconButton(onClick = onMenuTap) {
                Icon(
                    imageVector = Icons.Filled.Menu,
                    contentDescription = "Open bots drawer",
                    tint = t.textSecondary,
                )
            }
            // `session.title` commonly lands after `open_session`, and a brand
            // new session has none at all, so the bar had nothing in it and
            // rendered as a bare strip plus a hairline. Name the session
            // rather than leaving the header empty.
            Text(
                text = title.ifBlank { "New session" },
                style = androidx.compose.ui.text.TextStyle(
                    fontFamily = LocalFonts.current.sans,
                    fontSize = t.convFontSize.sp,
                ),
                color = if (title.isBlank()) t.textTertiary else t.textSecondary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier
                    .weight(1f)
                    .clickable(onClick = onTitleTap),
            )
            // T13 appearance control: a text-only "Aa" glyph (no icon set in
            // the phone yet — same reason the picker draws a plain dot). It
            // carries its OWN click target, kept ≥ 40dp for touch.
            Box(
                modifier = Modifier
                    .size(40.dp)
                    .clip(CircleShape)
                    .clickable(onClick = onAppearanceTap),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = "Aa",
                    style = androidx.compose.ui.text.TextStyle(
                        fontFamily = LocalFonts.current.sans,
                        fontSize = t.convFontSize.sp,
                        fontWeight = FontWeight.Medium,
                    ),
                    color = t.textSecondary,
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

/**
 * T16b: the session tab strip (framework layer — it renders the [TabSet]
 * it is handed; every decision lives in the pure entity / the repository).
 * Desktop contract: DESIGN.md "Panel titlebars" + `pane-tab.tsx` — active tab
 * = 2px bottom accent underline, idle label = `--ui-text-tertiary`, hairline
 * `strokeTertiary` between tabs; `+` is a trailing control; the label is the
 * session title, or "New session" when blank; a running turn is a small
 * `midground` dot, no extra copy. Tokens only — no invented colors.
 *
 * DECLARED phone-native divergences (vs Desktop `pane-tab.tsx`, not bugs to
 * fix):
 *  - × is ALWAYS visible (no hover fade — the phone has no hover) and its
 *    touch target is ≥ 40dp;
 *  - no middle-click, no ⌘W, no split panes, no slot numbers, no tab drag;
 *  - the tab row scrolls HORIZONTALLY instead of squeezing every tab into
 *    the viewport; `+` stays fixed so it is reachable with many tabs;
 *  - the picker (title tap) stays a bottom sheet (T11), not Desktop's
 *    centered dialog.
 */
@Composable
private fun SessionTabStrip(
    tabs: TabSet,
    sessions: Map<String, SessionUiState>,
    onSelect: (String) -> Unit,
    onClose: (String) -> Unit,
    onNew: () -> Unit,
) {
    val t = LocalHermoTokens.current
    Row(
        modifier = Modifier.fillMaxWidth(),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(
            modifier = Modifier
                .weight(1f)
                .horizontalScroll(rememberScrollState()),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            tabs.keys.forEachIndexed { index, key ->
                if (index > 0) {
                    // Hairline strokeTertiary between tabs (pane-tab.tsx).
                    Box(
                        Modifier
                            .width(1.dp)
                            .height(t.tabStripSeparatorHeightDp.dp)
                            .background(t.strokeTertiary),
                    )
                }
                SessionTabItem(
                    key = key,
                    state = sessions[key],
                    active = key == tabs.current,
                    closeable = tabs.keys.size > 1, // last tab is uncloseable
                    onSelect = { onSelect(key) },
                    onClose = { onClose(key) },
                )
            }
        }
        // Trailing `+`: `open_session(null)` — same verb as picker "New chat".
        Box(
            modifier = Modifier
                .size(40.dp)
                .clickable(onClick = onNew),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                text = "+",
                style = androidx.compose.ui.text.TextStyle(
                    fontFamily = LocalFonts.current.sans,
                    fontSize = t.convFontSize.sp,
                    fontWeight = FontWeight.Medium,
                ),
                color = t.textTertiary,
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

/**
 * One tab (pane-tab.tsx shape): label + optional running dot + close, with
 * the 2px accent underline when active. Idle label = textTertiary, active =
 * `--ui-text` (the pane-tab title color). The close × is always visible —
 * phone-native divergence declared above.
 */
@Composable
private fun SessionTabItem(
    key: String,
    state: SessionUiState?,
    active: Boolean,
    closeable: Boolean,
    onSelect: () -> Unit,
    onClose: () -> Unit,
) {
    val t = LocalHermoTokens.current
    Column(
        modifier = Modifier
            .heightIn(min = 40.dp)
            // Fixed width so tabs overflow into the horizontal scroller
            // instead of shrinking (02_two_tabs: third label clipped against +).
            .width(t.tabStripTabMaxWidthDp.dp)
            .clickable(onClick = onSelect),
    ) {
        // Wrap-content height (NO weight): a weight inside the screen Column's
        // leftover max inflated the tab to the whole transcript area.
        Row(
            modifier = Modifier.padding(start = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            if (state?.running == true) {
                Box(
                    Modifier
                        .size(6.dp)
                        .clip(CircleShape)
                        .background(t.midground),
                )
            }
            Text(
                // Tab label: the session title, or "New session" when blank.
                text = (state?.title ?: "").ifBlank { "New session" },
                style = androidx.compose.ui.text.TextStyle(
                    fontFamily = LocalFonts.current.sans,
                    fontSize = t.convToolFontSize.sp,
                ),
                color = if (active) t.text else t.textTertiary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f, fill = false),
            )
            if (closeable) {
                Box(
                    modifier = Modifier
                        .size(40.dp)
                        .clickable(onClick = onClose),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        text = "×",
                        style = androidx.compose.ui.text.TextStyle(
                            fontFamily = LocalFonts.current.sans,
                            fontSize = t.convFontSize.sp,
                        ),
                        color = t.textTertiary,
                    )
                }
            }
        }
        // Active = 2px bottom accent underline (pane-tab.tsx).
        Box(
            Modifier
                .fillMaxWidth()
                .height(2.dp)
                .background(if (active) t.midground else Color.Transparent),
        )
    }
}

/**
 * One-line banner directly under the titlebar (PI_TASK_FIX5 item 2): the
 * repository's `_errorText` was only read by Pairing/Password, so the
 * "Answered elsewhere" copy a 4009/4018 response produced never appeared on
 * the session window. A quiet destructive-tinted line — Desktop has no error
 * dialog here and neither does the phone.
 */
@Composable
private fun ErrorBanner(text: String) {
    if (text.isBlank()) return
    val t = LocalHermoTokens.current
    Text(
        text = text,
        style = androidx.compose.ui.text.TextStyle(
            fontFamily = LocalFonts.current.sans,
            fontSize = t.convToolFontSize.sp,
        ),
        color = t.destructive,
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 6.dp),
    )
}


@Composable
private fun TranscriptList(
    rows: List<ChatRow>,
    running: Boolean,
    onApproval: (String, String) -> Unit,
    onClarify: (String, String, String?) -> Unit,
    modifier: Modifier,
) {
    val t = LocalHermoTokens.current
    val listState = rememberLazyListState()
    val scope = rememberCoroutineScope()
    val pendingIdx = rows.indexOfFirst { it is ChatRow.Approval && !it.resolved }
    val pendingOffscreen by remember(pendingIdx) {
        derivedStateOf {
            if (pendingIdx < 0) return@derivedStateOf false
            val visible = listState.layoutInfo.visibleItemsInfo
            visible.isNotEmpty() && visible.none { it.index == pendingIdx }
        }
    }

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

        Box(modifier.fillMaxSize()) {
        LazyColumn(
            state = listState,
            modifier = Modifier.fillMaxWidth().fillMaxSize(),
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
                TranscriptRow(row, onApproval = onApproval, onClarify = onClarify)
            }
        }
        if (pendingOffscreen && pendingIdx >= 0) {
            Text(
                text = ApprovalCopy.JUMP_TO_APPROVAL,
                style = androidx.compose.ui.text.TextStyle(
                    fontFamily = LocalFonts.current.sans,
                    fontSize = t.convToolFontSize.sp,
                    fontWeight = FontWeight.Medium,
                ),
                color = t.onPrimary,
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .padding(bottom = 8.dp)
                    .clip(hermoRadius(t.radiusXl))
                    .background(t.primarySolid)
                    .clickable { scope.launch { listState.animateScrollToItem(pendingIdx) } }
                    .padding(horizontal = 12.dp, vertical = 8.dp),
            )
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
 * The composer: Desktop's composer shell — one rounded box (border in
 * --radius-lg, index.tsx composer-surface) holding the input row and the
 * send action in --theme-primary — with Desktop's behaviours (T8 scope 1):
 *
 * - multi-line input that GROWS to a cap (min 1 line, max --composer-input-
 *   max-height 9.375rem ≈ 7 lines; below that the field grows in place);
 * - send on the primary action, and STOP while a turn runs (Desktop's
 *   Stop button replaces Send mid-turn — one control, two states);
 * - the placeholder pool is Desktop's composer copy, re-rolled per session;
 * - the aui_composer-clearance bottom spacing above is the transcript's
 *   counterpart so the composer never covers the last message.
 *
 * The shell grid is Desktop's `menu | input | controls` (index.tsx ~1386:
 * grid-template-areas "menu_input_controls", controls `justify-end`): on the
 * phone there is no `menu` yet, so the row is `[input] [pill] [Send/Stop]`
 * — the model pill is the FIRST control of the cluster (controls.tsx:110),
 * the relocated status-bar pill (model-pill.tsx).
 *
 * Width under pressure follows Desktop's stated intent (model-pill.tsx:22-28:
 * the pill is "the one control in the row that can give width back", over an
 * `auto_1fr_auto` grid whose `1fr` is the input): the pill and the button are
 * the `auto` columns, sized to their content, and the input is the only
 * weighted child, so it absorbs the rest. The pill gives width back through
 * its `max-w-40` cap and its own ellipsis, never through a share of the
 * flexible space — a weighted pill was capped at a quarter of that space,
 * which on a phone is narrower than the cap it was meant to honour.
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
    model: String,
    running: Boolean,
    draft: String,
    prefillEvents: SharedFlow<AppViewModel.ComposerEvent>,
    onDraftChanged: (String) -> Unit,
    onDismissCompletions: () -> Unit,
    onSend: (String) -> Unit,
    onStop: () -> Unit,
) {
    val t = LocalHermoTokens.current
    // T10b: Prefill from the core rewrites the parent-owned draft.
    LaunchedEffect(prefillEvents) {
        prefillEvents.collect { event ->
            when (event) {
                is AppViewModel.ComposerEvent.Prefill -> onDraftChanged(event.text)
            }
        }
    }
    // Desktop re-rolls the placeholder per conversation, not per keystroke —
    // one pick per composer composition here (the phone has one session view).
    val placeholder = remember { PLACEHOLDERS.random() }
    // Border color follows focus exactly as the previous OutlinedTextField
    // colors did (focused = --theme-midground, rest = --ui-stroke-tertiary);
    // the hand-drawn shell needs the interaction source for that.
    val interaction = remember { MutableInteractionSource() }
    val focusRequester = remember { FocusRequester() }
    // Separate from `interaction`: the field's own source drives the focus
    // border, and a tap on the shell must not light that up by itself.
    val shellTaps = remember { MutableInteractionSource() }
    val focused by interaction.collectIsFocusedAsState()
    val borderColor = if (focused) t.midground else t.strokeTertiary
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .padding(12.dp)
            .clip(hermoRadius(t.radiusLg))
            .background(t.surface)
            // The whole bordered box reads as the input, so the whole bordered
            // box focuses it. Without this a tap outside the field's own
            // bounds did nothing and the keyboard never came up.
            .clickable(
                interactionSource = shellTaps,
                indication = null,
            ) { focusRequester.requestFocus() }
            .border(1.dp, borderColor, hermoRadius(t.radiusLg))
            .padding(
                horizontal = t.composerSurfacePadXDp.dp,
                vertical = t.composerSurfacePadYDp.dp,
            ),
    ) {
        Row(
            // Desktop's control cluster is `justify-end`: without this the Row
            // measures to its children and the Send button drifts inward by a
            // gap that changes width with the model name.
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.Bottom,
            horizontalArrangement = Arrangement.spacedBy(t.composerControlGapDp.dp),
        ) {
            BasicTextField(
                value = draft,
                onValueChange = {
                    // T10b: the view model owns when to complete (150 ms
                    // debounce) — the field only reports edits.
                    onDraftChanged(it)
                },
                // Desktop's `1fr` input column (index.tsx:1381): the only
                // weighted child, so it absorbs whatever the `auto` pill and
                // button leave. The min height matches the control cluster, so
                // a single line fills the shell rather than sitting at the
                // bottom of it under a dead strip of bordered box.
                modifier = Modifier
                    .weight(1f)
                    .heightIn(min = t.composerControlRowHeightDp.dp)
                    .focusRequester(focusRequester),
                textStyle = androidx.compose.ui.text.TextStyle(
                    fontFamily = LocalFonts.current.sans,
                    fontSize = t.convFontSize.sp,
                    lineHeight = t.convLineHeight.sp,
                    color = t.text,
                ),
                cursorBrush = SolidColor(t.midground),
                interactionSource = interaction,
                // Desktop's --composer-input-max-height: 9.375rem ≈ 7 lines of
                // --conversation-line-height (18sp) — the growth cap.
                maxLines = 7,
                decorationBox = { inner ->
                    // Centered: the field now owns the full control-row height,
                    // so a single line must sit in the middle of it.
                    Box(contentAlignment = Alignment.CenterStart) {
                        if (draft.isEmpty()) {
                            Text(
                                text = placeholder,
                                style = androidx.compose.ui.text.TextStyle(
                                    fontFamily = LocalFonts.current.sans,
                                    fontSize = t.convFontSize.sp,
                                    lineHeight = t.convLineHeight.sp,
                                ),
                                color = t.textTertiary,
                            )
                        }
                        inner()
                    }
                },
            )
            // Desktop shows a quiet spinner until the model resolves
            // (model-pill.tsx:95-107); the phone has no picker yet, so until
            // the model lands nothing renders — never invented "unknown" copy.
            if (model.isNotBlank()) {
                // No weight: Desktop's grid is `auto 1fr auto`, so the pill is
                // an `auto` column at its natural width under the max-w-40 cap
                // and the input is the `1fr` that absorbs the rest. Weighting
                // it against the input capped it at a quarter of the flexible
                // space — about 42dp of text on a 360dp phone, so every
                // realistic model name ellipsised to "claude-…" while the cap
                // it was supposed to honour could never be reached.
                ModelPill(model = model)
            }
            Button(
                onClick = {
                    if (running) {
                        onStop()
                    } else if (draft.isNotBlank()) {
                        onSend(draft)
                        onDraftChanged("")
                        onDismissCompletions()
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
}

/**
 * Desktop's ModelPill, relocated (model-pill.tsx 25-28): ghost styling,
 * --ui-text-tertiary, `text-xs` (11sp here), ONE truncating line at
 * `max-w-40` — "the one control in the row that can give width back".
 * The pill sits at its natural width up to the 160dp cap and ellipsises there;
 * the input is the weighted column that absorbs the rest of the row.
 *
 * Divergence, stated: the Desktop pill is the dropdown trigger for the live
 * `model.options` menu; the phone has no model picker yet (a later phase:
 * `model.options` + `config.set`), so there is NO chevron and NO press
 * affordance — a control that opens nothing would be a dead affordance.
 * This is a static label until the picker lands.
 */
@Composable
private fun ModelPill(model: String, modifier: Modifier = Modifier) {
    val t = LocalHermoTokens.current
    Text(
        text = model,
        style = androidx.compose.ui.text.TextStyle(
            fontFamily = LocalFonts.current.sans,
            fontSize = t.convToolFontSize.sp, // text-xs ≈ --conversation-tool-font-size
            fontWeight = FontWeight.Normal,
        ),
        color = t.textTertiary,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        modifier = modifier
            // The Desktop cap clamps the flexible quota the caller granted:
            // the pill never exceeds min(its Row share, max-w-40).
            .widthIn(max = t.composerPillMaxWidthDp.dp)
            .padding(horizontal = 8.dp, vertical = 4.dp), // px-2; 4dp vertical ≈ h-(--composer-control-size) on text-xs
    )
}

/**
 * T10b: the slash completion popup, Desktop's composer-completion-drawer
 * (completion-drawer.tsx COMPLETION_DRAWER_CLASS: `bottom-full mb-1` — it
 * floats just above the composer edge, inset from the left) rendering the
 * trigger-popover row (trigger-popover.tsx ROW_CLASS: one line, icon column,
 * name + description). DESIGN.md "Slash descriptions": the row is
 * single-line ellipsized and Desktop reveals the full description on hover —
 * the phone has no hover, so `meta` is a SUBTITLE on the row (declared
 * phone-native equivalent). Desktop groups rows Commands/Skills from the
 * backend `kind`; the phone's drawer is narrow, so `kind` renders as a quiet
 * trailing label instead of group headers.
 *
 * Rendered in the screen's Column directly above the composer, so nothing
 * floats — the popup simply appears between the status strip and the field.
 */
@Composable
fun SlashCompletionsPopup(
    completions: List<SlashCompletionRow>,
    onPick: (SlashCompletionRow) -> Unit,
) {
    if (completions.isEmpty()) return
    val t = LocalHermoTokens.current
    // The drawer shell: w-80 (320dp capped to the width), max-h 22rem ≈ 352dp,
    // popover surface + hairline (composerPanelCard) — Desktop's own numbers.
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 12.dp)
            .heightIn(max = 352.dp)
            .clip(hermoRadius(t.radiusLg))
            .background(t.elevated)
            .border(1.dp, t.strokeTertiary, hermoRadius(t.radiusLg))
            .padding(vertical = 4.dp) // p-1 of the drawer shell
            .verticalScroll(rememberScrollState()),
    ) {
        completions.forEach { item ->
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable { onPick(item) }
                    .padding(horizontal = 8.dp, vertical = 4.dp), // px-2 py-1 (ROW_CLASS)
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp), // gap-2
            ) {
                Column(Modifier.weight(1f)) {
                    Text(
                        text = item.display,
                        style = androidx.compose.ui.text.TextStyle(
                            fontFamily = LocalFonts.current.sans,
                            fontSize = t.convFontSize.sp,
                        ),
                        color = t.text,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    if (item.meta.isNotBlank()) {
                        Text(
                            text = item.meta,
                            style = androidx.compose.ui.text.TextStyle(
                                fontFamily = LocalFonts.current.sans,
                                fontSize = t.convToolFontSize.sp,
                            ),
                            color = t.textTertiary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
                if (item.kind.isNotBlank()) {
                    Text(
                        text = item.kind,
                        style = androidx.compose.ui.text.TextStyle(
                            fontFamily = LocalFonts.current.sans,
                            fontSize = 10.sp,
                        ),
                        color = t.textTertiary,
                    )
                }
            }
        }
    }
}

/**
 * T10b: the ephemeral slash-output banner. `SlashOutcome.Output` is NOT a
 * transcript row (that would invent an index on the stream) — a one-line
 * status line above the composer that fades after a few seconds. Declared
 * divergence: Desktop renders slash output inline in the transcript.
 */
@Composable
private fun SlashBanner(text: String) {
    if (text.isBlank()) return
    val t = LocalHermoTokens.current
    Text(
        text = text,
        style = androidx.compose.ui.text.TextStyle(
            fontFamily = LocalFonts.current.sans,
            fontSize = t.convToolFontSize.sp,
        ),
        color = t.scaffoldText,
        maxLines = 2,
        overflow = TextOverflow.Ellipsis,
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 4.dp),
    )
}
