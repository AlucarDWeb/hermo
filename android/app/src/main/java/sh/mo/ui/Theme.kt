package sh.mo.ui

import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.staticCompositionLocalOf
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
 * invented: every value is the Desktop token's resolved light-theme value
 * (`--dt-*` / `--ui-*` / `--theme-*`), with the alpha-carrying text and stroke
 * tokens expressed as Colour(alpha=…) over the same base the CSS mixes over.
 *
 * Not reproducible on the phone (stated, not silently dropped): the desktop's
 * translucent "window glass" chrome (backdrop blur), hover-only fills
 * (`--chrome-action-hover`), and theme skins — the phone port pins the
 * default light theme's resolved values (dark-theme mirrors land with the
 * desktop dark review; `isSystemInDarkTheme` currently maps to the same
 * tokens, as the desktop's default install does until a skin is chosen).
 */

// ── theme seeds (styles.css :root, lines 174-199) ────────────────────────
// --theme-primary: #0053fd; --theme-foreground: #17171a;
// --theme-neutral-chrome: #f3f3f3; --theme-background-seed: #f8faff;
// --theme-neutral-card: #fcfcfc; --theme-card-seed: #ffffff.
private val Accent = Color(0xFF0053FD)          // --theme-primary
private val Base = Color(0xFF17171A)            // --theme-foreground (mix base)
private val White = Color(0xFFFFFFFF)

// Resolved mixes (the CSS `color-mix(in srgb, seed X%, neutral)` values,
// computed, not eyeballed):
private val SurfaceChrome = Color(0xFFF8F9FE)   // --ui-bg-chrome / --color-background
private val CardEditor = Color(0xFFFDFDFD)      // --ui-bg-editor / --color-card / surface
private val BgElevated = Color(0xFFFDFDFD)      // --ui-bg-elevated
private val BgTertiary = Color(0xFFDFE6F3)      // --ui-bg-tertiary / --color-muted
private val BgQuaternary = Color(0xFFE9EDF6)    // --ui-bg-quaternary (soft control fill)

// --ui-text-* : color-mix(ui-base N%, transparent) → alpha over the surface.
private val TextPrimary = Base.copy(alpha = 0.94f)    // --ui-text-primary
private val TextSecondary = Base.copy(alpha = 0.74f)  // --ui-text-secondary
private val TextTertiary = Base.copy(alpha = 0.54f)   // --ui-text-tertiary
private val ScaffoldText = Base.copy(alpha = 0.64f)   // --conversation-scaffold-text
private val ScaffoldMeta = Base.copy(alpha = 0.44f)   // --conversation-scaffold-meta

// --ui-stroke-* : accent X% + ui-base Y% + transparent → resolved over white.
private val StrokePrimary = Color(0xFFABBFE8)   // --ui-stroke-primary
private val StrokeSecondary = Color(0xFFC6D3EF) // --ui-stroke-secondary (--dt-border)
private val StrokeTertiary = Color(0xFFDAE2F3)  // --ui-stroke-tertiary (transcript hairlines)
private val StrokeQuaternary = Color(0xFFE9EEF8)

private val Destructive = Color(0xFFCF2D56)     // --dt-destructive
private val DestructiveForeground = Color(0xFFFFFFFF)
private val PrimaryForeground = Color(0xFFFCFCFC) // --dt-primary-foreground
private val Secondary = Color(0xFFEDF3FF)       // --theme-secondary (accent 7% white)
private val SecondaryForeground = TextSecondary
private val AccentSoft = Color(0xFFE6EEFF)      // --theme-accent-soft (accent 10% white)
private val WidgetSurface = CardEditor          // --ui-widget-surface-background

/**
 * The Desktop token surface, under its own names. Compose code reads
 * `HermoTheme.tokens` and never a bare Material color, so a Desktop token
 * greps to exactly one definition here.
 */
