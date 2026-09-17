import HermoLogic
import SwiftUI

/// The assembled chat screen: titlebar, tab strip, error banner, transcript, status strip and
/// composer, in the order `ChatScreen.kt`'s `Column` renders them. The bot drawer, appearance
/// sheet, session picker and slash UI are later tasks, so this screen exposes only the surfaces
/// T13 builds; the rename dialog is the one overlay it owns outright, since its state (open or
/// closed) is chrome, not app state.
public struct ChatScreen: View {
    public struct Value: Equatable, Sendable {
        public var themeMode: ThemeMode
        public var model: String
        public var tabs: [SessionTabStrip.Tab]
        public var currentKey: String?
        public var errorText: String
        public var session: SessionUiState
        public var draft: String
        public var prefill: String?

        public init(
            themeMode: ThemeMode,
            model: String,
            tabs: [SessionTabStrip.Tab],
            currentKey: String?,
            errorText: String,
            session: SessionUiState,
            draft: String,
            prefill: String? = nil
        ) {
            self.themeMode = themeMode
            self.model = model
            self.tabs = tabs
            self.currentKey = currentKey
            self.errorText = errorText
            self.session = session
            self.draft = draft
            self.prefill = prefill
        }
    }

    private let value: Value
    private let onMenuTap: () -> Void
    private let onTitleTap: () -> Void
    private let onAppearanceTap: () -> Void
    private let onRename: (String) -> Void
    private let onSelectTab: (String) -> Void
    private let onCloseTab: (String) -> Void
    private let onAddTab: () -> Void
    private let onDraftChanged: (String) -> Void
    private let onSend: (String) -> Void
    private let onStop: () -> Void
    private let onApprovalChoice: (_ requestId: String, _ choice: String) -> Void
    private let onClarifyAnswer: (_ requestId: String, _ answer: String, _ questionId: String?) -> Void

    @State private var renameOpen = false
    @State private var rowCache = TranscriptRows()
    @Environment(\.hermoTokens) private var tokens

    public init(
        value: Value,
        onMenuTap: @escaping () -> Void,
        onTitleTap: @escaping () -> Void,
        onAppearanceTap: @escaping () -> Void,
        onRename: @escaping (String) -> Void,
        onSelectTab: @escaping (String) -> Void,
        onCloseTab: @escaping (String) -> Void,
        onAddTab: @escaping () -> Void,
        onDraftChanged: @escaping (String) -> Void,
        onSend: @escaping (String) -> Void,
        onStop: @escaping () -> Void,
        onApprovalChoice: @escaping (_ requestId: String, _ choice: String) -> Void,
        onClarifyAnswer: @escaping (_ requestId: String, _ answer: String, _ questionId: String?) -> Void
    ) {
        self.value = value
        self.onMenuTap = onMenuTap
        self.onTitleTap = onTitleTap
        self.onAppearanceTap = onAppearanceTap
        self.onRename = onRename
        self.onSelectTab = onSelectTab
        self.onCloseTab = onCloseTab
        self.onAddTab = onAddTab
        self.onDraftChanged = onDraftChanged
        self.onSend = onSend
        self.onStop = onStop
        self.onApprovalChoice = onApprovalChoice
        self.onClarifyAnswer = onClarifyAnswer
    }

    public var body: some View {
        let rows = rowCache.of(value.session.rows)
        ZStack {
            VStack(spacing: 0) {
                ChatTitlebar(
                    title: value.session.title,
                    themeMode: value.themeMode,
                    onMenuTap: onMenuTap,
                    onEditTitleTap: { renameOpen = true },
                    onTitleTap: onTitleTap,
                    onAppearanceTap: onAppearanceTap
                )
                SessionTabStrip(
                    tabs: value.tabs,
                    currentKey: value.currentKey,
                    onSelect: onSelectTab,
                    onClose: onCloseTab,
                    onAdd: onAddTab
                )
                ErrorBanner(text: value.errorText)
                Transcript(
                    rows: rows,
                    running: value.session.running,
                    onApprovalChoice: onApprovalChoice,
                    onClarifyAnswer: onClarifyAnswer
                )
                .frame(maxHeight: .infinity)
                StatusStrip(state: value.session, rows: rows)
                Composer(
                    model: value.model,
                    running: value.session.running,
                    draft: value.draft,
                    prefill: value.prefill,
                    onDraftChanged: onDraftChanged,
                    onSend: onSend,
                    onStop: onStop
                )
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(tokens.background)

            if renameOpen {
                // The dialog submits against whatever session is current when Save is tapped, so
                // the scrim has to swallow taps on the tab strip underneath it.
                tokens.scrim
                    .ignoresSafeArea()
                    .contentShape(Rectangle())
                    .onTapGesture { renameOpen = false }
                RenameDialog(
                    current: value.session.title,
                    onConfirm: { name in
                        renameOpen = false
                        onRename(name)
                    },
                    onDismiss: { renameOpen = false }
                )
            }
        }
        .onChange(of: value.session.key) { _, _ in renameOpen = false }
    }
}

#Preview("Light") {
    ChatScreen(
        value: ChatScreen.Value(
            themeMode: .light,
            model: "claude-sonnet-4.5",
            tabs: [
                .init(key: "a", title: "Refactor plan", running: false),
                .init(key: "b", title: "Running a long task", running: true),
            ],
            currentKey: "a",
            errorText: "",
            session: SessionUiState(
                key: "a",
                title: "Refactor plan",
                model: "claude-sonnet-4.5",
                rows: [],
                running: false
            ),
            draft: ""
        ),
        onMenuTap: {},
        onTitleTap: {},
        onAppearanceTap: {},
        onRename: { _ in },
        onSelectTab: { _ in },
        onCloseTab: { _ in },
        onAddTab: {},
        onDraftChanged: { _ in },
        onSend: { _ in },
        onStop: {},
        onApprovalChoice: { _, _ in },
        onClarifyAnswer: { _, _, _ in }
    )
    .hermoTheme(.light)
}

#Preview("Dark") {
    ChatScreen(
        value: ChatScreen.Value(
            themeMode: .dark,
            model: "claude-sonnet-4.5",
            tabs: [.init(key: "a", title: "", running: true)],
            currentKey: "a",
            errorText: "error: session registry unreachable",
            session: SessionUiState(
                key: "a",
                title: "",
                model: "claude-sonnet-4.5",
                rows: [],
                running: true
            ),
            draft: "hello"
        ),
        onMenuTap: {},
        onTitleTap: {},
        onAppearanceTap: {},
        onRename: { _ in },
        onSelectTab: { _ in },
        onCloseTab: { _ in },
        onAddTab: {},
        onDraftChanged: { _ in },
        onSend: { _ in },
        onStop: {},
        onApprovalChoice: { _, _ in },
        onClarifyAnswer: { _, _, _ in }
    )
    .hermoTheme(.dark)
}
