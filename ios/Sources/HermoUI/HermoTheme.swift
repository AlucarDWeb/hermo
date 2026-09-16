import SwiftUI
import HermoLogic

extension EnvironmentValues {
    @Entry public var hermoTokens: HermoTokens = .light
}

private struct HermoThemeModifier: ViewModifier {
    let mode: ThemeMode
    @Environment(\.colorScheme) private var colorScheme

    func body(content: Content) -> some View {
        let dark = resolve(mode, systemDark: colorScheme == .dark)
        // Liquid Glass materials and every system control read `colorScheme`, so the
        // resolved mode has to be pushed there too or an explicit Light/Dark pick paints
        // the tokens one way and the glass chrome the other.
        content
            .environment(\.hermoTokens, dark ? HermoTokens.dark : HermoTokens.light)
            .environment(\.colorScheme, dark ? .dark : .light)
    }
}

extension View {
    /// Resolves `mode` against SwiftUI's `colorScheme` and installs the matching
    /// `HermoTokens` into the environment for this subtree.
    public func hermoTheme(_ mode: ThemeMode) -> some View {
        modifier(HermoThemeModifier(mode: mode))
    }
}

private struct HermoThemePreviewContent: View {
    @Environment(\.hermoTokens) private var tokens

    var body: some View {
        VStack(spacing: 12) {
            Text("Hermo")
                .foregroundStyle(tokens.text)
            HStack(spacing: 8) {
                Circle().fill(tokens.successDot).frame(width: 12, height: 12)
                Text("Connected").foregroundStyle(tokens.textSecondary)
            }
        }
        .padding()
        .background(tokens.background)
    }
}

#Preview("Light") {
    HermoThemePreviewContent().hermoTheme(.light)
}

#Preview("Dark") {
    HermoThemePreviewContent().hermoTheme(.dark)
}

#Preview("System") {
    HermoThemePreviewContent().hermoTheme(.system)
}
