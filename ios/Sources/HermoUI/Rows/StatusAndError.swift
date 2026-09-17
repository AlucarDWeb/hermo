import HermoLogic
import SwiftUI

/// A status row (`TranscriptRow.kt`'s `ScaffoldLine`, called with the row's kind and text):
/// the glyph cell, then the kind label followed by the first non-blank line of the row's text.
public struct StatusLine: View {
    private let label: String
    private let detail: String
    @Environment(\.hermoTokens) private var tokens

    public init(label: String, detail: String = "") {
        self.label = label
        self.detail = detail
    }

    public var body: some View {
        HStack(spacing: 6) {
            ScaffoldGlyph(glyph: "")
            Text(detail.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? label : "\(label)  \(detail)")
                .font(.system(size: HermoMetrics.convToolFontSize))
                .hermoLineHeight(HermoMetrics.convLineHeight, fontSize: HermoMetrics.convToolFontSize)
                .foregroundStyle(tokens.scaffoldText)
        }
        .padding(.bottom, 4)
        .accessibilityIdentifier("hermo.chat.row.status")
    }
}

/// The error row (`TranscriptRow.kt`'s inline `Text` for `ChatRow.Error`): the message at the
/// conversation size, in `destructive`, with no background chip.
public struct ErrorRow: View {
    private let message: String
    @Environment(\.hermoTokens) private var tokens

    public init(message: String) {
        self.message = message
    }

    public var body: some View {
        Text(message)
            .font(.system(size: HermoMetrics.convFontSize))
            .hermoLineHeight(HermoMetrics.convLineHeight, fontSize: HermoMetrics.convFontSize)
            .foregroundStyle(tokens.destructive)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.bottom, HermoMetrics.paragraphGap)
            .accessibilityIdentifier("hermo.chat.row.error")
    }
}

/// The first non-blank line of a string, or an empty string when every line is blank
/// (`TranscriptRow.kt`'s `firstLine`).
func firstLine(_ text: String) -> String {
    for line in text.split(separator: "\n", omittingEmptySubsequences: false) {
        if !line.trimmingCharacters(in: .whitespaces).isEmpty {
            return String(line)
        }
    }
    return ""
}

#Preview("Light") {
    VStack(alignment: .leading, spacing: 12) {
        StatusLine(label: "Compacting", detail: "Trimming older turns to fit the context window")
        ErrorRow(message: "Error: the gateway closed the connection")
    }
    .padding()
    .hermoTheme(.light)
}

#Preview("Dark") {
    VStack(alignment: .leading, spacing: 12) {
        StatusLine(label: "Compacting", detail: "Trimming older turns to fit the context window")
        ErrorRow(message: "Error: the gateway closed the connection")
    }
    .padding()
    .hermoTheme(.dark)
}
