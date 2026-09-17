import HermoLogic
import SwiftUI

/// Port of `TranscriptRow.kt`'s `ThinkingRow`. The settled label ladder and the open rule live in
/// `ThinkingLabel`; this view owns the live label, the character counter, the chevron and the
/// `ContinuousClock` anchor that measures the block's duration from its first live frame.
public struct ThinkingRow: View {
    private let text: String
    private let live: Bool

    @Environment(\.hermoTokens) private var tokens
    @State private var userToggledOpen: Bool?
    @State private var measuredS: Int64?

    public init(text: String, live: Bool) {
        self.text = text
        self.live = live
    }

    private var open: Bool {
        ThinkingLabel.isOpen(live: live, userToggle: userToggledOpen)
    }

    private var label: String {
        live ? "Thinking" : ThinkingLabel.settledLabel(measuredS: measuredS)
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
            if open, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                Text(text)
                    .font(.system(size: 12))
                    .hermoLineHeight(16, fontSize: 12)
                    .foregroundStyle(tokens.textTertiary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.top, 2)
                    .padding(.bottom, 4)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        // A container identifier swallows its children unless the element is a container.
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("hermo.chat.row.thinking")
        .task(id: live) {
            guard live else { return }
            let clock = ContinuousClock()
            let start = clock.now
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(1))
                if Task.isCancelled { return }
                measuredS = Int64(start.duration(to: clock.now).components.seconds)
            }
        }
    }

    private var header: some View {
        HStack(spacing: 6) {
            ScaffoldGlyph(glyph: "")
            Text(label)
                .font(.system(size: HermoMetrics.convToolFontSize))
                .hermoLineHeight(HermoMetrics.convLineHeight, fontSize: HermoMetrics.convToolFontSize)
                .foregroundStyle(tokens.scaffoldText)
            Chevron(open: open, tint: tokens.scaffoldMeta.opacity(0.8))
            if live {
                Text("\(text.utf16.count) chars")
                    .font(.system(size: 10))
                    .foregroundStyle(tokens.scaffoldMeta)
            }
        }
        .padding(.vertical, 1)
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
        .onTapGesture {
            userToggledOpen = !(userToggledOpen ?? live)
        }
        .accessibilityAddTraits(.isButton)
        .accessibilityIdentifier("hermo.chat.row.thinking.toggle")
    }
}

#Preview("Light — live") {
    ThinkingRow(text: "Weighing a few approaches to the retry ladder before committing to one.", live: true)
        .padding()
        .hermoTheme(.light)
}

#Preview("Light — settled") {
    ThinkingRow(text: "Weighing a few approaches to the retry ladder before committing to one.", live: false)
        .padding()
        .hermoTheme(.light)
}

#Preview("Dark — live") {
    ThinkingRow(text: "Weighing a few approaches to the retry ladder before committing to one.", live: true)
        .padding()
        .hermoTheme(.dark)
}

#Preview("Dark — settled") {
    ThinkingRow(text: "Weighing a few approaches to the retry ladder before committing to one.", live: false)
        .padding()
        .hermoTheme(.dark)
}
