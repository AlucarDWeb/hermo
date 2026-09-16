import HermoLogic
import SwiftUI

/// The chat screen's glass titlebar: menu, rename, the title with a chevron to the session picker, and appearance (`ChatScreen.kt`'s `ChatTitlebar`).
public struct ChatTitlebar: View {
    private let title: String
    private let themeMode: ThemeMode
    private let onMenuTap: () -> Void
    private let onEditTitleTap: () -> Void
    private let onTitleTap: () -> Void
    private let onAppearanceTap: () -> Void
    @Environment(\.hermoTokens) private var tokens

    public init(
        title: String,
        themeMode: ThemeMode,
        onMenuTap: @escaping () -> Void,
        onEditTitleTap: @escaping () -> Void,
        onTitleTap: @escaping () -> Void,
        onAppearanceTap: @escaping () -> Void
    ) {
        self.title = title
        self.themeMode = themeMode
        self.onMenuTap = onMenuTap
        self.onEditTitleTap = onEditTitleTap
        self.onTitleTap = onTitleTap
        self.onAppearanceTap = onAppearanceTap
    }

    private var isBlankTitle: Bool {
        title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    public var body: some View {
        VStack(spacing: 0) {
            GlassBar {
                HStack(spacing: 8) {
                    GlassPill {
                        Button(action: onMenuTap) {
                            Image(systemName: "line.3.horizontal")
                                .foregroundStyle(tokens.textSecondary)
                                .frame(width: 40, height: 40)
                        }
                        .accessibilityIdentifier("hermo.chat.menu")
                    }

                    GlassPill {
                        Button(action: onEditTitleTap) {
                            Image(systemName: "pencil")
                                .foregroundStyle(tokens.textSecondary)
                                .frame(width: 40, height: 40)
                        }
                        .accessibilityIdentifier("hermo.chat.rename")
                    }

                    GlassPill {
                        Button(action: onTitleTap) {
                            HStack(spacing: 4) {
                                Text(isBlankTitle ? "New session" : title)
                                    .font(HermoFonts.bodyMedium)
                                    .foregroundStyle(isBlankTitle ? tokens.textTertiary : tokens.textSecondary)
                                    .lineLimit(1)
                                    .truncationMode(.tail)
                                Image(systemName: "chevron.down")
                                    .foregroundStyle(tokens.textTertiary)
                            }
                            .padding(.horizontal, 12)
                            .frame(height: 40)
                        }
                        .accessibilityIdentifier("hermo.chat.titlePicker")
                    }
                    .frame(maxWidth: .infinity)

                    GlassPill {
                        Button(action: onAppearanceTap) {
                            Image(systemName: appearanceSymbol(themeMode))
                                .foregroundStyle(tokens.textSecondary)
                                .frame(width: 40, height: 40)
                        }
                        .accessibilityIdentifier("hermo.chat.appearance")
                    }
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
            }
            Rectangle()
                .fill(tokens.strokeTertiary)
                .frame(height: 1)
        }
    }
}

/// The rename dialog, prefilled with the current title, Save disabled while the field is blank (`ChatScreen.kt`'s `RenameDialog`).
public struct RenameDialog: View {
    private let current: String
    private let onConfirm: (String) -> Void
    private let onDismiss: () -> Void

    @State private var name: String
    @Environment(\.hermoTokens) private var tokens

    public init(current: String, onConfirm: @escaping (String) -> Void, onDismiss: @escaping () -> Void) {
        self.current = current
        self.onConfirm = onConfirm
        self.onDismiss = onDismiss
        _name = State(initialValue: current)
    }

    private var nameIsBlank: Bool {
        name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Rename session")
                .font(HermoFonts.titleMedium)
                .foregroundStyle(tokens.text)

            TextField("Session name", text: $name)
                .textFieldStyle(.roundedBorder)
                .accessibilityIdentifier("hermo.chat.renameField")

            HStack {
                Spacer()
                Button("Cancel", action: onDismiss)
                    .buttonStyle(.plain)
                    .foregroundStyle(tokens.textSecondary)
                    .accessibilityIdentifier("hermo.chat.renameCancel")
                Button("Save") { onConfirm(name) }
                    .buttonStyle(.borderedProminent)
                    .tint(tokens.primary)
                    .disabled(nameIsBlank)
                    .accessibilityIdentifier("hermo.chat.renameSave")
            }
        }
        .padding(20)
        .background(tokens.elevated)
        .clipShape(RoundedRectangle(cornerRadius: HermoMetrics.radius3xl))
    }
}

/// The inline error strip above the transcript, hidden entirely when there is nothing to say (`ChatScreen.kt`'s `ErrorBanner`).
public struct ErrorBanner: View {
    private let text: String
    @Environment(\.hermoTokens) private var tokens

    public init(text: String) {
        self.text = text
    }

    public var body: some View {
        if !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            Text(text)
                .font(.system(size: HermoMetrics.convToolFontSize))
                .foregroundStyle(tokens.destructive)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 16)
                .padding(.vertical, 6)
        }
    }
}

private struct ChatChromeSwatch<Content: View>: View {
    let mode: ThemeMode
    @ViewBuilder let content: Content

    var body: some View {
        ChatChromeSwatchBody(content: content)
            .hermoTheme(mode)
    }
}

private struct ChatChromeSwatchBody<Content: View>: View {
    @Environment(\.hermoTokens) private var tokens
    let content: Content

    var body: some View {
        VStack(spacing: 16) {
            content
        }
        .padding(20)
        .background(tokens.background)
    }
}

#Preview("ChatTitlebar - Light") {
    ChatChromeSwatch(mode: .light) {
        ChatTitlebar(
            title: "",
            themeMode: .light,
            onMenuTap: {},
            onEditTitleTap: {},
            onTitleTap: {},
            onAppearanceTap: {}
        )
        ChatTitlebar(
            title: "Investigate the flaky test",
            themeMode: .light,
            onMenuTap: {},
            onEditTitleTap: {},
            onTitleTap: {},
            onAppearanceTap: {}
        )
    }
}

#Preview("ChatTitlebar - Dark") {
    ChatChromeSwatch(mode: .dark) {
        ChatTitlebar(
            title: "",
            themeMode: .dark,
            onMenuTap: {},
            onEditTitleTap: {},
            onTitleTap: {},
            onAppearanceTap: {}
        )
        ChatTitlebar(
            title: "Investigate the flaky test",
            themeMode: .dark,
            onMenuTap: {},
            onEditTitleTap: {},
            onTitleTap: {},
            onAppearanceTap: {}
        )
    }
}

#Preview("RenameDialog - Light") {
    ChatChromeSwatch(mode: .light) {
        RenameDialog(current: "Investigate the flaky test", onConfirm: { _ in }, onDismiss: {})
        RenameDialog(current: "", onConfirm: { _ in }, onDismiss: {})
    }
}

#Preview("RenameDialog - Dark") {
    ChatChromeSwatch(mode: .dark) {
        RenameDialog(current: "Investigate the flaky test", onConfirm: { _ in }, onDismiss: {})
        RenameDialog(current: "", onConfirm: { _ in }, onDismiss: {})
    }
}

#Preview("ErrorBanner - Light") {
    ChatChromeSwatch(mode: .light) {
        ErrorBanner(text: "error: session registry unreachable")
    }
}

#Preview("ErrorBanner - Dark") {
    ChatChromeSwatch(mode: .dark) {
        ErrorBanner(text: "error: session registry unreachable")
    }
}
