import SwiftUI

extension Color {
    /// Matches Compose's `Color(0xAARRGGBBL)` argument order so every token below is a straight copy of its Kotlin literal.
    fileprivate init(hermoHex value: UInt32) {
        let a = Double((value >> 24) & 0xFF) / 255
        let r = Double((value >> 16) & 0xFF) / 255
        let g = Double((value >> 8) & 0xFF) / 255
        let b = Double(value & 0xFF) / 255
        self.init(.sRGB, red: r, green: g, blue: b, opacity: a)
    }
}

public struct HermoTokens: Sendable {
    public let background: Color
    public let surface: Color
    public let elevated: Color
    public let widgetSurface: Color
    public let midground: Color
    public let primary: Color
    public let primarySolid: Color
    public let onPrimary: Color
    public let secondary: Color
    public let onSecondary: Color
    public let accent: Color
    public let softFill: Color
    public let muted: Color
    public let border: Color
    public let strokeTertiary: Color
    public let strokeQuaternary: Color
    public let text: Color
    public let textSecondary: Color
    public let textTertiary: Color
    public let scaffoldText: Color
    public let scaffoldMeta: Color
    public let destructive: Color
    public let onDestructive: Color
    public let userBubble: Color
    public let userBubbleBorder: Color
    public let successDot: Color

    /// Theme.kt:242 `errorContainer`: the destructive colour at 12 percent.
    public var errorContainer: Color { destructive.opacity(0.12) }

    /// Theme.kt:246 `scrim`: behind the bot drawer and the sheets. Compose gets this from
    /// `MaterialTheme.colorScheme`; SwiftUI has no equivalent, so it has to be a token or it
    /// becomes a literal at every call site.
    public var scrim: Color { Color.black.opacity(0.5) }

    public static let light = HermoTokens(
        background: Color(hermoHex: 0xFFF8F9FE),           // --ui-bg-chrome (bg 92% / #f3f3f3)
        surface: Color(hermoHex: 0xFFFDFDFD),               // --ui-bg-editor (card 22% / #fcfcfc)
        elevated: Color(hermoHex: 0xFFFDFDFD),              // --ui-bg-elevated (popover 28% / #fcfcfc)
        widgetSurface: Color(hermoHex: 0xFFFDFDFD),         // --ui-widget-surface-background (light)
        midground: Color(hermoHex: 0xFF0053FD),             // --theme-midground / --ui-accent
        primary: Color(hermoHex: 0xFF0053FD),               // --theme-primary / --dt-primary
        primarySolid: Color(hermoHex: 0xFF0053FD),
        onPrimary: Color(hermoHex: 0xFFFCFCFC),             // --dt-primary-foreground
        secondary: Color(hermoHex: 0xFFEDF3FF),             // --theme-secondary (accent 7% over surface)
        onSecondary: Color(hermoHex: 0xFF2B2F37),
        accent: Color(hermoHex: 0xFFE6EEFF),                // --theme-accent-soft (accent 10% over surface)
        softFill: Color(hermoHex: 0xFFEAEFF6),              // --ui-bg-quaternary (accent 5% + base 4%)
        muted: Color(hermoHex: 0xFFE0E6F5),                 // --ui-bg-tertiary (accent 8% + base 5%)
        border: Color(hermoHex: 0xFFC9D6F1),                // --ui-stroke-secondary / --dt-border
        strokeTertiary: Color(hermoHex: 0xFFDBE3F5),        // --ui-stroke-tertiary
        strokeQuaternary: Color(hermoHex: 0xFFE9EEF8),
        text: Color(hermoHex: 0xFF17171A).opacity(0.94),                // --ui-text-primary
        textSecondary: Color(hermoHex: 0xFF17171A).opacity(0.74),
        textTertiary: Color(hermoHex: 0xFF17171A).opacity(0.54),
        scaffoldText: Color(hermoHex: 0xFF17171A).opacity(0.64),        // --conversation-scaffold-text
        scaffoldMeta: Color(hermoHex: 0xFF17171A).opacity(0.44),        // --conversation-scaffold-meta
        destructive: Color(hermoHex: 0xFFCF2D56),           // --dt-destructive (--ui-red)
        onDestructive: Color(hermoHex: 0xFFFFFFFF),
        userBubble: Color(hermoHex: 0xFFDAE7FD),            // presets.ts:203 nousTheme.lightColors.userBubble
        userBubbleBorder: Color(hermoHex: 0xFFD0D7DE),      // presets.ts:204 nousTheme.lightColors.userBubbleBorder
        successDot: Color(hermoHex: 0xFF2AA17C)
    )

