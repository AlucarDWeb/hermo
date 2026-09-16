import SwiftUI
import HermoLogic

/// Transcript list with the empty-state copy and stick-to-bottom scrolling from `ChatScreen.kt`'s
/// `TranscriptList`. T13 renders user and assistant rows as plain text only; T14 ports the real
/// row views (bubbles, tool cards, thinking, approval, clarify, status, error) for every other kind.
public struct Transcript: View {
    private let rows: [ChatRow]

    @Environment(\.hermoTokens) private var tokens
    @State private var nearBottom = true

    /// The Kotlin rule is "last visible index >= count - 2"; in points that is roughly the last
    /// two turns, so anything within this much of the content's foot still counts as parked
    /// at the bottom.
    private static let stickToBottomSlack: CGFloat = 96

    public init(rows: [ChatRow]) {
        self.rows = rows
    }

    public var body: some View {
        Group {
            if rows.isEmpty {
                emptyState
            } else {
                transcriptList
            }
        }
        .accessibilityIdentifier("hermo.chat.transcript")
    }

    private var emptyState: some View {
        Text("Ask anything — the transcript starts here")
            .font(.system(size: HermoMetrics.convFontSize))
            .foregroundStyle(tokens.textTertiary)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
            .accessibilityIdentifier("hermo.chat.transcript.emptyState")
    }

    private var transcriptList: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: HermoMetrics.turnGap) {
                    ForEach(rows, id: \.id) { row in
                        rowView(row)
                            .id(row.id)
                    }
                }
                .padding(.horizontal, 16)
                .padding(.top, HermoMetrics.turnBlockGap)
                .padding(.bottom, HermoMetrics.turnBlockGap + 16)
            }
            .scrollEdgeEffectStyle(.soft, for: .top)
            // SwiftUI has no `LazyListState.layoutInfo`, so "near the bottom" is measured off the
            // scroll geometry rather than off the last visible index.
            .onScrollGeometryChange(for: Bool.self) { geometry in
                geometry.contentSize.height - geometry.visibleRect.maxY <= Self.stickToBottomSlack
            } action: { _, isNear in
                nearBottom = isNear
            }
            .onChange(of: rows.count) { _, newCount in
                guard newCount > 0, nearBottom else { return }
                withAnimation {
                    proxy.scrollTo(rows[newCount - 1].id, anchor: .bottom)
                }
            }
        }
    }

    @ViewBuilder
    private func rowView(_ row: ChatRow) -> some View {
        switch row {
        case .user(_, let text):
            Text(text)
                .font(.system(size: HermoMetrics.convFontSize))
                .foregroundStyle(tokens.text)
                .frame(maxWidth: .infinity, alignment: .trailing)
                .accessibilityIdentifier("hermo.chat.transcript.userRow")
        case .assistant(_, let text, _, _, _):
            Text(text)
                .font(.system(size: HermoMetrics.convFontSize))
                .foregroundStyle(tokens.text)
                .frame(maxWidth: .infinity, alignment: .leading)
                .accessibilityIdentifier("hermo.chat.transcript.assistantRow")
        default:
            EmptyView()
        }
    }
}

#Preview("Light") {
    Transcript(rows: [
        .user(id: 0, text: "Hi, can you check the build?"),
        .assistant(id: 1, text: "Sure, give me a moment.", streaming: false, warning: "", usageJson: ""),
    ])
    .hermoTheme(.light)
}

#Preview("Dark") {
    Transcript(rows: [
        .user(id: 0, text: "Hi, can you check the build?"),
        .assistant(id: 1, text: "Sure, give me a moment.", streaming: false, warning: "", usageJson: ""),
    ])
    .hermoTheme(.dark)
}

#Preview("Empty") {
    Transcript(rows: [])
        .hermoTheme(.light)
}
