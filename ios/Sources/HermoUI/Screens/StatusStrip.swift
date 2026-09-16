import SwiftUI
import HermoLogic

/// The session status strip: connection dot, working/ready label, usage chip and elapsed turn
/// timer, ported from `ChatScreen.kt`'s `StatusStrip`.
public struct StatusStrip: View {
    private let state: SessionUiState
    private let rows: [ChatRow]

    @Environment(\.hermoTokens) private var tokens
    @State private var elapsed: Int64 = 0

    public init(state: SessionUiState, rows: [ChatRow]) {
        self.state = state
        self.rows = rows
    }

    public var body: some View {
        if state.key.isEmpty {
            EmptyView()
        } else {
            strip
        }
    }

    private var strip: some View {
        HStack(spacing: 8) {
            Circle()
                .fill(state.running ? tokens.midground : tokens.successDot)
                .frame(width: 6, height: 6)
                .accessibilityIdentifier("hermo.chat.statusStrip.dot")
            Text(state.running ? "Hermes is working" : "Ready")
                .font(.system(size: HermoMetrics.convToolFontSize))
                .foregroundStyle(tokens.scaffoldText)
                .accessibilityIdentifier("hermo.chat.statusStrip.label")
            Spacer()
            if !usageLabel.isEmpty {
                Text(usageLabel)
                    .font(.system(size: 10))
                    .foregroundStyle(tokens.scaffoldMeta)
                    .accessibilityIdentifier("hermo.chat.statusStrip.usage")
            }
            if state.running || elapsed > 0 {
                Text(ToolCardModel.formatElapsed(elapsed))
                    .font(.system(size: 9))
                    .tracking(0.2)
                    .foregroundStyle(tokens.midground.opacity(0.55))
                    .accessibilityIdentifier("hermo.chat.statusStrip.elapsed")
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 2)
        .accessibilityIdentifier("hermo.chat.statusStrip")
        // Mirrors the Kotlin pair of effects: this one resets on a session switch alone.
        .task(id: state.key) {
            elapsed = 0
        }
        // This one restarts the count whenever the session or its running state changes,
        // so a switch back to an already-running session does not inherit a stale value.
        .task(id: StatusStripTickKey(key: state.key, running: state.running)) {
            guard state.running else { return }
            elapsed = 0
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(1))
                if Task.isCancelled { return }
                elapsed += 1
            }
        }
    }

    /// The most recent assistant row's usage payload, Desktop's `usageContextLabel` by way of `ToolCardModel`.
    private var usageLabel: String {
        for row in rows.reversed() {
            if case .assistant(_, _, _, _, let usageJson) = row {
                return ToolCardModel.usageLabel(usageJson)
            }
        }
        return ""
    }
}

private struct StatusStripTickKey: Equatable {
    let key: String
    let running: Bool
}

#Preview("Light — working") {
    StatusStrip(
        state: SessionUiState(key: "s1", running: true),
        rows: [.assistant(id: 0, text: "…", streaming: true, warning: "", usageJson: "")]
    )
    .hermoTheme(.light)
}

#Preview("Light — ready") {
    StatusStrip(
        state: SessionUiState(key: "s1", running: false),
        rows: [
            .assistant(
                id: 0,
                text: "Done.",
                streaming: false,
                warning: "",
                usageJson: "{\"total\":1234}"
            ),
        ]
    )
    .hermoTheme(.light)
}

#Preview("Dark — working") {
    StatusStrip(
        state: SessionUiState(key: "s1", running: true),
        rows: [.assistant(id: 0, text: "…", streaming: true, warning: "", usageJson: "")]
    )
    .hermoTheme(.dark)
}

#Preview("Dark — ready") {
    StatusStrip(
        state: SessionUiState(key: "s1", running: false),
        rows: [
            .assistant(
                id: 0,
                text: "Done.",
                streaming: false,
                warning: "",
                usageJson: "{\"total\":1234}"
            ),
        ]
    )
    .hermoTheme(.dark)
}
