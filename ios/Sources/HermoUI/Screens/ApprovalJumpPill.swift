import HermoLogic
import SwiftUI

/// The floating "Approval needed" pill from `ChatScreen.kt`'s `TranscriptList`
/// (ChatScreen.kt:717-734): `Transcript` shows it only while the first unresolved approval row
/// is off screen, and a tap scrolls the transcript to that row.
public struct ApprovalJumpPill: View {
    private let action: () -> Void

    public init(action: @escaping () -> Void) {
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            Text(ApprovalCopy.jumpToApproval)
                .font(.system(size: HermoMetrics.convToolFontSize, weight: .medium))
        }
        .buttonStyle(.glass)
        .buttonBorderShape(.capsule)
        .accessibilityIdentifier("hermo.chat.transcript.approvalJumpPill")
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
            .padding(20)
            .background(tokens.background)
    }
}

#Preview("Light") {
    ThemedSwatch(.light) {
        ApprovalJumpPill(action: {})
    }
}

#Preview("Dark") {
    ThemedSwatch(.dark) {
        ApprovalJumpPill(action: {})
    }
}
