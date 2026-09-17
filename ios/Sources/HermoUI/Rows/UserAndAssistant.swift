import HermoLogic
import SwiftUI

/// Desktop's user message (`user-message.tsx`'s bordered bubble), reused verbatim on phone:
/// no sticky behaviour, no edit-on-click, no reactions, since those are window affordances
/// (TranscriptRow.kt's `UserBubble`). The bubble fills the row's full width rather than
/// hugging its content, matching the Kotlin `fillMaxWidth` composable.
public struct UserBubble: View {
    private let text: String
    @Environment(\.hermoTokens) private var tokens

    public init(text: String) {
        self.text = text
    }

    public var body: some View {
        Text(text)
            .font(HermoFonts.bodyMedium)
            .hermoLineHeight(HermoMetrics.convLineHeight, fontSize: HermoMetrics.convFontSize)
            .foregroundStyle(tokens.text)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(tokens.userBubble)
            .clipShape(RoundedRectangle(cornerRadius: HermoMetrics.radiusXl))
            .overlay(
                RoundedRectangle(cornerRadius: HermoMetrics.radiusXl)
                    .stroke(tokens.userBubbleBorder, lineWidth: 1)
            )
            .padding(.bottom, HermoMetrics.paragraphGap)
            .accessibilityIdentifier("hermo.chat.row.user")
    }
}

/// Assistant markdown (TranscriptRow.kt's `AssistantMarkdown`): blocks from the core's
/// `MarkdownBlocks.split`, plain text at the conversation style, fenced code as a
/// monospace `CodeBlock`. Inline emphasis and headers stay unimplemented here, same as
/// Android: the core's splitter defines the block grammar and no markdown library is used.
public struct AssistantMarkdown: View {
    private let text: String
    @Environment(\.hermoTokens) private var tokens

    public init(text: String) {
        self.text = text
    }

    public var body: some View {
        let blocks = MarkdownBlocks.split(text)
        VStack(alignment: .leading, spacing: 0) {
            ForEach(Array(blocks.enumerated()), id: \.offset) { index, block in
                if block.isFence || !block.language.isEmpty {
                    CodeBlock(block: block)
                } else {
                    Text(block.text)
                        .font(HermoFonts.bodyMedium)
                        .hermoLineHeight(HermoMetrics.convLineHeight, fontSize: HermoMetrics.convFontSize)
                        .foregroundStyle(tokens.text)
                        .padding(.top, index == 0 ? 0 : HermoMetrics.paragraphGap)
                }
            }
        }
        .padding(.bottom, HermoMetrics.paragraphGap)
        .accessibilityIdentifier("hermo.chat.row.assistant")
    }
}

/// A fenced code block: a language label when the fence carried one, horizontal scrolling
/// for long lines, and no syntax highlighting (TranscriptRow.kt's `CodeBlock`, a deliberate
/// Android decision rather than a gap in this port).
private struct CodeBlock: View {
    let block: MarkdownBlock
    @Environment(\.hermoTokens) private var tokens

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if !block.language.isEmpty {
                Text(block.language)
                    // 10 pt is TranscriptRow.kt's language-label literal, not a HermoMetrics token.
                    .font(.system(size: 10, design: .monospaced))
                    .foregroundStyle(tokens.textTertiary)
            }
            ScrollView(.horizontal, showsIndicators: false) {
                Text(block.text)
                    .font(HermoFonts.mono)
                    .hermoLineHeight(HermoMetrics.convLineHeight, fontSize: HermoMetrics.convToolFontSize, monospaced: true)
                    .foregroundStyle(tokens.text)
            }
            .padding(.top, block.language.isEmpty ? 0 : 4)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(8)
        .background(tokens.widgetSurface)
        .clipShape(RoundedRectangle(cornerRadius: HermoMetrics.radiusMd))
        .padding(.top, HermoMetrics.paragraphGap)
    }
}

#Preview("UserBubble and AssistantMarkdown - Light") {
    ScrollView {
        VStack(alignment: .leading, spacing: 0) {
            UserBubble(text: "Can you check the build and fix the failing test?")
            AssistantMarkdown(text: """
            Sure, here is the fix:

            ```swift
            func add(_ a: Int, _ b: Int) -> Int { a + b }
            ```

            That should do it.
            """)
        }
        .padding(16)
    }
    .hermoTheme(.light)
}

#Preview("UserBubble and AssistantMarkdown - Dark") {
    ScrollView {
        VStack(alignment: .leading, spacing: 0) {
            UserBubble(text: "Can you check the build and fix the failing test?")
            AssistantMarkdown(text: """
            Sure, here is the fix:

            ```swift
            func add(_ a: Int, _ b: Int) -> Int { a + b }
            ```

            That should do it.
            """)
        }
        .padding(16)
    }
    .hermoTheme(.dark)
}
