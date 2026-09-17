import SwiftUI
import HermoLogic

/// Transcript list with the empty-state copy and stick-to-bottom scrolling from `ChatScreen.kt`'s
/// `TranscriptList`, dispatching each row to its view the way `TranscriptRow.kt`'s own `when`
/// does. Approval and clarify rows are T15's cards; until then they render nothing here.
public struct Transcript: View {
    private let rows: [ChatRow]
    private let running: Bool

    @Environment(\.hermoTokens) private var tokens
    @State private var nearBottom = true

    /// The Kotlin rule is "last visible index >= count - 2"; in points that is roughly the last
    /// two turns, so anything within this much of the content's foot still counts as parked
    /// at the bottom.
    private static let stickToBottomSlack: CGFloat = 96

    /// `running` defaults to false because Android reads it from one `LocalTurnRunning` composition
    /// local shared by every thinking row in the transcript (`ChatScreen.kt:684`), not a per-row
    /// flag; the caller passes the session's own running state once it wires this parameter through.
    public init(rows: [ChatRow], running: Bool = false) {
        self.rows = rows
        self.running = running
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
            UserBubble(text: text)
        case .assistant(_, let text, _, _, _):
            AssistantMarkdown(text: text)
        case .thinking(_, let text):
            ThinkingRow(text: text, live: running)
        case .tool(let id, let name, let complete, _, let argsJson, let resultJson, let inlineDiff, let durationS, let exitCode):
            ToolCard(
                id: id,
                name: name,
                complete: complete,
                argsJson: argsJson,
                resultJson: resultJson,
                inlineDiff: inlineDiff,
                durationS: durationS,
                exitCode: exitCode
            )
        case .status(_, let kind, let text):
            if !kind.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                || !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                StatusLine(label: kind.firstCharacterUppercased, detail: firstLine(text))
            }
        case .error(_, let message):
            ErrorRow(message: message)
        case .approval, .clarify:
            // T15 renders these as cards; the transcript contributes nothing for them yet.
            EmptyView()
        }
    }
}

private extension String {
    /// `Kotlin`'s `replaceFirstChar { it.uppercase() }`: only the first character changes case.
    var firstCharacterUppercased: String {
        guard let first else { return self }
        return first.uppercased() + dropFirst()
    }
}

private let previewRows: [ChatRow] = [
    .user(id: 0, text: "Can you check the build and fix the failing test?"),
    .thinking(id: 1, text: "Weighing a couple of approaches before picking one."),
    .tool(
        id: 2,
        name: "execute_code",
        complete: true,
        context: "",
        argsJson: "{\"command\":\"pytest -q\"}",
        resultJson: "{\"exit_code\":0}",
        inlineDiff: "",
        durationS: 1.4,
        exitCode: 0
    ),
    .assistant(id: 3, text: "Fixed it, the suite is green now.", streaming: false, warning: "", usageJson: ""),
    .status(id: 4, kind: "compacting", text: "Trimming older turns to fit the context window"),
    .error(id: 5, message: "Error: the gateway closed the connection"),
]

#Preview("Light") {
    Transcript(rows: previewRows, running: true)
        .hermoTheme(.light)
}

#Preview("Dark") {
    Transcript(rows: previewRows, running: true)
        .hermoTheme(.dark)
}

#Preview("Empty") {
    Transcript(rows: [])
        .hermoTheme(.light)
}