    public static let dark = HermoTokens(
        background: Color(hermoHex: 0xFF0D1015),            // --ui-bg-chrome (bg 74% / #0d0d0e)
        surface: Color(hermoHex: 0xFF0E0F12),                // --ui-bg-editor (card 38% / #161618)
        elevated: Color(hermoHex: 0xFF16181D),               // --ui-bg-elevated (popover 46% / #161618)
        widgetSurface: Color(hermoHex: 0xFF0C0D10),
        midground: Color(hermoHex: 0xFF4A84FE),              // darkColors.midground / --ui-accent
        primary: Color(hermoHex: 0xFF4A84FE),
        primarySolid: Color(hermoHex: 0xFF2369FE),
        onPrimary: Color(hermoHex: 0xFF161616),
        secondary: Color(hermoHex: 0xFF111825),
        onSecondary: Color(hermoHex: 0xFFE6EDF3),
        accent: Color(hermoHex: 0xFF131C2C),
        softFill: Color(hermoHex: 0xFF191E29),               // --ui-bg-quaternary (accent 5% + base 4%)
        muted: Color(hermoHex: 0xFF1C2332),                  // --ui-bg-tertiary (accent 8% + base 5%)
        border: Color(hermoHex: 0xFF232F48),                 // --ui-stroke-secondary / --dt-border
        strokeTertiary: Color(hermoHex: 0xFF1D2636),         // --ui-stroke-tertiary
        strokeQuaternary: Color(hermoHex: 0xFF171E2A),
        text: Color(hermoHex: 0xFFE6EDF3).opacity(0.94),
        textSecondary: Color(hermoHex: 0xFFE6EDF3).opacity(0.74),
        textTertiary: Color(hermoHex: 0xFFE6EDF3).opacity(0.54),
        scaffoldText: Color(hermoHex: 0xFFE6EDF3).opacity(0.64),
        scaffoldMeta: Color(hermoHex: 0xFFE6EDF3).opacity(0.44),
        destructive: Color(hermoHex: 0xFFF85149),            // darkColors.destructive (--ui-red dark)
        onDestructive: Color(hermoHex: 0xFFFFFFFF),
        userBubble: Color(hermoHex: 0xFF07162C),             // presets.ts:231 nousTheme.darkColors.userBubble
        userBubbleBorder: Color(hermoHex: 0xFF30363D),       // presets.ts:232 nousTheme.darkColors.userBubbleBorder
        successDot: Color(hermoHex: 0xFF2AA17C)
    )
}

public enum HermoMetrics {
    public static let convFontSize: CGFloat = 17
    public static let convToolFontSize: CGFloat = 13
    public static let convLineHeight: CGFloat = 24

    public static let headlineLargeFontSize: CGFloat = 30
    public static let headlineLargeLineHeight: CGFloat = 36
    public static let titleMediumFontSize: CGFloat = 18
    public static let titleMediumLineHeight: CGFloat = 22
    public static let labelSmallLineHeight: CGFloat = 16

    public static let turnGap: CGFloat = 6                    // --conversation-turn-gap: 0.375rem
    public static let turnBlockGap: CGFloat = 12              // --turn-block-gap: 0.75rem
    public static let scaffoldBlockGap: CGFloat = 4           // --scaffold-block-gap: turn-block-gap/3
    public static let paragraphGap: CGFloat = 11.2            // --paragraph-gap: 0.7rem
    public static let messageIndent: CGFloat = 12             // --message-text-indent: 0.75rem

    public static let radiusSm: CGFloat = 1.6                 // --radius-sm  (scalar 0.2 × rem×16)
    public static let radiusMd: CGFloat = 2                   // --radius-md
    public static let radiusLg: CGFloat = 2.4                 // --radius-lg (the composer shell)
    public static let radiusXl: CGFloat = 3.2                 // --radius-xl
    public static let radius2xl: CGFloat = 4.8                // --radius-2xl
    public static let radius3xl: CGFloat = 6.4                // --radius-3xl (widget shell)
    public static let radius4xl: CGFloat = 8                  // --radius-4xl

    public static let composerControlGap: CGFloat = 4         // --composer-control-gap: 0.25rem
    public static let composerSurfacePadX: CGFloat = 8        // --composer-surface-pad-x: 0.5rem
    public static let composerSurfacePadY: CGFloat = 5        // --composer-surface-pad-y: 0.3125rem
    public static let composerPillMaxWidth: CGFloat = 160     // model-pill.tsx:26 'max-w-40' (10rem)
    public static let composerControlRowHeight: CGFloat = 44

    public static let tabStripSeparatorHeight: CGFloat = 16
    public static let tabStripTabMaxWidth: CGFloat = 160
}
