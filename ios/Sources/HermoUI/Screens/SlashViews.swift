import HermoLogic
import SwiftUI

/// The slash-command completions list rendered above the composer (`ChatScreen.kt`'s
/// `SlashCompletionsPopup`), a bounded, scrolling panel whose rows hand the picked item back to
/// the caller. Declared divergence from the Kotlin, which paints a plain `elevated` surface:
/// plan section 3.4 places this popup on the chrome layer, so it renders as Liquid Glass.
public struct SlashCompletionsPopup: View {
    private static let maxHeight: CGFloat = 352

    private let completions: [SlashCompletionRow]
    private let onPick: (SlashCompletionRow) -> Void
    /// A `ScrollView` takes every point offered along its axis, so a two-row popup would claim
    /// the whole 352 and halve the transcript. Measuring the rows keeps 352 a ceiling, which is
    /// what the Kotlin `heightIn(max = 352.dp)` means.
    @State private var contentHeight: CGFloat = 0
    @Environment(\.hermoTokens) private var tokens

    public init(completions: [SlashCompletionRow], onPick: @escaping (SlashCompletionRow) -> Void) {
        self.completions = completions
        self.onPick = onPick
    }

    public var body: some View {
        if !completions.isEmpty {
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(completions.enumerated()), id: \.offset) { index, item in
                        Button {
                            onPick(item)
                        } label: {
                            SlashCompletionRowContent(item: item)
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("hermo.chat.slash.item.\(index)")
                    }
                }
                .padding(.vertical, 4)
                .onGeometryChange(for: CGFloat.self, of: { $0.size.height }) { contentHeight = $0 }
            }
            .frame(height: min(contentHeight, Self.maxHeight))
            .glassEffect(.regular, in: RoundedRectangle(cornerRadius: HermoMetrics.radiusLg))
            .overlay(
                RoundedRectangle(cornerRadius: HermoMetrics.radiusLg)
                    .strokeBorder(tokens.strokeTertiary, lineWidth: 1)
            )
            .padding(.horizontal, 12)
        }
    }
}

private struct SlashCompletionRowContent: View {
    let item: SlashCompletionRow
    @Environment(\.hermoTokens) private var tokens

    var body: some View {
        HStack(spacing: 8) {
            VStack(alignment: .leading, spacing: 0) {
                Text(item.display)
                    .font(.system(size: HermoMetrics.convFontSize))
                    .foregroundStyle(tokens.text)
                    .lineLimit(1)
                if !item.meta.isEmpty {
                    Text(item.meta)
                        .font(.system(size: HermoMetrics.convToolFontSize))
                        .foregroundStyle(tokens.textTertiary)
                        .lineLimit(1)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            if !item.kind.isEmpty {
                Text(item.kind)
                    .font(.system(size: 10))
                    .foregroundStyle(tokens.textTertiary)
                    .lineLimit(1)
            }
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 4)
        .contentShape(Rectangle())
    }
}

/// The ephemeral slash-output line above the composer (`ChatScreen.kt`'s `SlashBanner`).
public struct SlashBanner: View {
    private let text: String
    @Environment(\.hermoTokens) private var tokens

    public init(text: String) {
        self.text = text
    }

    public var body: some View {
        if !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            Text(text)
                .font(.system(size: HermoMetrics.convToolFontSize))
                .foregroundStyle(tokens.scaffoldText)
                .lineLimit(2)
                .truncationMode(.tail)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 16)
                .padding(.vertical, 4)
                .accessibilityIdentifier("hermo.chat.slash.banner")
        }
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
            .padding(.vertical, 20)
            .background(tokens.background)
    }
}

private let previewCompletions = [
    SlashCompletionRow(text: "/help", display: "/help", kind: "Command", meta: "Show available commands"),
    SlashCompletionRow(text: "/clear", display: "/clear", kind: "Command", meta: "Clear the conversation"),
    SlashCompletionRow(
        text: "/summarize-the-entire-conversation-so-far",
        display: "/summarize-the-entire-conversation-so-far",
        kind: "Skill",
        meta: "Summarizes everything discussed in this session so far, including tool calls and their results"
    ),
    SlashCompletionRow(text: "/sessions", display: "/sessions", kind: "", meta: "List open sessions"),
    SlashCompletionRow(text: "/quit", display: "/quit", kind: "Command", meta: ""),
]

private let previewLongCompletions = (0..<20).map { index in
    SlashCompletionRow(
        text: "/command-\(index)",
        display: "/command-\(index)",
        kind: index % 2 == 0 ? "Command" : "Skill",
        meta: "Description for command \(index)"
    )
}

#Preview("Popup — Light") {
    ThemedSwatch(.light) {
        SlashCompletionsPopup(completions: previewCompletions) { _ in }
    }
}

#Preview("Popup — Dark") {
    ThemedSwatch(.dark) {
        SlashCompletionsPopup(completions: previewCompletions) { _ in }
    }
}

#Preview("Popup — Empty") {
    ThemedSwatch(.light) {
        SlashCompletionsPopup(completions: []) { _ in }
    }
}

#Preview("Popup — Long list, scrolls") {
    ThemedSwatch(.dark) {
        SlashCompletionsPopup(completions: previewLongCompletions) { _ in }
    }
}

#Preview("Banner — Light") {
    ThemedSwatch(.light) {
        VStack(spacing: 8) {
            SlashBanner(text: "Ran /help — 12 commands available")
            SlashBanner(text: "")
            SlashBanner(text: "A much longer status line that keeps going until it wraps past two lines and has to truncate somewhere near the end of the second line")
        }
    }
}

#Preview("Banner — Dark") {
    ThemedSwatch(.dark) {
        VStack(spacing: 8) {
            SlashBanner(text: "Ran /help — 12 commands available")
            SlashBanner(text: "")
            SlashBanner(text: "A much longer status line that keeps going until it wraps past two lines and has to truncate somewhere near the end of the second line")
        }
    }
}
