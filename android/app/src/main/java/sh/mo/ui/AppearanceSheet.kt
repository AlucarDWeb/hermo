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
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import sh.mo.ThemeMode

/**
 * T13 appearance sheet: the theme selector (Light / Dark / System) as a
 * bottom sheet — the same three modes and labels as Desktop's `THEME_MODES`
 * (apps/desktop/src/app/command-palette/index.tsx:451) and `resolveMode`
 * (themes/context.tsx:51, `system` default). Declared divergence vs Desktop:
 * it lives in the command palette there, on the phone it rides the chat
 * titlebar's trailing control and renders as a bottom sheet (one-hand
 * reach, same shape as the session picker).
 *
 * The composable renders the [mode] it is HANDED and reports a choice up —
 * it never infers the current mode and never persists; the adapter owns
 * that (UiPrefs / MainActivity).
 */
@Composable
fun AppearanceSheet(
    mode: ThemeMode,
    onModeChange: (ThemeMode) -> Unit,
    onDismiss: () -> Unit,
) {
    val t = LocalHermoTokens.current
    Box(
        modifier = Modifier
            .fillMaxSize()
            .clickable(onClick = onDismiss) // the scrim tap dismisses
            .background(t.background.copy(alpha = 0.55f)),
        contentAlignment = Alignment.BottomCenter,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
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
                    text = "Appearance",
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
                    .background(t.strokeTertiary)
                    .padding(0.5.dp),
            )
            ThemeMode.values().forEach { candidate ->
                AppearanceRow(
                    label = candidate.name,
                    selected = candidate == mode,
                    onClick = {
                        onModeChange(candidate)
                        onDismiss()
                    },
                )
            }
        }
    }
}

/** One mode row: label + a dot on the selected one (the picker row shape). */
@Composable
private fun AppearanceRow(label: String, selected: Boolean, onClick: () -> Unit) {
    val t = LocalHermoTokens.current
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Column(Modifier.weight(1f)) {
            Text(
                text = label,
                style = androidx.compose.ui.text.TextStyle(
                    fontFamily = LocalFonts.current.sans,
                    fontSize = t.convFontSize.sp,
                ),
                color = t.text,
            )
        }
        if (selected) {
            Box(
                modifier = Modifier
                    .padding(4.dp)
                    .clip(CircleShape)
                    .size(8.dp)
                    .background(t.midground),
            )
        }
    }
}
