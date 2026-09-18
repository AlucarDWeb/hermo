import HermoLogic
import SwiftUI

/// The bot drawer: a custom leading side panel over a scrim, since SwiftUI's `.sheet` has no
/// modal-drawer counterpart (`BotDrawer.kt`'s `ModalDrawerSheet`, presented via
/// `ChatScreen.kt`'s `ModalNavigationDrawer`). `isOpen` and `onDismiss` own the presentation
/// here rather than at the call site, so the scrim tap and the panel's own dismissal path
/// (logging out) both land in one place.
public struct BotDrawer: View {
    private static let panelMaxWidth: CGFloat = 320

    private let isOpen: Bool
    private let state: DrawerUiState
    private let onProfileTap: (BotDrawerRow) -> Void
    private let onRetry: () -> Void
    private let onDismiss: () -> Void
    private let onLogout: () -> Void

    @State private var logoutConfirm = false
    @Environment(\.hermoTokens) private var tokens

    public init(
        isOpen: Bool,
        state: DrawerUiState,
        onProfileTap: @escaping (BotDrawerRow) -> Void,
        onRetry: @escaping () -> Void,
        onDismiss: @escaping () -> Void,
        onLogout: @escaping () -> Void
    ) {
        self.isOpen = isOpen
        self.state = state
        self.onProfileTap = onProfileTap
        self.onRetry = onRetry
        self.onDismiss = onDismiss
        self.onLogout = onLogout
    }

    public var body: some View {
        if isOpen {
            ZStack(alignment: .leading) {
                tokens.scrim
                    .ignoresSafeArea()
                    .contentShape(Rectangle())
                    .onTapGesture { onDismiss() }
                    .accessibilityIdentifier("hermo.drawer.scrim")

                BotDrawerPanel(
                    state: state,
                    onProfileTap: onProfileTap,
                    onRetry: onRetry,
                    onLogoutTap: { logoutConfirm = true }
                )
                .frame(maxWidth: Self.panelMaxWidth, maxHeight: .infinity, alignment: .leading)
                .glassEffect(.regular, in: .rect(cornerRadius: 24))
                .accessibilityElement(children: .contain)
                .accessibilityIdentifier("hermo.drawer.panel")

                if logoutConfirm {
                    tokens.scrim
                        .ignoresSafeArea()
                        .contentShape(Rectangle())
                        .onTapGesture { logoutConfirm = false }
                    LogoutConfirmDialog(
                        onConfirm: {
                            logoutConfirm = false
                            onDismiss()
                            onLogout()
                        },
                        onCancel: { logoutConfirm = false }
                    )
                }
            }
        }
    }
}

private struct BotDrawerPanel: View {
    let state: DrawerUiState
    let onProfileTap: (BotDrawerRow) -> Void
    let onRetry: () -> Void
    let onLogoutTap: () -> Void
    @Environment(\.hermoTokens) private var tokens

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                Text("Bots")
                    .font(.system(size: HermoMetrics.convFontSize, weight: .medium))
                    .foregroundStyle(tokens.textSecondary)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 8)

                switch state {
                case .loading:
                    HStack(spacing: 12) {
                        ProgressView()
                            .tint(tokens.midground)
                        Text("Loading profiles…")
                            .font(.system(size: HermoMetrics.convToolFontSize))
                            .foregroundStyle(tokens.textTertiary)
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 16)
                    .accessibilityElement(children: .combine)
                    .accessibilityIdentifier("hermo.drawer.loading")

                case .failed(let message):
                    VStack(alignment: .leading, spacing: 8) {
                        Text(message)
                            .font(.system(size: HermoMetrics.convToolFontSize))
                            .foregroundStyle(tokens.destructive)
                            .lineLimit(3)
                            .truncationMode(.tail)
                            .accessibilityIdentifier("hermo.drawer.failed.message")
                        Button("Retry", action: onRetry)
                            .buttonStyle(.plain)
                            .foregroundStyle(tokens.primary)
                            .accessibilityIdentifier("hermo.drawer.retry")
                    }
                    .padding(.horizontal, 16)

                case .ready(let rows):
                    if rows.isEmpty {
                        Text("No profiles on this gateway")
                            .font(.system(size: HermoMetrics.convToolFontSize))
                            .foregroundStyle(tokens.textTertiary)
                            .padding(.horizontal, 16)
                            .padding(.vertical, 8)
                            .accessibilityIdentifier("hermo.drawer.empty")
                    }
                    ForEach(rows, id: \.name) { row in
                        BotDrawerRowView(row: row, onTap: { onProfileTap(row) })
                    }
                }

                Divider()
                    .overlay(tokens.strokeTertiary)
                    .padding(.vertical, 8)

                Button(action: onLogoutTap) {
                    Text("Log out / pair another gateway")
                        .font(.system(size: HermoMetrics.convToolFontSize))
                        .foregroundStyle(tokens.destructive)
                }
                .buttonStyle(.plain)
                .padding(.horizontal, 8)
                .accessibilityIdentifier("hermo.drawer.logout")
            }
            .padding(.vertical, 8)
        }
    }
}

