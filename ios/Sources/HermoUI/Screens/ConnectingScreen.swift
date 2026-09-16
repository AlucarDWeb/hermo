import SwiftUI

/// Connecting phase: one line and a spinner, no fake intermediate states (`Screens.kt`'s `ConnectingScreen`).
public struct ConnectingScreen: View {
    @Environment(\.hermoTokens) private var tokens

    public init() {}

    public var body: some View {
        VStack(spacing: 16) {
            Text("Connecting…")
                .font(HermoFonts.bodyMedium)
                .foregroundStyle(tokens.text)
            ProgressView()
                .progressViewStyle(.linear)
                .tint(tokens.primary)
                .accessibilityIdentifier("hermo.connecting.progress")
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(tokens.background.ignoresSafeArea())
    }
}

#Preview("Light") {
    ConnectingScreen().hermoTheme(.light)
}

#Preview("Dark") {
    ConnectingScreen().hermoTheme(.dark)
}
