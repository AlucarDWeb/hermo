import SwiftUI

/// The chat screen's session tab strip, ported from `ChatScreen.kt`'s `SessionTabStrip` and
/// `SessionTabItem`. Liquid Glass trades the Kotlin underline treatment for `GlassPill`
/// capsules, so the tab shape is chrome rather than a literal port of `pane-tab.tsx`. The close
/// control has no hover state on a phone, so unlike the desktop original it stays visible at
/// all times, and every hit target (select, close, add) holds to at least 40 points.
public struct SessionTabStrip: View {
    public struct Tab: Identifiable, Equatable, Sendable {
        public let key: String
        public let title: String
        public let running: Bool

        public init(key: String, title: String, running: Bool) {
            self.key = key
            self.title = title
            self.running = running
        }

        public var id: String { key }
    }

    private let tabs: [Tab]
    private let currentKey: String?
    private let onSelect: (String) -> Void
    private let onClose: (String) -> Void
    private let onAdd: () -> Void
    @Environment(\.hermoTokens) private var tokens

    public init(
        tabs: [Tab],
        currentKey: String?,
        onSelect: @escaping (String) -> Void,
        onClose: @escaping (String) -> Void,
        onAdd: @escaping () -> Void
    ) {
        self.tabs = tabs
        self.currentKey = currentKey
        self.onSelect = onSelect
        self.onClose = onClose
        self.onAdd = onAdd
    }

    public var body: some View {
        HStack(spacing: 8) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 8) {
                    ForEach(tabs) { tab in
                        SessionTabPill(
                            tab: tab,
                            active: tab.key == currentKey,
                            onSelect: { onSelect(tab.key) },
                            onClose: { onClose(tab.key) }
                        )
                    }
                }
            }
            Button(action: onAdd) {
                Image(systemName: "plus")
                    .font(.system(size: HermoMetrics.convFontSize, weight: .medium))
                    .foregroundStyle(tokens.textTertiary)
                    .frame(width: 40, height: 40)
            }
            .buttonStyle(.plain)
            .accessibilityIdentifier("hermo.chat.tab.add")
        }
    }
}

/// One tab: the running dot, the title (or "New session" when blank), and an always-visible
/// close control, all inside a `GlassPill` tinted when the tab is active.
private struct SessionTabPill: View {
    let tab: SessionTabStrip.Tab
    let active: Bool
    let onSelect: () -> Void
    let onClose: () -> Void
    @Environment(\.hermoTokens) private var tokens

    var body: some View {
        GlassPill(active: active) {
            HStack(spacing: 6) {
                if tab.running {
                    Circle()
                        .fill(tokens.successDot)
                        .frame(width: 6, height: 6)
                }
                Text(tab.title.isEmpty ? "New session" : tab.title)
                    .font(HermoFonts.labelSmall)
                    .foregroundStyle(active ? tokens.text : tokens.textTertiary)
                    .lineLimit(1)
                    .truncationMode(.tail)
                Button(action: onClose) {
                    Image(systemName: "xmark")
                        .font(.system(size: 11, weight: .medium))
                        .foregroundStyle(tokens.textTertiary)
                        .frame(width: 40, height: 40)
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("hermo.chat.tab.\(tab.key).close")
            }
            .padding(.leading, 10)
        }
        .frame(maxWidth: HermoMetrics.tabStripTabMaxWidth)
        .contentShape(Rectangle())
        .onTapGesture(perform: onSelect)
        // `children: .contain` keeps the close button addressable; identifying the
        // pill as one element swallows it.
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("hermo.chat.tab.\(tab.key)")
    }
}

#Preview("Light") {
    SessionTabStrip(
        tabs: [
            .init(key: "a", title: "Refactor plan", running: false),
            .init(key: "b", title: "Running a very long task title", running: true),
        ],
        currentKey: "a",
        onSelect: { _ in },
        onClose: { _ in },
        onAdd: {}
    )
    .padding()
    .hermoTheme(.light)
}

#Preview("Dark") {
    SessionTabStrip(
        tabs: [
            .init(key: "a", title: "Refactor plan", running: false),
            .init(key: "b", title: "Running a very long task title", running: true),
        ],
        currentKey: "a",
        onSelect: { _ in },
        onClose: { _ in },
        onAdd: {}
    )
    .padding()
    .hermoTheme(.dark)
}
