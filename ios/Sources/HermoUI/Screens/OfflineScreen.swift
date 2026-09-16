import SwiftUI

/// Offline phase, with the destructive "Reset sessions" action gated behind a confirmation (`Screens.kt`'s `OfflineScreen`).
public struct OfflineScreen: View {
    private let reason: String
    private let onRetry: () -> Void
    private let onResetSessions: () -> Void
    private let onLogout: () -> Void

    @State private var resetConfirm = false
    @Environment(\.hermoTokens) private var tokens

    public init(
        reason: String,
        onRetry: @escaping () -> Void,
        onResetSessions: @escaping () -> Void,
        onLogout: @escaping () -> Void
    ) {
        self.reason = reason
        self.onRetry = onRetry
        self.onResetSessions = onResetSessions
        self.onLogout = onLogout
    }

    public var body: some View {
        VStack(spacing: 4) {
            Text("Offline: \(reason)")
                .font(HermoFonts.bodyMedium)
                .foregroundStyle(tokens.text)
            Button("Retry", action: onRetry)
                .buttonStyle(.bordered)
                .tint(tokens.primary)
                .padding(.top, 12)
                .accessibilityIdentifier("hermo.offline.retry")
            Button("Reset sessions") {
                resetConfirm = true
            }
            .buttonStyle(.plain)
            .foregroundStyle(tokens.destructive)
            .padding(.top, 4)
            .accessibilityIdentifier("hermo.offline.resetSessions")
            Button("Log out / pair another gateway", action: onLogout)
                .buttonStyle(.plain)
                .foregroundStyle(tokens.destructive)
                .padding(.top, 4)
                .accessibilityIdentifier("hermo.offline.logOut")
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(tokens.background.ignoresSafeArea())
        .alert("Reset sessions?", isPresented: $resetConfirm) {
            Button("Reset", action: onResetSessions)
                .accessibilityIdentifier("hermo.offline.confirmReset")
            Button("Cancel", role: .cancel) {}
                .accessibilityIdentifier("hermo.offline.cancelReset")
        } message: {
            Text(
                "Clears the local session list on this phone and starts a new "
                    + "chat. The chats on the host are not deleted, and the pairing "
                    + "(host + password) is kept."
            )
        }
    }
}

#Preview("Light") {
    OfflineScreen(
        reason: "error: session registry unreachable",
        onRetry: {},
        onResetSessions: {},
        onLogout: {}
    )
    .hermoTheme(.light)
}

#Preview("Dark") {
    OfflineScreen(
        reason: "error: session registry unreachable",
        onRetry: {},
        onResetSessions: {},
        onLogout: {}
    )
    .hermoTheme(.dark)
}
