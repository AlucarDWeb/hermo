import SwiftUI
import HermoLogic

/// Transcript list with the empty-state copy and stick-to-bottom scrolling from `ChatScreen.kt`'s
/// `TranscriptList`, dispatching each row to its view the way `TranscriptRow.kt`'s own `when`
/// does.
public struct Transcript: View {
    private let rows: [ChatRow]
    private let running: Bool
    private let onApprovalChoice: (_ requestId: String, _ choice: String) -> Void
    private let onClarifyAnswer: (_ requestId: String, _ answer: String, _ questionId: String?) -> Void

    @Environment(\.hermoTokens) private var tokens
    @State private var nearBottom = true
    @State private var geometry = TranscriptGeometry()
    @State private var pendingApprovalOffscreen = false

    /// The Kotlin rule is "last visible index >= count - 2"; in points that is roughly the last
    /// two turns, so anything within this much of the content's foot still counts as parked
    /// at the bottom.
    private static let stickToBottomSlack: CGFloat = 96

    private static let contentSpace = "hermo.transcript.content"

    /// `running` defaults to false because Android reads it from one `LocalTurnRunning` composition
    /// local shared by every thinking row in the transcript (`ChatScreen.kt:684`), not a per-row
    /// flag; the caller passes the session's own running state once it wires this parameter through.
    public init(
        rows: [ChatRow],
        running: Bool = false,
        onApprovalChoice: @escaping (_ requestId: String, _ choice: String) -> Void = { _, _ in },
        onClarifyAnswer: @escaping (_ requestId: String, _ answer: String, _ questionId: String?) -> Void = { _, _, _ in }
    ) {
        self.rows = rows
        self.running = running
        self.onApprovalChoice = onApprovalChoice
        self.onClarifyAnswer = onClarifyAnswer
    }

    /// The first unresolved approval row, mirroring `TranscriptList`'s
    /// `rows.indexOfFirst { it is ChatRow.Approval && !it.resolved }`.
    private var pendingApprovalRowId: Int? {
        for row in rows {
            if case .approval(let id, _, _, _, _, let resolved) = row, !resolved {
                return id
            }
        }
        return nil
    }

    /// A row LazyVStack has not yet laid out (a `nil` frame) is one SwiftUI has not scrolled
    /// near, so it counts as off screen exactly like a row absent from Compose's
    /// `visibleItemsInfo`; only a frame that has been reported and does not overlap the
    /// scroll view's visible rect also counts as off screen.
    ///
    /// The visible rect and the row's frame live in a reference box rather than in `@State`
    /// so that a scroll, which reports a new rect on every displayed frame, only invalidates
    /// the transcript when the pill's own visibility flips.
    private func refreshPendingApprovalVisibility() {
        let offscreen: Bool
        if pendingApprovalRowId == nil {
            offscreen = false
        } else if let frame = geometry.pendingApprovalFrame {
            offscreen = !geometry.visibleRect.intersects(frame)
        } else {
            offscreen = true
        }
        if pendingApprovalOffscreen != offscreen {
            pendingApprovalOffscreen = offscreen
        }
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
            ZStack(alignment: .bottom) {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: HermoMetrics.turnGap) {
                        ForEach(rows, id: \.id) { row in
                            rowView(row)
                                .id(row.id)
                                .modifier(PendingRowFrameReporter(
                                    isTarget: row.id == pendingApprovalRowId,
                                    coordinateSpaceName: Self.contentSpace
                                ) { frame in
                                    geometry.pendingApprovalFrame = frame
                                    refreshPendingApprovalVisibility()
                                })
                        }
                    }
                    .padding(.horizontal, 16)
                    .padding(.top, HermoMetrics.turnBlockGap)
                    .padding(.bottom, HermoMetrics.turnBlockGap + 16)
                    .coordinateSpace(name: Self.contentSpace)
                }
                .scrollEdgeEffectStyle(.soft, for: .top)
                // SwiftUI has no `LazyListState.layoutInfo`, so "near the bottom" and each row's
                // visibility are both measured off scroll geometry rather than off item indices.
                .onScrollGeometryChange(for: Bool.self) { geometry in
                    geometry.contentSize.height - geometry.visibleRect.maxY <= Self.stickToBottomSlack
                } action: { _, isNear in
                    nearBottom = isNear
                }
                .onScrollGeometryChange(for: CGRect.self) { scrollGeometry in
                    scrollGeometry.visibleRect
                } action: { _, rect in
                    geometry.visibleRect = rect
                    refreshPendingApprovalVisibility()
                }
                .onChange(of: rows.count) { _, newCount in
                    guard newCount > 0, nearBottom else { return }
                    withAnimation {
                        proxy.scrollTo(rows[newCount - 1].id, anchor: .bottom)
                    }
                }
                .onChange(of: pendingApprovalRowId) { _, _ in
                    geometry.pendingApprovalFrame = nil
                    refreshPendingApprovalVisibility()
                }

                if let pendingApprovalRowId, pendingApprovalOffscreen {
                    ApprovalJumpPill {
                        withAnimation {
                            proxy.scrollTo(pendingApprovalRowId, anchor: .center)
                        }
                    }
                    .padding(.bottom, 8)
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
        case .approval(let id, let requestId, let command, let description, let choices, let resolved):
            ApprovalCard(
                id: id,
                requestId: requestId,
                command: command,
                description: description,
                choices: choices,
                resolved: resolved,
                onChoice: onApprovalChoice
            )
        case .clarify(let id, let requestId, let questions, let resolved):
            ClarifyCard(
                id: id,
                requestId: requestId,
                questionsJson: questions,
                resolved: resolved,
                onAnswer: onClarifyAnswer
            )
        }
    }
}

/// Reports the target row's frame in `coordinateSpaceName`, a content-anchored coordinate space
/// so the reported frame stays comparable to `ScrollGeometry.visibleRect` as the scroll position
/// changes. Only the pending approval row carries this modifier; every other row skips the
/// geometry read.
private struct PendingRowFrameReporter: ViewModifier {
    let isTarget: Bool
    let coordinateSpaceName: String
    let onChange: (CGRect) -> Void

    func body(content: Content) -> some View {
        // One unconditional branch: an `if isTarget` here would put the row in a different
        // arm of a `_ConditionalContent` as the pending row changes, throwing away the card's
        // own state (an approval already on its always-confirm step, say).
        content.onGeometryChange(
            for: CGRect?.self,
            of: { isTarget ? $0.frame(in: .named(coordinateSpaceName)) : nil }
        ) { frame in
            guard let frame else { return }
            onChange(frame)
        }
    }
}

/// Scroll geometry the pill reads but the transcript does not re-render for.
private final class TranscriptGeometry {
    var visibleRect: CGRect = .zero
    var pendingApprovalFrame: CGRect?
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
