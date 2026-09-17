import HermoLogic
import SwiftUI

/// An approval prompt: the description or command in a `WidgetShell`, then either every
/// choice the server sent as a button or, after tapping "always", a second confirm step.
/// Port of `PromptCards.kt`'s `ApprovalCard` (PromptCards.kt:44-130). The buttons and shell
/// are `ChoiceButton` and `WidgetShell` from `GlassPrimitives.swift`; the actions render below
/// the shell rather than inside it, matching the Kotlin layout.
public struct ApprovalCard: View {
    private let id: Int
    private let requestId: String
    private let command: String
    private let description: String
    private let choices: [String]
    private let resolved: Bool
    private let onChoice: (_ requestId: String, _ choice: String) -> Void

    @State private var confirmingAlways: Bool
    @Environment(\.hermoTokens) private var tokens

    public init(
        id: Int,
        requestId: String,
        command: String,
        description: String,
        choices: [String],
        resolved: Bool,
        onChoice: @escaping (_ requestId: String, _ choice: String) -> Void
    ) {
        self.init(
            id: id,
            requestId: requestId,
            command: command,
            description: description,
            choices: choices,
            resolved: resolved,
            confirmingAlways: false,
            onChoice: onChoice
        )
    }

    /// Lets the previews below land directly on the always-confirm step without a tap.
    init(
        id: Int,
        requestId: String,
        command: String,
        description: String,
        choices: [String],
        resolved: Bool,
        confirmingAlways: Bool,
        onChoice: @escaping (_ requestId: String, _ choice: String) -> Void
    ) {
        self.id = id
        self.requestId = requestId
        self.command = command
        self.description = description
        self.choices = choices
        self.resolved = resolved
        self.onChoice = onChoice
        _confirmingAlways = State(initialValue: confirmingAlways)
    }

    private var bodyText: String {
        description.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? command : description
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            WidgetShell {
                if !bodyText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    Text(bodyText)
                        .font(.system(size: HermoMetrics.convFontSize))
                        .hermoLineHeight(HermoMetrics.convLineHeight, fontSize: HermoMetrics.convFontSize)
                        .foregroundStyle(tokens.text)
                }
                if !command.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
                   !description.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    Text(command)
                        .font(HermoFonts.mono)
                        .foregroundStyle(tokens.textTertiary)
                        .padding(.top, 6)
                }
            }
            if !resolved {
                if confirmingAlways {
                    alwaysConfirmSection
                } else {
                    choicesSection
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.bottom, HermoMetrics.paragraphGap)
        // A container identifier swallows its children unless the element is a container.
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("hermo.chat.row.approval.\(id)")
    }

    private var alwaysConfirmSection: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(ApprovalCopy.alwaysTitle)
                .font(.system(size: HermoMetrics.convFontSize, weight: .medium))
                .foregroundStyle(tokens.text)
                .padding(.top, 8)
            Text(ApprovalCopy.alwaysBody)
                .font(.system(size: HermoMetrics.convToolFontSize))
                .foregroundStyle(tokens.textSecondary)
                .padding(.top, 4)
                .padding(.bottom, 8)
            HStack(spacing: 8) {
                ChoiceButton(
                    label: ApprovalCopy.alwaysConfirm,
                    primary: true,
                    identifier: "hermo.chat.row.approval.\(id).always.confirm"
                ) {
                    onChoice(requestId, "always")
                    confirmingAlways = false
                }
                ChoiceButton(
                    label: ApprovalCopy.alwaysCancel,
                    primary: false,
                    identifier: "hermo.chat.row.approval.\(id).always.cancel"
                ) {
                    confirmingAlways = false
                }
            }
        }
    }

    private var choicesSection: some View {
        let primary = primaryApprovalChoice(choices)
        return FlowLayout(spacing: 8) {
            ForEach(Array(choices.enumerated()), id: \.offset) { _, wire in
                let known = ApprovalChoice.fromWire(wire)
                let label = known?.label ?? wire
                let isPrimary = primary != nil && wire == primary?.wire
                ChoiceButton(
                    label: label,
                    primary: isPrimary,
                    identifier: "hermo.chat.row.approval.\(id).choice.\(wire)"
                ) {
                    if wire == "always" {
                        confirmingAlways = true
                    } else {
                        onChoice(requestId, wire)
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.top, 8)
    }
}

/// Wraps its children onto new rows once they overflow the available width, standing in for
/// Compose's `FlowRow`, which `PromptCards.kt`'s `ApprovalCard` relies on so that four
/// server-sent choices do not clip on a narrow phone screen.
private struct FlowLayout: Layout {
    let spacing: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        let rows = rows(fitting: width, subviews: subviews)
        let height = rows.reduce(CGFloat(0)) { $0 + $1.height } + CGFloat(max(0, rows.count - 1)) * spacing
        let usedWidth = rows.map(\.width).max() ?? 0
        return CGSize(width: min(usedWidth, width), height: height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let rows = rows(fitting: bounds.width, subviews: subviews)
        var y = bounds.minY
        for row in rows {
            var x = bounds.minX
            for item in row.items {
                item.subview.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(item.size))
                x += item.size.width + spacing
            }
            y += row.height + spacing
        }
    }

    private struct Item {
        let subview: LayoutSubview
        let size: CGSize
    }

    private struct Row {
        var items: [Item] = []
        var width: CGFloat = 0
        var height: CGFloat = 0
    }

    private func rows(fitting width: CGFloat, subviews: Subviews) -> [Row] {
        var rows: [Row] = []
        var current = Row()
        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            let projectedWidth = current.width + (current.items.isEmpty ? 0 : spacing) + size.width
            if !current.items.isEmpty, projectedWidth > width {
                rows.append(current)
                current = Row()
            }
            current.width += (current.items.isEmpty ? 0 : spacing) + size.width
            current.height = max(current.height, size.height)
            current.items.append(Item(subview: subview, size: size))
        }
        if !current.items.isEmpty {
            rows.append(current)
        }
        return rows
    }
}

