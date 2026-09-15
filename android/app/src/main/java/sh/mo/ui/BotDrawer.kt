package sh.mo.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import sh.mo.BotDrawerRow
import sh.mo.DrawerUiState

/**
 * T16c: the bot drawer's content — a Material 3 `ModalDrawerSheet` listing
 * one row per Hermes profile (`list_profiles`), the phone-native equivalent
 * of Desktop's Bot Mode sidebar (the sidebar + cards panel does not fit the
 * phone; DECLARED divergence, per the brief).
 *
 * One row: `name` (headline), then `model` and `description` as supporting
 * lines, each only when non-empty. ALL profiles the core returns are shown —
 * `GET /api/profiles` carries NO `hidden` flag (declared gap, not filtered).
 * While the load runs: a progress row; on failure: an error row with a Retry
 * — no silent empty drawer.
 *
 * Tokens only (`LocalHermoTokens`); M3 components only.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun BotDrawerContent(
    state: DrawerUiState,
    onProfileTap: (BotDrawerRow) -> Unit,
    onRetry: () -> Unit,
) {
    val t = LocalHermoTokens.current
    ModalDrawerSheet(
        drawerContainerColor = t.elevated,
    ) {
        Column(
            modifier = Modifier
                .statusBarsPadding()
                .fillMaxWidth()
                .verticalScroll(rememberScrollState())
                .padding(vertical = 8.dp),
        ) {
            Text(
                text = "Bots",
                style = androidx.compose.ui.text.TextStyle(
                    fontFamily = LocalFonts.current.sans,
                    fontSize = t.convFontSize.sp,
                    fontWeight = FontWeight.Medium,
                ),
                color = t.textSecondary,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
            )
            when (state) {
                DrawerUiState.Loading -> Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 16.dp, vertical = 16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    CircularProgressIndicator(
                        modifier = Modifier.padding(end = 12.dp),
                        color = t.midground,
                    )
                    Text(
                        text = "Loading profiles…",
                        style = androidx.compose.ui.text.TextStyle(
                            fontFamily = LocalFonts.current.sans,
                            fontSize = t.convToolFontSize.sp,
                        ),
                        color = t.textTertiary,
                    )
                }
                is DrawerUiState.Failed -> Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 16.dp),
                ) {
                    Text(
                        text = state.message,
                        style = androidx.compose.ui.text.TextStyle(
                            fontFamily = LocalFonts.current.sans,
                            fontSize = t.convToolFontSize.sp,
                        ),
                        color = t.destructive,
                        maxLines = 3,
                        overflow = TextOverflow.Ellipsis,
                    )
                    TextButton(onClick = onRetry) {
                        Text(
                            text = "Retry",
                            style = androidx.compose.ui.text.TextStyle(
                                fontFamily = LocalFonts.current.sans,
                                fontSize = t.convToolFontSize.sp,
                            ),
                        )
                    }
                }
                is DrawerUiState.Ready -> {
                    if (state.rows.isEmpty()) {
                        Text(
                            text = "No profiles on this gateway",
                            style = androidx.compose.ui.text.TextStyle(
                                fontFamily = LocalFonts.current.sans,
                                fontSize = t.convToolFontSize.sp,
                            ),
                            color = t.textTertiary,
                            modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                        )
                    }
                    state.rows.forEach { row -> BotDrawerItem(row = row, onTap = { onProfileTap(row) }) }
                }
            }
        }
    }
}

/** One profile row: name headline, model/description supporting when non-empty. */
@Composable
private fun BotDrawerItem(row: BotDrawerRow, onTap: () -> Unit) {
    val t = LocalHermoTokens.current
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onTap)
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        Text(
            text = row.name,
            style = androidx.compose.ui.text.TextStyle(
                fontFamily = LocalFonts.current.sans,
                fontSize = t.convFontSize.sp,
                fontWeight = FontWeight.Medium,
            ),
            color = t.text,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        if (row.model.isNotEmpty()) {
            Text(
                text = row.model,
                style = androidx.compose.ui.text.TextStyle(
                    fontFamily = LocalFonts.current.mono,
                    fontSize = t.convToolFontSize.sp,
                ),
                color = t.textTertiary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        if (row.description.isNotEmpty()) {
            Text(
                text = row.description,
                style = androidx.compose.ui.text.TextStyle(
                    fontFamily = LocalFonts.current.sans,
                    fontSize = t.convToolFontSize.sp,
                ),
                color = t.scaffoldMeta,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}
