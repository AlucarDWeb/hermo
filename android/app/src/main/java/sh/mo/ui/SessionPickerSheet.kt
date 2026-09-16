package sh.mo.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import sh.mo.RemoteSessionRow

/**
 * T11 session picker: the phone sheet over `session.list` (the Desktop's
 * `session-picker.tsx` is the contract — title + preview rows, type-to-filter
 * declared nice-to-have; the phone ships the simple list). DESIGN.md "Panel
 * titlebars" (the sheet's header) and "Feedback & empty/error/loading states"
 * (the loading spinner and the empty copy); tokens from `ui/Theme.kt`.
 *
 * Rendered as a bottom sheet overlaying the chat (declared divergence vs
 * Desktop's centered dialog: the phone's one-hand reach).
 */
@Composable
fun SessionPickerSheet(
    sessions: List<RemoteSessionRow>,
    loading: Boolean,
    onDismiss: () -> Unit,
    onResume: (String) -> Unit,
    onNewChat: () -> Unit,
) {
    val t = LocalHermoTokens.current
    Box(
        modifier = Modifier
            .fillMaxSize()
            .clickable(onClick = onDismiss) // the scrim tap dismisses — FIX8-bis: the scrim covers the WHOLE screen (it was width-only: taps above the sheet hit the chat and the sheet was inescapable)
            .background(t.background.copy(alpha = 0.55f)),
        contentAlignment = Alignment.BottomCenter,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .clickable(
                    indication = null,
                    interactionSource = remember { androidx.compose.foundation.interaction.MutableInteractionSource() },
                ) { /* consume taps inside the sheet: they must NOT dismiss */ }
                .clip(RoundedCornerShape(topStart = 16.dp, topEnd = 16.dp))
                .background(t.elevated)
                .border(1.dp, t.strokeTertiary, RoundedCornerShape(topStart = 16.dp, topEnd = 16.dp))
                .padding(vertical = 8.dp),
        ) {
            // Header (Panel titlebars): the sheet's title, hairline under.
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 10.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Text(
                    text = "Sessions",
                    style = androidx.compose.ui.text.TextStyle(
                        fontFamily = LocalFonts.current.sans,
                        fontSize = t.convFontSize.sp,
                        fontWeight = FontWeight.Medium,
                    ),
                    color = t.textSecondary,
                )
            }
            Box(
                Modifier
                    .fillMaxWidth()
                    .heightIn(min = 1.dp)
                    .background(t.strokeTertiary),
            )
            when {
                loading -> Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(24.dp),
                    horizontalArrangement = Arrangement.Center,
                ) {
                    CircularProgressIndicator(color = t.midground, modifier = Modifier.padding(4.dp))
                }
                else -> LazyColumn(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(max = 420.dp),
                ) {
                    // Always first — even when the gateway list is empty
                    // (fresh install / list failed). Hiding it behind
                    // sessions.isNotEmpty() made New chat unreachable.
                    item {
                        PickerRow(
                            title = "New chat",
                            preview = "Start a fresh conversation",
                            onClick = onNewChat,
                        )
                    }
                    if (sessions.isEmpty()) {
                        item {
                            Text(
                                text = "No other sessions on the gateway yet",
                                style = androidx.compose.ui.text.TextStyle(
                                    fontFamily = LocalFonts.current.sans,
                                    fontSize = t.convToolFontSize.sp,
                                ),
                                color = t.textTertiary,
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .padding(horizontal = 16.dp, vertical = 12.dp),
                            )
                        }
                    } else {
                        items(sessions, key = { it.id }) { s ->
                            PickerRow(
                                title = s.displayTitle,
                                preview = s.displayPreview,
                                onClick = { onResume(s.id) },
                            )
                        }
                    }
                }
            }
        }
    }
}

/** One picker row (Desktop's CommandItem shape): title line + preview line. */
@Composable
private fun PickerRow(title: String, preview: String, onClick: () -> Unit) {
    val t = LocalHermoTokens.current
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        // Desktop's MessageCircle icon is a plain dot here: no icon set in
        // the phone yet, and a dead-looking icon is worse than a quiet dot.
        Box(
            modifier = Modifier
                .padding(4.dp)
                .clip(CircleShape)
                .size(8.dp)
                .background(t.midground),
        )
        Column(Modifier.weight(1f)) {
            Text(
                text = title,
                style = androidx.compose.ui.text.TextStyle(
                    fontFamily = LocalFonts.current.sans,
                    fontSize = t.convFontSize.sp,
                ),
                color = t.text,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (preview.isNotBlank()) {
                Text(
                    text = preview,
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
    }
}
