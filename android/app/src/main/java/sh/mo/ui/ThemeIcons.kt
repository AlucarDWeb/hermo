package sh.mo.ui

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.unit.dp
import sh.mo.ThemeMode

/**
 * FIX8 item 4 — theme-mode glyphs for the titlebar's appearance control.
 *
 * DECLARED DIVERGENCE from the brief's "core icon set" premise: the
 * material-icons-core aar (the set the app resolves today, verified against
 * the fetched artifact) carries only ~49 glyphs — Menu, Edit,
 * KeyboardArrowDown, … — and NO `Brightness*` icon, while adding the
 * extended artifact is a banned new maven pin. The three vectors below are
 * therefore built inline from the OFFICIAL Material path data (Apache-2.0,
 * google/material-design-icons `src/image/brightness_{4,6}` and the
 * baseline `brightness_auto`), which keeps the requirement — an M3 `Icon`
 * reflecting the CURRENT `themeMode`, no new dependency — intact.
 *
 * Desktop shows the same three states as its theme menu (themes/context.tsx:
 * light / dark / system); the phone surfaces them in the titlebar because
 * the appearance sheet's entry point lives there (T13).
 */
private fun themeGlyph(name: String, path: String): ImageVector =
    ImageVector.Builder(
        name = name,
        defaultWidth = 24.dp,
        defaultHeight = 24.dp,
        viewportWidth = 24f,
        viewportHeight = 24f,
    ).apply {
        addPath(pathData = addPathNodes(path), fill = SolidColor(Color.Black))
    }.build()

/** Brightness 6 — filled sun outline, dark inner disc (light mode). */
private const val BRIGHTNESS_LIGHT_PATH =
    "M20 15.31L23.31 12 20 8.69V4h-4.69L12 .69 8.69 4H4v4.69L.69 12 " +
        "4 15.31V20h4.69L12 23.31 15.31 20H20v-4.69zM12 18V6c3.31 0 6 2.69 6 6s-2.69 6-6 6z"

/** Brightness 4 — sun outline with the half-moon cut (dark mode). */
private const val BRIGHTNESS_DARK_PATH =
    "M20 8.69V4h-4.69L12 .69 8.69 4H4v4.69L.69 12 4 15.31V20h4.69L12 23.31 " +
        "15.31 20H20v-4.69L23.31 12 20 8.69zM12 18c-.89 0-1.74-.2-2.5-.55C11.56 16.5 " +
        "13 14.42 13 12s-1.44-4.5-3.5-5.45C10.26 6.2 11.11 6 12 6c3.31 0 6 2.69 6 6s-2.69 6-6 6z"

/** Brightness auto — sun outline with the "A" (system mode). */
private const val BRIGHTNESS_AUTO_PATH =
    "M10.85 12.65h2.3L12 9l-1.15 3.65zM20 8.69V4h-4.69L12 .69L8.69 4H4v4.69L.69 " +
        "12L4 15.31V20h4.69L12 23.31L15.31 20H20v-4.69L23.31 12L20 8.69zM14.3 16l-.7-2h-3.2l-.7 " +
        "2H7.8L11 7h2l3.2 9h-1.9z"

/** The glyph for the CURRENT mode: light → 6, dark → 4, system → auto. */
fun themeIcon(mode: ThemeMode): ImageVector = when (mode) {
    ThemeMode.Light -> themeGlyph("Brightness6", BRIGHTNESS_LIGHT_PATH)
    ThemeMode.Dark -> themeGlyph("Brightness4", BRIGHTNESS_DARK_PATH)
    ThemeMode.System -> themeGlyph("BrightnessAuto", BRIGHTNESS_AUTO_PATH)
}