private struct BotDrawerRowView: View {
    let row: BotDrawerRow
    let onTap: () -> Void
    @Environment(\.hermoTokens) private var tokens

    var body: some View {
        Button(action: onTap) {
            VStack(alignment: .leading, spacing: 2) {
                Text(row.name)
                    .font(.system(size: HermoMetrics.convFontSize, weight: .medium))
                    .foregroundStyle(tokens.text)
                    .lineLimit(1)
                    .truncationMode(.tail)
                if !row.model.isEmpty {
                    Text(row.model)
                        .font(HermoFonts.mono)
                        .foregroundStyle(tokens.textTertiary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
                if !row.description.isEmpty {
                    Text(row.description)
                        .font(.system(size: HermoMetrics.convToolFontSize))
                        .foregroundStyle(tokens.scaffoldMeta)
                        .lineLimit(2)
                        .truncationMode(.tail)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("hermo.drawer.profile.\(row.name)")
    }
}

/// The "Log out?" confirm dialog, copied from `ChatScreen.kt:270-292`.
private struct LogoutConfirmDialog: View {
    let onConfirm: () -> Void
    let onCancel: () -> Void
    @Environment(\.hermoTokens) private var tokens

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Log out?")
                .font(HermoFonts.titleMedium)
                .foregroundStyle(tokens.text)
            Text(
                "Forgets this gateway on the phone: the local session list and the saved " +
                    "login go, and the pairing screen comes back. The chats on the host are " +
                    "not deleted."
            )
            .font(.system(size: HermoMetrics.convToolFontSize))
            .foregroundStyle(tokens.textSecondary)
            HStack {
                Spacer()
                Button("Cancel", action: onCancel)
                    .buttonStyle(.plain)
                    .foregroundStyle(tokens.textSecondary)
                    .accessibilityIdentifier("hermo.drawer.logout.cancel")
                Button("Log out", action: onConfirm)
                    .buttonStyle(.plain)
                    .foregroundStyle(tokens.destructive)
                    .accessibilityIdentifier("hermo.drawer.logout.confirm")
            }
        }
        .padding(20)
        .frame(maxWidth: 320)
        .background(tokens.elevated)
        .clipShape(RoundedRectangle(cornerRadius: HermoMetrics.radius3xl))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("hermo.drawer.logout.dialog")
    }
}

private struct ThemedSwatch<Content: View>: View {
    let mode: ThemeMode
    let content: Content

    init(_ mode: ThemeMode, @ViewBuilder content: () -> Content) {
        self.mode = mode
        self.content = content()
    }

    var body: some View {
        ThemedSwatchBody(content: content)
            .hermoTheme(mode)
    }
}

private struct ThemedSwatchBody<Content: View>: View {
    @Environment(\.hermoTokens) private var tokens
    let content: Content

    var body: some View {
        content
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(tokens.background)
    }
}

private let previewRows = [
    botDrawerRow(name: "Default", model: "claude-sonnet-4.5", description: "General-purpose assistant"),
    botDrawerRow(name: "Reviewer", model: "claude-opus-4.5", description: "Code review with a security lens"),
    botDrawerRow(name: "Scribe", model: "", description: ""),
]

#Preview("Loading — Light") {
    ThemedSwatch(.light) {
        BotDrawer(isOpen: true, state: .loading, onProfileTap: { _ in }, onRetry: {}, onDismiss: {}, onLogout: {})
    }
}

#Preview("Loading — Dark") {
    ThemedSwatch(.dark) {
        BotDrawer(isOpen: true, state: .loading, onProfileTap: { _ in }, onRetry: {}, onDismiss: {}, onLogout: {})
    }
}

#Preview("Failed — Light") {
    ThemedSwatch(.light) {
        BotDrawer(
            isOpen: true,
            state: .failed("Could not reach the gateway. Check the connection and try again."),
            onProfileTap: { _ in },
            onRetry: {},
            onDismiss: {},
            onLogout: {}
        )
    }
}

#Preview("Failed — Dark") {
    ThemedSwatch(.dark) {
        BotDrawer(
            isOpen: true,
            state: .failed("Could not reach the gateway. Check the connection and try again."),
            onProfileTap: { _ in },
            onRetry: {},
            onDismiss: {},
            onLogout: {}
        )
    }
}

#Preview("Ready — Light") {
    ThemedSwatch(.light) {
        BotDrawer(isOpen: true, state: .ready(previewRows), onProfileTap: { _ in }, onRetry: {}, onDismiss: {}, onLogout: {})
    }
}

#Preview("Ready — Dark") {
    ThemedSwatch(.dark) {
        BotDrawer(isOpen: true, state: .ready(previewRows), onProfileTap: { _ in }, onRetry: {}, onDismiss: {}, onLogout: {})
    }
}

#Preview("Ready — Empty") {
    ThemedSwatch(.light) {
        BotDrawer(isOpen: true, state: .ready([]), onProfileTap: { _ in }, onRetry: {}, onDismiss: {}, onLogout: {})
    }
}
