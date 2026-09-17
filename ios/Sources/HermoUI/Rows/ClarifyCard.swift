import HermoLogic
import SwiftUI

/// The clarify card: one open question with its chips and free-text field, port of
/// `PromptCards.kt`'s `ClarifyCard` and `ClarifyQuestionBlock` (PromptCards.kt:132-262). The
/// row's `resolved` flag covers the whole batch rather than one question at a time, and the
/// core's `respond_clarify` resolves the row after the first answer with no `remaining` state,
/// so only the first parsed question is ever shown while the row is pending; once resolved the
/// card instead lists every question, none of them interactive.
public struct ClarifyCard: View {
    private let id: Int
    private let requestId: String
    private let questionsJson: String
    private let resolved: Bool
    private let onAnswer: (_ requestId: String, _ answer: String, _ questionId: String?) -> Void

    @Environment(\.hermoTokens) private var tokens

    public init(
        id: Int,
        requestId: String,
        questionsJson: String,
        resolved: Bool,
        onAnswer: @escaping (_ requestId: String, _ answer: String, _ questionId: String?) -> Void
    ) {
        self.id = id
        self.requestId = requestId
        self.questionsJson = questionsJson
        self.resolved = resolved
        self.onAnswer = onAnswer
    }

    private var questions: [ClarifyQuestionUi] {
        parseClarifyQuestions(questionsJson)
    }

    public var body: some View {
        let qs = questions
        Group {
            if qs.isEmpty {
                emptyState
            } else {
                let visible = resolved ? qs : Array(qs.prefix(1))
                let answeredCount = resolved ? qs.count : 0
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(visible.enumerated()), id: \.offset) { index, question in
                        ClarifyQuestionBlock(
                            identifierPrefix: "hermo.chat.row.clarify.\(id)",
                            question: question,
                            resolved: resolved,
                            showProgress: qs.count > 1 && !resolved,
                            answered: answeredCount + index,
                            total: qs.count,
                            onSubmit: { answer in
                                let qid = question.qid.trimmingCharacters(in: .whitespacesAndNewlines)
                                onAnswer(requestId, answer, qid.isEmpty ? nil : qid)
                            }
                        )
                        .id(question.qid.isEmpty ? "index-\(index)" : question.qid)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.bottom, HermoMetrics.paragraphGap)
            }
        }
        // A container identifier swallows its children unless the element is a container.
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("hermo.chat.row.clarify.\(id)")
    }

    /// Garbage or empty `questions[]` degrades to one status line, matching
    /// `PromptCards.kt`'s early return before its padded column ever opens.
    private var emptyState: some View {
        HStack(spacing: 6) {
            ScaffoldGlyph(glyph: "")
            Text("Question")
                .font(.system(size: HermoMetrics.convToolFontSize))
                .hermoLineHeight(HermoMetrics.convLineHeight, fontSize: HermoMetrics.convToolFontSize)
                .foregroundStyle(tokens.scaffoldText)
            if !resolved {
                Text("pending")
                    .font(.system(size: 10))
                    .foregroundStyle(tokens.scaffoldMeta)
            }
        }
        .padding(.bottom, 4)
    }
}

private struct ClarifyQuestionBlock: View {
    let identifierPrefix: String
    let question: ClarifyQuestionUi
    let resolved: Bool
    let showProgress: Bool
    let answered: Int
    let total: Int
    let onSubmit: (String) -> Void

    @State private var selected: [String] = []
    @State private var draft: String = ""
    @Environment(\.hermoTokens) private var tokens

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            WidgetShell {
                VStack(alignment: .leading, spacing: 0) {
                    if showProgress {
                        Text(clarifyProgressLabel(answered, total))
                            .font(.system(size: HermoMetrics.convToolFontSize))
                            .foregroundStyle(tokens.textTertiary)
                            .padding(.bottom, 4)
                    }
                    Text(question.question)
                        .font(.system(size: HermoMetrics.convFontSize))
                        .hermoLineHeight(HermoMetrics.convLineHeight, fontSize: HermoMetrics.convFontSize)
                        .foregroundStyle(tokens.text)
                }
            }
            if !resolved {
                if !question.choices.isEmpty {
                    FlowChips(spacing: 8) {
                        ForEach(Array(question.choices.enumerated()), id: \.offset) { index, choice in
                            ChoiceButton(
                                label: choice,
                                primary: selected.contains(choice),
                                identifier: "\(identifierPrefix).choice.\(index)"
                            ) {
                                pick(choice)
                            }
                        }
                    }
                    .padding(.top, 8)
                }
                Text("Other (type your answer)")
                    .font(.system(size: HermoMetrics.convToolFontSize))
                    .foregroundStyle(tokens.textTertiary)
                    .padding(.top, 8)
                    .padding(.bottom, 4)
                TextField("", text: $draft)
                    .font(.system(size: HermoMetrics.convFontSize))
                    .foregroundStyle(tokens.text)
                    .tint(tokens.primary)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 8)
                    .background(tokens.surface)
                    .clipShape(RoundedRectangle(cornerRadius: HermoMetrics.radiusMd))
                    .overlay(
                        RoundedRectangle(cornerRadius: HermoMetrics.radiusMd)
                            .strokeBorder(tokens.strokeTertiary, lineWidth: 1)
                    )
                    // Typing here clears the chip pick; PromptCards.kt's BasicTextField
                    // never restores it if the field goes blank again.
                    .onChange(of: draft) { _, newValue in
                        if !newValue.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                            selected = []
                        }
                    }
                    .accessibilityIdentifier("\(identifierPrefix).other")
                ChoiceButton(label: "Continue", primary: true, identifier: "\(identifierPrefix).continue") {
                    let answer = encodeClarifyAnswer(question, picks: selected, draft: draft)
                    if !answer.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                        onSubmit(answer)
                    }
                }
                .padding(.top, 8)
            }
        }
        .padding(.bottom, (answered < total - 1 && !resolved) ? 12 : 0)
    }

    private func pick(_ choice: String) {
        if question.multiSelect {
            if let index = selected.firstIndex(of: choice) {
                selected.remove(at: index)
            } else {
                selected.append(choice)
            }
        } else {
            selected = [choice]
        }
        draft = ""
    }
}

