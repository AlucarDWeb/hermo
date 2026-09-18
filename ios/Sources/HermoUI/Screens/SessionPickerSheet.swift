import HermoLogic
import SwiftUI

/// The session picker (Android's `SessionPickerSheet.kt`): title, loading state, the "New chat"
/// row, then the remote sessions. Liquid Glass trades the Kotlin's own scrim and bottom sheet
/// for a native `.sheet` with `.presentationDetents`, glass on iOS 26 on its own (plan section
/// 3.4), so this view carries no scrim and no dismiss closure.
public struct SessionPickerSheet: View {
    private static let listMaxHeight: CGFloat = 420

    private let sessions: [RemoteSessionRow]
    private let loading: Bool
    private let onResume: (String) -> Void
    private let onNewChat: () -> Void

    /// A `ScrollView` takes every point offered along its axis, so a short list would claim the
    /// full 420 with empty space beneath its rows; measuring the rows keeps 420 a ceiling.
    @State private var listHeight: CGFloat = 0
    @Environment(\.hermoTokens) private var tokens

    public init(
        sessions: [RemoteSessionRow],
        loading: Bool,
        onResume: @escaping (String) -> Void,
        onNewChat: @escaping () -> Void
    ) {
        self.sessions = sessions
        self.loading = loading
        self.onResume = onResume
        self.onNewChat = onNewChat
    }

    public var body: some View {
        VStack(spacing: 0) {
            Text("Sessions")
                .font(.system(size: HermoMetrics.convFontSize, weight: .medium))
                .foregroundStyle(tokens.textSecondary)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
                .accessibilityIdentifier("hermo.picker.title")
            Rectangle()
                .fill(tokens.strokeTertiary)
                .frame(height: 1)
            if loading {
                HStack {
                    ProgressView()
                        .tint(tokens.midground)
                }
                .frame(maxWidth: .infinity)
                .padding(24)
                .accessibilityIdentifier("hermo.picker.loading")
            } else {
                ScrollView {
                    VStack(spacing: 0) {
                        // Rendered unconditionally, ahead of the empty check: gating it on a
                        // non-empty session list makes "New chat" unreachable on a fresh
                        // install or a failed load.
                        SessionPickerRow(
                            title: "New chat",
                            preview: "Start a fresh conversation",
                            identifier: "hermo.picker.newChat",
                            onTap: onNewChat
                        )
                        if sessions.isEmpty {
                            Text("No other sessions on the gateway yet")
                                .font(.system(size: HermoMetrics.convToolFontSize))
                                .foregroundStyle(tokens.textTertiary)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .padding(.horizontal, 16)
                                .padding(.vertical, 12)
                                .accessibilityIdentifier("hermo.picker.empty")
                        } else {
                            ForEach(sessions) { session in
                                SessionPickerRow(
                                    title: session.displayTitle,
                                    preview: session.displayPreview,
                                    identifier: "hermo.picker.row.\(session.id)",
                                    onTap: { onResume(session.id) }
                                )
                            }
                        }
                    }
                    .onGeometryChange(for: CGFloat.self, of: { $0.size.height }) { listHeight = $0 }
                }
                .frame(height: min(listHeight, Self.listMaxHeight))
            }
        }
        .padding(.vertical, 8)
        .presentationDetents([.medium, .large])
    }
}

/// One row: a dot, the title, and the preview line, shown only when it is not blank.
private struct SessionPickerRow: View {
    let title: String
    let preview: String
    let identifier: String
    let onTap: () -> Void
    @Environment(\.hermoTokens) private var tokens

    var body: some View {
        HStack(spacing: 8) {
            Circle()
                .fill(tokens.midground)
                .frame(width: 8, height: 8)
                .padding(4)
            VStack(alignment: .leading, spacing: 0) {
                Text(title)
                    .font(.system(size: HermoMetrics.convFontSize))
                    .foregroundStyle(tokens.text)
                    .lineLimit(1)
                    .truncationMode(.tail)
                if !preview.isEmpty {
                    Text(preview)
                        .font(.system(size: HermoMetrics.convToolFontSize))
                        .foregroundStyle(tokens.textTertiary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .contentShape(Rectangle())
        .onTapGesture(perform: onTap)
        // `children: .contain` keeps the title and preview addressable; identifying the
        // row as one element swallows them.
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier(identifier)
    }
}

private let previewSessions = [
    RemoteSessionRow(id: "1", title: "Refactor the picker", preview: "Let's port the session picker sheet", messageCount: 4),
    RemoteSessionRow(id: "2", title: "", preview: "", messageCount: 0),
    RemoteSessionRow(id: "3", title: "Long running task", preview: "Still working through the fixture recording", messageCount: 12),
]

#Preview("Loading — Light") {
    SessionPickerSheet(sessions: [], loading: true, onResume: { _ in }, onNewChat: {})
        .hermoTheme(.light)
}

#Preview("Loading — Dark") {
    SessionPickerSheet(sessions: [], loading: true, onResume: { _ in }, onNewChat: {})
        .hermoTheme(.dark)
}

#Preview("Populated — Light") {
    SessionPickerSheet(sessions: previewSessions, loading: false, onResume: { _ in }, onNewChat: {})
        .hermoTheme(.light)
}

#Preview("Populated — Dark") {
    SessionPickerSheet(sessions: previewSessions, loading: false, onResume: { _ in }, onNewChat: {})
        .hermoTheme(.dark)
}

#Preview("Empty — Light") {
    SessionPickerSheet(sessions: [], loading: false, onResume: { _ in }, onNewChat: {})
        .hermoTheme(.light)
}

#Preview("Empty — Dark") {
    SessionPickerSheet(sessions: [], loading: false, onResume: { _ in }, onNewChat: {})
        .hermoTheme(.dark)
}