private struct ThemedApprovalPreview<Content: View>: View {
    let mode: ThemeMode
    let content: Content

    init(_ mode: ThemeMode, @ViewBuilder content: () -> Content) {
        self.mode = mode
        self.content = content()
    }

    var body: some View {
        content
            .padding(16)
            .background(Color(.secondarySystemBackground))
            .hermoTheme(mode)
    }
}

#Preview("Light — pending") {
    ThemedApprovalPreview(.light) {
        ApprovalCard(
            id: 1,
            requestId: "req-1",
            command: "rm -rf build/",
            description: "Delete the stale build output before recompiling?",
            choices: ["once", "session", "always", "deny"],
            resolved: false
        ) { _, _ in }
    }
}

#Preview("Dark — pending") {
    ThemedApprovalPreview(.dark) {
        ApprovalCard(
            id: 1,
            requestId: "req-1",
            command: "rm -rf build/",
            description: "Delete the stale build output before recompiling?",
            choices: ["once", "session", "always", "deny"],
            resolved: false
        ) { _, _ in }
    }
}

#Preview("Light — always confirm") {
    ThemedApprovalPreview(.light) {
        ApprovalCard(
            id: 2,
            requestId: "req-2",
            command: "git push --force",
            description: "Force-push over the remote branch?",
            choices: ["once", "always", "deny"],
            resolved: false,
            confirmingAlways: true
        ) { _, _ in }
    }
}

#Preview("Dark — always confirm") {
    ThemedApprovalPreview(.dark) {
        ApprovalCard(
            id: 2,
            requestId: "req-2",
            command: "git push --force",
            description: "Force-push over the remote branch?",
            choices: ["once", "always", "deny"],
            resolved: false,
            confirmingAlways: true
        ) { _, _ in }
    }
}

#Preview("Light — resolved") {
    ThemedApprovalPreview(.light) {
        ApprovalCard(
            id: 3,
            requestId: "req-3",
            command: "npm install",
            description: "Install the missing dependency?",
            choices: ["once", "always", "deny"],
            resolved: true
        ) { _, _ in }
    }
}

#Preview("Dark — resolved") {
    ThemedApprovalPreview(.dark) {
        ApprovalCard(
            id: 3,
            requestId: "req-3",
            command: "npm install",
            description: "Install the missing dependency?",
            choices: ["once", "always", "deny"],
            resolved: true
        ) { _, _ in }
    }
}