/// A wrapping row with fixed spacing on both axes, standing in for Compose's `FlowRow`
/// (`PromptCards.kt`'s clarify choices use it to avoid clipping on a narrow screen).
private struct FlowChips: Layout {
    let spacing: CGFloat

    init(spacing: CGFloat = 8) {
        self.spacing = spacing
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let maxWidth = proposal.width ?? .infinity
        var rowWidth: CGFloat = 0
        var rowHeight: CGFloat = 0
        var totalHeight: CGFloat = 0
        var widestRow: CGFloat = 0

        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if rowWidth > 0, rowWidth + spacing + size.width > maxWidth {
                totalHeight += rowHeight + spacing
                widestRow = max(widestRow, rowWidth)
                rowWidth = size.width
                rowHeight = size.height
            } else {
                rowWidth += (rowWidth > 0 ? spacing : 0) + size.width
                rowHeight = max(rowHeight, size.height)
            }
        }
        totalHeight += rowHeight
        widestRow = max(widestRow, rowWidth)
        return CGSize(width: maxWidth.isFinite ? maxWidth : widestRow, height: totalHeight)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x = bounds.minX
        var y = bounds.minY
        var rowHeight: CGFloat = 0

        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x > bounds.minX, x + size.width > bounds.maxX {
                x = bounds.minX
                y += rowHeight + spacing
                rowHeight = 0
            }
            subview.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
            x += size.width + spacing
            rowHeight = max(rowHeight, size.height)
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
            .padding(16)
            .background(tokens.background)
    }
}

#Preview("Light — single choice") {
    ThemedSwatch(.light) {
        ClarifyCard(
            id: 1,
            requestId: "req-1",
            questionsJson: "[{\"qid\":\"q1\",\"question\":\"Which environment should I target?\",\"choices\":[\"Staging\",\"Production\"],\"multi_select\":false}]",
            resolved: false,
            onAnswer: { _, _, _ in }
        )
    }
    .frame(width: 320)
}

#Preview("Light — multi-select with progress") {
    ThemedSwatch(.light) {
        ClarifyCard(
            id: 2,
            requestId: "req-2",
            questionsJson: "[{\"qid\":\"q1\",\"question\":\"Which checks should block the merge?\",\"choices\":[\"Lint\",\"Unit tests\",\"E2E\"],\"multi_select\":true},{\"qid\":\"q2\",\"question\":\"Who should review it?\",\"choices\":[\"Alice\",\"Bob\"],\"multi_select\":false}]",
            resolved: false,
            onAnswer: { _, _, _ in }
        )
    }
    .frame(width: 320)
}

#Preview("Light — resolved") {
    ThemedSwatch(.light) {
        ClarifyCard(
            id: 3,
            requestId: "req-3",
            questionsJson: "[{\"qid\":\"q1\",\"question\":\"Which environment should I target?\",\"choices\":[\"Staging\",\"Production\"],\"multi_select\":false}]",
            resolved: true,
            onAnswer: { _, _, _ in }
        )
    }
    .frame(width: 320)
}

#Preview("Dark — single choice") {
    ThemedSwatch(.dark) {
        ClarifyCard(
            id: 1,
            requestId: "req-1",
            questionsJson: "[{\"qid\":\"q1\",\"question\":\"Which environment should I target?\",\"choices\":[\"Staging\",\"Production\"],\"multi_select\":false}]",
            resolved: false,
            onAnswer: { _, _, _ in }
        )
    }
    .frame(width: 320)
}

#Preview("Dark — multi-select with progress") {
    ThemedSwatch(.dark) {
        ClarifyCard(
            id: 2,
            requestId: "req-2",
            questionsJson: "[{\"qid\":\"q1\",\"question\":\"Which checks should block the merge?\",\"choices\":[\"Lint\",\"Unit tests\",\"E2E\"],\"multi_select\":true},{\"qid\":\"q2\",\"question\":\"Who should review it?\",\"choices\":[\"Alice\",\"Bob\"],\"multi_select\":false}]",
            resolved: false,
            onAnswer: { _, _, _ in }
        )
    }
    .frame(width: 320)
}

#Preview("Dark — resolved") {
    ThemedSwatch(.dark) {
        ClarifyCard(
            id: 3,
            requestId: "req-3",
            questionsJson: "[{\"qid\":\"q1\",\"question\":\"Which environment should I target?\",\"choices\":[\"Staging\",\"Production\"],\"multi_select\":false}]",
            resolved: true,
            onAnswer: { _, _, _ in }
        )
    }
    .frame(width: 320)
}
