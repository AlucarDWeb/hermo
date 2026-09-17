import HermoLogic
import SwiftUI

/// A tool row: title, running/exit suffix and duration in the header, expandable to the diff or
/// the technical trace. Port of `TranscriptRow.kt`'s `ToolCard` (TranscriptRow.kt:369-477).
/// The shell is `WidgetShell` rather than a hand-rolled background: section 3.4 keeps glass off
/// content, and this reuses the primitive that already embodies that rule.
public struct ToolCard: View {
    private let id: Int
    private let name: String
    private let complete: Bool
    private let argsJson: String
    private let resultJson: String
    private let inlineDiff: String
    private let durationS: Double
    private let exitCode: Int?

    @State private var open: Bool
    @Environment(\.hermoTokens) private var tokens

    private static let monoPayloadFontSize: CGFloat = 10.4 // TOOL_PAYLOAD_PRE_CLASS: 0.65rem, TranscriptRow.kt:475
    private static let monoPayloadFont = Font.system(size: monoPayloadFontSize, design: .monospaced)

    public init(
        id: Int,
        name: String,
        complete: Bool,
        argsJson: String,
        resultJson: String,
        inlineDiff: String,
        durationS: Double,
        exitCode: Int?
    ) {
        self.id = id
        self.name = name
        self.complete = complete
        self.argsJson = argsJson
        self.resultJson = resultJson
        self.inlineDiff = inlineDiff
        self.durationS = durationS
        self.exitCode = exitCode
        _open = State(initialValue: !inlineDiff.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
    }

    private var running: Bool { !complete }

    private var hasDiff: Bool {
        !inlineDiff.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    public var body: some View {
        WidgetShell {
            VStack(alignment: .leading, spacing: 0) {
                header
                if open {
                    expandedBody
                        .padding(.top, 6)
                }
            }
        }
        // A container identifier swallows its children unless the element is a container.
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("hermo.chat.row.tool.\(id)")
    }

    private var header: some View {
        Button {
            open.toggle()
        } label: {
            HStack(spacing: 6) {
                ScaffoldGlyph(symbol: toolSymbol(name))
                headerText
                    .font(.system(size: HermoMetrics.convToolFontSize))
                    .hermoLineHeight(HermoMetrics.convLineHeight, fontSize: HermoMetrics.convToolFontSize)
                if !running, !durationText.isEmpty {
                    Text(durationText)
                        .font(.system(size: 10))
                        .foregroundStyle(tokens.scaffoldMeta)
                }
                Chevron(open: open, tint: tokens.scaffoldMeta)
            }
            .padding(.vertical, 1)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("hermo.chat.row.tool.\(id).toggle")
    }

    /// Title plus a status suffix, "  ·  running" or "  ·  exit N" (a zero exit code is success,
    /// TranscriptRow.kt:398's own comment: not worth the destructive styling).
    private var headerText: Text {
        let title = Text(ToolCardModel.toolTitle(name))
            .foregroundColor(running ? tokens.scaffoldMeta : tokens.scaffoldText)
        if running {
            let suffix = Text("  ·  running").foregroundColor(tokens.scaffoldMeta)
            return Text("\(title)\(suffix)")
        }
        if let exitCode, exitCode != 0 {
            let suffix = Text("  ·  exit \(exitCode)").foregroundColor(tokens.destructive)
            return Text("\(title)\(suffix)")
        }
        return title
    }

    private var durationText: String {
        running ? "" : ToolCardModel.formatToolDuration(durationS)
    }

    @ViewBuilder
    private var expandedBody: some View {
        if hasDiff {
            Text(ToolCardModel.stripInlineDiffChrome(inlineDiff))
                .font(Self.monoPayloadFont)
                .foregroundStyle(tokens.textSecondary)
                .hermoLineHeight(HermoMetrics.convLineHeight, fontSize: Self.monoPayloadFontSize, monospaced: true)
                .frame(maxWidth: .infinity, alignment: .leading)
        } else {
            let trace = ToolCardModel.clampForDisplay(
                ToolCardModel.technicalTrace(argsJson: argsJson, resultJson: resultJson)
            )
            if !trace.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    Text(trace)
                        .font(Self.monoPayloadFont)
                        .foregroundStyle(tokens.textSecondary)
                        .hermoLineHeight(HermoMetrics.convLineHeight, fontSize: Self.monoPayloadFontSize, monospaced: true)
                }
            }
        }
    }
}

#Preview("Light — running") {
    ToolCard(
        id: 1,
        name: "execute_code",
        complete: false,
        argsJson: "{\"command\":\"pytest -q\"}",
        resultJson: "",
        inlineDiff: "",
        durationS: 0,
        exitCode: nil
    )
    .padding()
    .hermoTheme(.light)
}

#Preview("Light — complete with diff") {
    ToolCard(
        id: 2,
        name: "patch",
        complete: true,
        argsJson: "{\"path\":\"main.go\"}",
        resultJson: "{\"ok\":true}",
        inlineDiff: "┊ review diff\n--- a/main.go\n+++ b/main.go\n-old line\n+new line",
        durationS: 1.2,
        exitCode: 0
    )
    .padding()
    .hermoTheme(.light)
}

#Preview("Light — complete without diff") {
    ToolCard(
        id: 3,
        name: "read_file",
        complete: true,
        argsJson: "{\"path\":\"README.md\"}",
        resultJson: "{\"content\":\"# hermo\"}",
        inlineDiff: "",
        durationS: 0.42,
        exitCode: 0
    )
    .padding()
    .hermoTheme(.light)
}

#Preview("Dark — running") {
    ToolCard(
        id: 1,
        name: "execute_code",
        complete: false,
        argsJson: "{\"command\":\"pytest -q\"}",
        resultJson: "",
        inlineDiff: "",
        durationS: 0,
        exitCode: nil
    )
    .padding()
    .hermoTheme(.dark)
}

#Preview("Dark — complete with diff") {
    ToolCard(
        id: 2,
        name: "patch",
        complete: true,
        argsJson: "{\"path\":\"main.go\"}",
        resultJson: "{\"ok\":true}",
        inlineDiff: "┊ review diff\n--- a/main.go\n+++ b/main.go\n-old line\n+new line",
        durationS: 1.2,
        exitCode: 0
    )
    .padding()
    .hermoTheme(.dark)
}

#Preview("Dark — complete without diff") {
    ToolCard(
        id: 3,
        name: "read_file",
        complete: true,
        argsJson: "{\"path\":\"README.md\"}",
        resultJson: "{\"content\":\"# hermo\"}",
        inlineDiff: "",
        durationS: 0.42,
        exitCode: 0
    )
    .padding()
    .hermoTheme(.dark)
}
