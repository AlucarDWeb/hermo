package sh.mo.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.filter
import kotlinx.coroutines.launch
import sh.mo.ChatRow
import sh.mo.SessionUiState
import sh.mo.parseChatRow
import sh.mo.ui.LocalFonts
import sh.mo.ui.LocalHermoTokens
import sh.mo.ui.hermoRadius

/**
 * The shared session window (PI_TASK_T7A deliverable 2): the Ready phase
 * becomes the Desktop-shaped transcript — a panel titlebar (model + session
 * title, DESIGN.md "Panel titlebars"), the row list (user / assistant /
 * thinking / tool / status / error, DESIGN.md "Chat, tools & boot surfaces")
 * and the composer. Auto-scroll follows Desktop's stick-to-bottom rule: it
 * only follows while the reader is already at the bottom, never yanks the
 * viewport while they scroll history (styles.css `[data-slot=aui_thread-
 * viewport]` following semantics).
 */
@Composable
fun ChatScreen(viewModel: sh.mo.AppViewModel, model: String) {
    val t = LocalHermoTokens.current
    val sessions by viewModel.sessions.collectAsState()
    val currentKey by viewModel.currentKey.collectAsState()

    // The repository OWNS the screen's session. A second entry in the
    // sessions map (a foreign key nobody opened) must never steal the view:
    // render the repository's key, fall back to the map's own entry for it —
    // never keys.firstOrNull() (the fidelity defect's suspect #2).
    val key = currentKey ?: sessions.keys.firstOrNull()
    val state: SessionUiState = key?.let { sessions[it] } ?: SessionUiState(key = "")
    val rows: List<ChatRow> = state.rows
        .filter { it.isNotEmpty() }
        .mapIndexed { i, json -> parseChatRow(i, json) }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .background(t.background)
            .imePadding(),
    ) {
        ChatTitlebar(model = model.ifEmpty { "unknown" }, title = state.title)
        TranscriptList(rows = rows, modifier = Modifier.weight(1f))
        Composer(
            onSend = { text ->
                viewModel.send(text)
            },
        )
    }
}

/** Panel titlebar: one hairline under the model + session title. */
@Composable
private fun ChatTitlebar(model: String, title: String) {
    val t = LocalHermoTokens.current
    Column {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Box(
                modifier = Modifier
                    .clip(hermoRadius(t.radiusSm))
                    .background(t.primary)
                    .padding(horizontal = 6.dp, vertical = 2.dp),
            ) {
                Text(
                    text = model,
                    style = androidx.compose.ui.text.TextStyle(
                        fontFamily = LocalFonts.current.sans,
                        fontSize = 11.sp,
                        fontWeight = androidx.compose.ui.text.font.FontWeight.Medium,
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
private fun TranscriptList(rows: List<ChatRow>, modifier: Modifier) {
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
        snapshotFlow { rows.size }
            .distinctUntilChanged()
            .filter { nearBottom }
            .collect {
                if (rows.isNotEmpty()) listState.animateScrollToItem(rows.size - 1)
            }
    }

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
        return
    }

    LazyColumn(
        state = listState,
        modifier = modifier.fillMaxWidth(),
        contentPadding = PaddingValues(
            start = 16.dp,
            end = 16.dp,
            top = t.turnBlockGapDp.dp,
            bottom = t.turnBlockGapDp.dp,
        ),
        verticalArrangement = Arrangement.spacedBy(t.turnGapDp.dp),
    ) {
        items(rows, key = { it.id }) { row ->
            TranscriptRow(row)
        }
    }
}

/**
 * The composer: the Desktop composer shell — full-width rounded field, the
 * send action in --theme-primary, --radius-lg shell — reduced to the phone's
 * single line (Desktop's directive chips / status stack arrive with T8;
 * stated, not silently dropped).
 */
@Composable
private fun Composer(onSend: (String) -> Unit) {
    val t = LocalHermoTokens.current
    var draft by remember { mutableStateOf("") }
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
            placeholder = { Text("Message", style = androidx.compose.ui.text.TextStyle(fontSize = t.convFontSize.sp)) },
            colors = OutlinedTextFieldDefaults.colors(
                focusedBorderColor = t.midground,
                unfocusedBorderColor = t.strokeTertiary,
                cursorColor = t.midground,
            ),
            maxLines = 4,
        )
        Button(
            onClick = {
                if (draft.isNotBlank()) {
                    onSend(draft)
                    draft = ""
                }
            },
            enabled = draft.isNotBlank(),
            shape = hermoRadius(t.radiusLg),
            colors = ButtonDefaults.buttonColors(
                containerColor = t.primary,
                contentColor = t.onPrimary,
                disabledContainerColor = t.softFill,
                disabledContentColor = t.textTertiary,
            ),
            modifier = Modifier.padding(bottom = 4.dp),
        ) {
            Text("Send")
        }
    }
}