data class HermoTokens(
    // Stroke & color tokens (DESIGN.md table, verbatim names).
    val surface: Color = CardEditor,          // --ui-bg-editor: the chat surface
    val background: Color = SurfaceChrome,    // --ui-bg-chrome
    val elevated: Color = BgElevated,         // --ui-bg-elevated
    val midground: Color = Accent,            // --theme-midground / --ui-accent
    val border: Color = StrokeSecondary,      // --dt-border
    val strokeTertiary: Color = StrokeTertiary, // transcript hairlines/fences
    val strokeQuaternary: Color = StrokeQuaternary,
    val text: Color = TextPrimary,            // --ui-text-primary
    val textSecondary: Color = TextSecondary, // --ui-text-secondary
    val textTertiary: Color = TextTertiary,   // --ui-text-tertiary
    val scaffoldText: Color = ScaffoldText,   // --conversation-scaffold-text
    val scaffoldMeta: Color = ScaffoldMeta,   // --conversation-scaffold-meta
    val primary: Color = Accent,              // --theme-primary / --dt-primary
    val onPrimary: Color = PrimaryForeground, // --dt-primary-foreground
    val softFill: Color = BgQuaternary,       // --ui-bg-quaternary (secondary button)
    val widgetSurface: Color = WidgetSurface, // --ui-widget-surface-background
    val secondary: Color = Secondary,         // --theme-secondary
    val onSecondary: Color = SecondaryForeground,
    val accent: Color = AccentSoft,           // --theme-accent-soft
    val destructive: Color = Destructive,     // --dt-destructive
    val onDestructive: Color = DestructiveForeground,
    // Conversation typography/spacing knobs (styles.css 474-496).
    val convFontSize: Float = 13f,            // --conversation-text-font-size: 0.8125rem
    val convToolFontSize: Float = 11f,        // --conversation-tool-font-size: 0.6875rem
    val convLineHeight: Float = 18f,          // --conversation-line-height: 1.125rem
    val turnGapDp: Float = 6f,                // --conversation-turn-gap: 0.375rem
    val turnBlockGapDp: Float = 12f,          // --turn-block-gap: 0.75rem
    val paragraphGapDp: Float = 11.2f,        // --paragraph-gap: 0.7rem
    val messageIndentDp: Float = 12f,         // --message-text-indent: 0.75rem
    val radiusSm: Float = 1.6f,               // --radius-sm  (scalar 0.2 × rem×16)
    val radiusMd: Float = 2f,                 // --radius-md
    val radiusLg: Float = 2.4f,               // --radius-lg (the composer shell)
    val radiusXl: Float = 3.2f,               // --radius-xl
    val radius2xl: Float = 4.8f,              // --radius-2xl
    val radius3xl: Float = 6.4f,              // --radius-3xl (widget shell)
    val radius4xl: Float = 8f,                // --radius-4xl
)

val LocalHermoTokens = staticCompositionLocalOf { HermoTokens() }

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
        surfaceVariant = BgTertiary,
        onSurfaceVariant = t.textSecondary,
        surfaceTint = t.surface,
        inverseSurface = Base,
        inverseOnSurface = White,
        error = t.destructive,
        onError = t.onDestructive,
        errorContainer = t.destructive.copy(alpha = 0.12f),
        onErrorContainer = t.destructive,
        outline = t.border,
        outlineVariant = t.strokeTertiary,
        scrim = Base.copy(alpha = 0.5f),
        surfaceBright = t.elevated,
        surfaceDim = t.background,
        surfaceContainer = t.elevated,
        surfaceContainerHigh = t.elevated,
        surfaceContainerHighest = t.elevated,
        surfaceContainerLow = t.surface,
        surfaceContainerLowest = White,
    )

/** --dt-base-size 1rem / --dt-line-height 1.5; conversation sizes per token. */
private fun hermoTypography(t: HermoTokens, fonts: Fonts): Typography = Typography(
    headlineLarge = TextStyle(fontFamily = fonts.sans, fontWeight = FontWeight.SemiBold, fontSize = 28.sp, lineHeight = 36.sp),
    titleMedium = TextStyle(fontFamily = fonts.sans, fontWeight = FontWeight.SemiBold, fontSize = 16.sp, lineHeight = 22.sp),
    bodyMedium = TextStyle(fontFamily = fonts.sans, fontSize = t.convFontSize.sp, lineHeight = t.convLineHeight.sp),
    labelSmall = TextStyle(fontFamily = fonts.sans, fontWeight = FontWeight.Medium, fontSize = t.convToolFontSize.sp, lineHeight = 14.sp),
)

@Composable
fun HermoTheme(content: @Composable () -> Unit) {
    // Dark: the desktop's own default is light until a skin/theme is chosen;
    // the tokens are pinned to that default (stated divergence, see header).
    val tokens = HermoTokens()
    val fonts = Fonts(sans = FontFamily.SansSerif, mono = FontFamily.Monospace)
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
