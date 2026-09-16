import SwiftUI

public enum HermoFonts {
    public static let headlineLarge = Font.system(size: HermoMetrics.headlineLargeFontSize, weight: .semibold)
    public static let titleMedium = Font.system(size: HermoMetrics.titleMediumFontSize, weight: .semibold)
    public static let bodyMedium = Font.system(size: HermoMetrics.convFontSize)
    public static let labelSmall = Font.system(size: HermoMetrics.convToolFontSize, weight: .medium)

    /// The system font in its monospaced design, standing in for Kotlin's `FontFamily.Monospace`.
    public static let mono = Font.system(size: HermoMetrics.convToolFontSize, design: .monospaced)
}

#Preview("Type ramp") {
    VStack(alignment: .leading, spacing: 12) {
        Text("Headline large").font(HermoFonts.headlineLarge)
        Text("Title medium").font(HermoFonts.titleMedium)
        Text("Body medium").font(HermoFonts.bodyMedium)
        Text("Label small").font(HermoFonts.labelSmall)
        Text("let tokens = HermoTokens.light").font(HermoFonts.mono)
    }
    .padding()
}
