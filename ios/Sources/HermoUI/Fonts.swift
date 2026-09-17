import SwiftUI
import UIKit

public enum HermoFonts {
    public static let headlineLarge = Font.system(size: HermoMetrics.headlineLargeFontSize, weight: .semibold)
    public static let titleMedium = Font.system(size: HermoMetrics.titleMediumFontSize, weight: .semibold)
    public static let bodyMedium = Font.system(size: HermoMetrics.convFontSize)
    public static let labelSmall = Font.system(size: HermoMetrics.convToolFontSize, weight: .medium)

    /// The system font in its monospaced design, standing in for Kotlin's `FontFamily.Monospace`.
    public static let mono = Font.system(size: HermoMetrics.convToolFontSize, design: .monospaced)
}

private struct HermoLineHeightModifier: ViewModifier {
    let lineHeight: CGFloat
    let fontSize: CGFloat
    let monospaced: Bool

    func body(content: Content) -> some View {
        // `lineSpacing` ADDS to the font's own leading rather than replacing it, so the gap to
        // ask for is the target pitch minus what the font already occupies, not minus its size.
        let font = monospaced
            ? UIFont.monospacedSystemFont(ofSize: fontSize, weight: .regular)
            : UIFont.systemFont(ofSize: fontSize)
        content.lineSpacing(max(0, lineHeight - font.lineHeight))
    }
}

extension View {
    /// Compose's `lineHeight` is a total line pitch; SwiftUI's `Font` carries no equivalent, so
    /// this asks for the spacing that brings the rendered pitch to `lineHeight`.
    public func hermoLineHeight(_ lineHeight: CGFloat, fontSize: CGFloat, monospaced: Bool = false) -> some View {
        modifier(HermoLineHeightModifier(lineHeight: lineHeight, fontSize: fontSize, monospaced: monospaced))
    }
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
