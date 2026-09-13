package sh.mo.ui

import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * hermo theme — Hermes Desktop's tokens (`apps/desktop/src/styles.css` +
 * DESIGN.md "Stroke & color tokens" / "Surfaces & elevation" / "Chat, tools &
 * boot surfaces"), mirrored into Compose with the DESKTOP TOKEN NAMES KEPT so
 * the mapping is auditable line by line. No colour, radius or spacing here is
 * invented: every value is a Desktop token's resolved value (`--dt-*` /
 * `--ui-*` / `--theme-*`), computed from the same `color-mix()` chains the CSS
 * runs (T7 divergence 1: BOTH modes now exist).
 *
 * Light set — the default `:root` values (T7a's, unchanged):
 *   nous skin light seeds: bg #ffffff, fg #1f2328, card #f6f8fa, popover #fff,
 *   primary #0053fd … over neutral-chrome #f3f3f3 / neutral-card #fcfcfc.
 * Dark set — the `:root.dark` chain over the nous skin's darkColors
 *   (themes/presets.ts `nousTheme.darkColors`: bg #0d1117, fg #e6edf3,
 *   card #010409, popover #161b22, primary #4a84fe, …, neutral-chrome
 *   #0d0d0e, neutral-card #161618):
 *   chrome      = mix(bg 74%, neutral-chrome)            #0d1015
 *   editor      = mix(card 38%, neutral-card)            #0e0f12
 *   elevated    = mix(popover 46%, neutral-card)         #16181d
 *   widget      = mix(editor 88%, #000)  (the .dark override)
 *   tertiary    = accent 8% + base 5% over chrome        #1c2332
 *   quaternary  = accent 5% + base 4% over chrome        #191e29
 *   strokeSec   = accent 16% + base 7% over chrome       #232f48
 *   strokeTert  = accent 10% + base 5% over chrome       #1d2636
 *   strokeQuat  = accent 6% + base 3% over chrome        #171e2a
 *   secondary   = accent 7% over chrome                  #111825
 *   accentSoft  = accent 10% over chrome                 #131c2c
 *   destructive #f85149 (darkColors), onPrimary/primary-foreground #161616
 *     and the LOUD primary: `ensureContrast(primary, #fcfcfc, 4.5)` → #2369fe.
 *
 * Which mode paints: Desktop resolves light/dark from the user's choice with
 * `system` as the default — the phone's equivalent is `isSystemInDarkTheme()`
 * (T7 divergence 1). Not reproducible on the phone (stated, not silently
 * dropped): the translucent "window glass" chrome (backdrop blur), hover-only
 * fills (`--chrome-action-hover`), and theme skins — the phone pins the
 * default `nous` skin's resolved values for the mode it paints.
 */

// ── light seeds (styles.css :root lines 172-200; nous light, presets 178-204) ─
private object LightTokens : HermoTokens {
    override val background = Color(0xFFF8F9FE)      // --ui-bg-chrome (bg 92% / #f3f3f3)
    override val surface = Color(0xFFFDFDFD)         // --ui-bg-editor (card 22% / #fcfcfc)
    override val elevated = Color(0xFFFDFDFD)        // --ui-bg-elevated (popover 28% / #fcfcfc)
    override val widgetSurface = surface             // --ui-widget-surface-background (light)
    override val midground = Color(0xFF0053FD)       // --theme-midground / --ui-accent
    override val primary = Color(0xFF0053FD)         // --theme-primary / --dt-primary
    override val primarySolid = Color(0xFF0053FD)    // accent deep enough for #fcfcfc already
    override val onPrimary = Color(0xFFFCFCFC)       // --dt-primary-foreground
    override val secondary = Color(0xFFEDF3FF)       // --theme-secondary (accent 7% over surface)
    override val onSecondary = Color(0xFF2B2F37)
    override val accent = Color(0xFFE6EEFF)          // --theme-accent-soft (accent 10% over surface)
    override val softFill = Color(0xFFEAEFF6)        // --ui-bg-quaternary (accent 5% + base 4%)
    override val muted = Color(0xFFE0E6F5)           // --ui-bg-tertiary (accent 8% + base 5%)
    override val border = Color(0xFFC9D6F1)          // --ui-stroke-secondary / --dt-border
    override val strokeTertiary = Color(0xFFDBE3F5)  // --ui-stroke-tertiary
    override val strokeQuaternary = Color(0xFFE9EEF8)
    override val text = Color(0xFF17171A).copy(alpha = 0.94f)      // --ui-text-primary
    override val textSecondary = Color(0xFF17171A).copy(alpha = 0.74f)
    override val textTertiary = Color(0xFF17171A).copy(alpha = 0.54f)
    override val scaffoldText = Color(0xFF17171A).copy(alpha = 0.64f) // --conversation-scaffold-text
    override val scaffoldMeta = Color(0xFF17171A).copy(alpha = 0.44f) // --conversation-scaffold-meta
    override val destructive = Color(0xFFCF2D56)     // --dt-destructive (--ui-red)
    override val onDestructive = Color(0xFFFFFFFF)
    override val userBubble = Color(0xFFDAE7FD)       // presets.ts:203 nousTheme.lightColors.userBubble
    override val userBubbleBorder = Color(0xFFD0D7DE) // presets.ts:204 nousTheme.lightColors.userBubbleBorder
    override val monoFont: FontFamily = FontFamily.Monospace
}

// ── dark seeds (styles.css :root.dark lines 550-580; nous dark, presets 206-237) ─
private object DarkTokens : HermoTokens {
    override val background = Color(0xFF0D1015)      // --ui-bg-chrome (bg 74% / #0d0d0e)
    override val surface = Color(0xFF0E0F12)         // --ui-bg-editor (card 38% / #161618)
    override val elevated = Color(0xFF16181D)        // --ui-bg-elevated (popover 46% / #161618)
    override val widgetSurface = Color(0xFF0C0D10)   // .dark: editor 88% / #000
    override val midground = Color(0xFF4A84FE)       // darkColors.midground / --ui-accent
    override val primary = Color(0xFF4A84FE)
    override val primarySolid = Color(0xFF2369FE)    // ensureContrast(primary, #fcfcfc, 4.5)
    override val onPrimary = Color(0xFF161616)       // darkColors.primaryForeground
    override val secondary = Color(0xFF111825)       // accent 7% over chrome (darkColors.secondary at rest)
    override val onSecondary = Color(0xFFE6EDF3)
    override val accent = Color(0xFF131C2C)          // accent 10% over chrome
    override val softFill = Color(0xFF191E29)        // --ui-bg-quaternary (accent 5% + base 4%)
    override val muted = Color(0xFF1C2332)           // --ui-bg-tertiary (accent 8% + base 5%)
    override val border = Color(0xFF232F48)          // --ui-stroke-secondary / --dt-border
    override val strokeTertiary = Color(0xFF1D2636)  // --ui-stroke-tertiary
    override val strokeQuaternary = Color(0xFF171E2A)
    override val text = Color(0xFFE6EDF3).copy(alpha = 0.94f)
    override val textSecondary = Color(0xFFE6EDF3).copy(alpha = 0.74f)
    override val textTertiary = Color(0xFFE6EDF3).copy(alpha = 0.54f)
    override val scaffoldText = Color(0xFFE6EDF3).copy(alpha = 0.64f)
    override val scaffoldMeta = Color(0xFFE6EDF3).copy(alpha = 0.44f)
    override val destructive = Color(0xFFF85149)     // darkColors.destructive (--ui-red dark)
    override val onDestructive = Color(0xFFFFFFFF)
    override val userBubble = Color(0xFF07162C)       // presets.ts:231 nousTheme.darkColors.userBubble
    override val userBubbleBorder = Color(0xFF30363D) // presets.ts:232 nousTheme.darkColors.userBubbleBorder
    override val monoFont: FontFamily = FontFamily.Monospace
}

/**
 * The Desktop token surface, under its own names. Compose code reads
 * `HermoTheme.tokens` and never a bare Material color, so a Desktop token
 * greps to exactly one definition here. An interface over two sealed
 * objects keeps both token sets TOTAL and immutable.
 */
interface HermoTokens {
    // Stroke & color tokens (DESIGN.md table, verbatim names).
    val surface: Color          // --ui-bg-editor: the chat surface
    val background: Color       // --ui-bg-chrome
    val elevated: Color         // --ui-bg-elevated
    val widgetSurface: Color    // --ui-widget-surface-background
    val midground: Color        // --theme-midground / --ui-accent
    val primary: Color          // --theme-primary / --dt-primary
    val primarySolid: Color     // --dt-primary-solid (the loud fill)
    val onPrimary: Color        // --dt-primary-foreground
    val secondary: Color        // --theme-secondary
    val onSecondary: Color
    val accent: Color           // --theme-accent-soft
    val softFill: Color         // --ui-bg-quaternary (secondary button)
    val muted: Color            // --ui-bg-tertiary / --color-muted
    val border: Color           // --ui-stroke-secondary / --dt-border
    val strokeTertiary: Color   // transcript hairlines/fences
    val strokeQuaternary: Color
    val text: Color             // --ui-text-primary
    val textSecondary: Color    // --ui-text-secondary
    val textTertiary: Color     // --ui-text-tertiary
    val scaffoldText: Color     // --conversation-scaffold-text
    val scaffoldMeta: Color     // --conversation-scaffold-meta
    val destructive: Color      // --dt-destructive
    val onDestructive: Color
    val userBubble: Color       // presets.ts userBubble (nousTheme light/dark)
    val userBubbleBorder: Color // presets.ts userBubbleBorder (nousTheme light/dark)

    /** The chat surface's `--font-mono` (T7 divergence 2: mirrored, not
     *  hardcoded — the platform's mono face stands in for Menlo/Monaco/SF Mono). */
    val monoFont: FontFamily

    // Conversation typography/spacing knobs (styles.css 474-496).
    val convFontSize: Float get() = 13f         // --conversation-text-font-size: 0.8125rem
    val convToolFontSize: Float get() = 11f     // --conversation-tool-font-size: 0.6875rem
    val convLineHeight: Float get() = 18f       // --conversation-line-height: 1.125rem
    val turnGapDp: Float get() = 6f             // --conversation-turn-gap: 0.375rem
    val turnBlockGapDp: Float get() = 12f       // --turn-block-gap: 0.75rem
    val scaffoldBlockGapDp: Float get() = 4f    // --scaffold-block-gap: turn-block-gap/3
    val paragraphGapDp: Float get() = 11.2f     // --paragraph-gap: 0.7rem
    val messageIndentDp: Float get() = 12f      // --message-text-indent: 0.75rem
    val radiusSm: Float get() = 1.6f            // --radius-sm  (scalar 0.2 × rem×16)
    val radiusMd: Float get() = 2f              // --radius-md
    val radiusLg: Float get() = 2.4f            // --radius-lg (the composer shell)
    val radiusXl: Float get() = 3.2f            // --radius-xl
    val radius2xl: Float get() = 4.8f           // --radius-2xl
    val radius3xl: Float get() = 6.4f           // --radius-3xl (widget shell)
    val radius4xl: Float get() = 8f             // --radius-4xl

    // Composer shell & controls (styles.css 492-497; model-pill.tsx:26 max-w-40).
    val composerControlGapDp: Float get() = 4f       // --composer-control-gap: 0.25rem
    val composerSurfacePadXDp: Float get() = 8f      // --composer-surface-pad-x: 0.5rem
    val composerSurfacePadYDp: Float get() = 5f      // --composer-surface-pad-y: 0.3125rem
    val composerPillMaxWidthDp: Float get() = 160f   // model-pill.tsx:26 'max-w-40' (10rem)
}

val LocalHermoTokens = staticCompositionLocalOf<HermoTokens> { LightTokens }

/** --font-sans / --font-mono resolved to platform families (system faces). */
val LocalFonts = staticCompositionLocalOf { Fonts(sans = FontFamily.SansSerif, mono = FontFamily.Monospace) }

data class Fonts(val sans: FontFamily, val mono: FontFamily)

private fun hermoColorScheme(t: HermoTokens): ColorScheme =
    ColorScheme(
        primary = t.primary,
        onPrimary = t.onPrimary,
        primaryContainer = t.accent,
        onPrimaryContainer = t.text,
        inversePrimary = t.primary,
        secondary = t.secondary,
        onSecondary = t.onSecondary,
        secondaryContainer = t.softFill,
        onSecondaryContainer = t.text,
        tertiary = t.midground,
        onTertiary = t.onPrimary,
        tertiaryContainer = t.accent,
        onTertiaryContainer = t.text,
        background = t.background,
        onBackground = t.text,
        surface = t.surface,
        onSurface = t.text,
        surfaceVariant = t.muted,
        onSurfaceVariant = t.textSecondary,
        surfaceTint = t.surface,
        inverseSurface = t.text,
        inverseOnSurface = t.surface,
        error = t.destructive,
        onError = t.onDestructive,
        errorContainer = t.destructive.copy(alpha = 0.12f),
        onErrorContainer = t.destructive,
        outline = t.border,
        outlineVariant = t.strokeTertiary,
        scrim = Color.Black.copy(alpha = 0.5f),
        surfaceBright = t.elevated,
        surfaceDim = t.background,
        surfaceContainer = t.elevated,
        surfaceContainerHigh = t.elevated,
        surfaceContainerHighest = t.elevated,
        surfaceContainerLow = t.surface,
        surfaceContainerLowest = if (t.background.luminanceCompat() > 0.5f) Color.White else t.surface,
    )

private fun Color.luminanceCompat(): Float =
    0.2126f * red + 0.7152f * green + 0.0722f * blue

/** --dt-base-size 1rem / --dt-line-height 1.5; conversation sizes per token. */
private fun hermoTypography(t: HermoTokens, fonts: Fonts): Typography = Typography(
    headlineLarge = TextStyle(fontFamily = fonts.sans, fontWeight = FontWeight.SemiBold, fontSize = 28.sp, lineHeight = 36.sp),
    titleMedium = TextStyle(fontFamily = fonts.sans, fontWeight = FontWeight.SemiBold, fontSize = 16.sp, lineHeight = 22.sp),
    bodyMedium = TextStyle(fontFamily = fonts.sans, fontSize = t.convFontSize.sp, lineHeight = t.convLineHeight.sp),
    labelSmall = TextStyle(fontFamily = fonts.sans, fontWeight = FontWeight.Medium, fontSize = t.convToolFontSize.sp, lineHeight = 14.sp),
)

@Composable
fun HermoTheme(content: @Composable () -> Unit) {
    // Desktop resolves light/dark from the user's choice, defaulting to
    // `system` (themes/context.tsx resolveMode) — the phone equivalent is the
    // system dark setting (T7 divergence 1).
    val tokens = if (isSystemInDarkTheme()) DarkTokens else LightTokens
    val fonts = Fonts(sans = FontFamily.SansSerif, mono = tokens.monoFont)
    CompositionLocalProvider(LocalHermoTokens provides tokens, LocalFonts provides fonts) {
        MaterialTheme(
            colorScheme = hermoColorScheme(tokens),
            typography = hermoTypography(tokens, fonts),
            content = content,
        )
    }
}

/** Rounded radius from the token scale — the only corner-radius entry point. */
fun hermoRadius(dp: Float) = RoundedCornerShape(dp.dp)
