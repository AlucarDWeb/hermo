import HermoLogic
import SwiftUI

/// The assembled chat screen: titlebar, tab strip, error banner, transcript, status strip, slash
/// completions and banner, and composer, in the order `ChatScreen.kt`'s `Column` renders them.
/// It also owns every overlay the titlebar opens: the rename dialog, the bot drawer and the
/// appearance sheet are chrome-only state kept locally, while the session picker's presented
/// flag comes from `SessionPickerFeature.State` since a reducer already tracks it.
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
        public var slashCompletions: [SlashCompletionRow]
        public var slashReplaceFrom: Int64
        public var slashBanner: String
        public var sessionPickerPresented: Bool
        public var sessionPickerLoading: Bool
        public var sessionPickerSessions: [RemoteSessionRow]
        public var botDrawerState: DrawerUiState
        public var botDrawerOpen: Bool

        public init(
            themeMode: ThemeMode,
            model: String,
            tabs: [SessionTabStrip.Tab],
            currentKey: String?,
            errorText: String,
            session: SessionUiState,
            draft: String,
            prefill: String? = nil,
            slashCompletions: [SlashCompletionRow] = [],
            slashReplaceFrom: Int64 = 1,
            slashBanner: String = "",
            sessionPickerPresented: Bool = false,
            sessionPickerLoading: Bool = false,
            sessionPickerSessions: [RemoteSessionRow] = [],
            botDrawerState: DrawerUiState = .loading,
            botDrawerOpen: Bool = false
        ) {
            self.themeMode = themeMode
            self.model = model
            self.tabs = tabs
            self.currentKey = currentKey
            self.errorText = errorText
            self.session = session
            self.draft = draft
            self.prefill = prefill
            self.slashCompletions = slashCompletions
            self.slashReplaceFrom = slashReplaceFrom
            self.slashBanner = slashBanner
            self.sessionPickerPresented = sessionPickerPresented
            self.sessionPickerLoading = sessionPickerLoading
            self.sessionPickerSessions = sessionPickerSessions
            self.botDrawerState = botDrawerState
            self.botDrawerOpen = botDrawerOpen
        }
    }

    private let value: Value
    private let onMenuTap: () -> Void
    private let onTitleTap: () -> Void
    private let onRename: (String) -> Void
    private let onSelectTab: (String) -> Void
    private let onCloseTab: (String) -> Void
    private let onAddTab: () -> Void
    private let onDraftChanged: (String) -> Void
    private let onDismissCompletions: () -> Void
    private let onSend: (String) -> Void
    private let onStop: () -> Void
    private let onApprovalChoice: (_ requestId: String, _ choice: String) -> Void
    private let onClarifyAnswer: (_ requestId: String, _ answer: String, _ questionId: String?) -> Void
    private let onSessionPickerRowTap: (String) -> Void
    private let onSessionPickerNewChat: () -> Void
    private let onSessionPickerDismiss: () -> Void
    private let onBotDrawerProfileTap: (String) -> Void
    private let onBotDrawerRetry: () -> Void
    private let onBotDrawerLogout: () -> Void
    private let onBotDrawerDismiss: () -> Void
    private let onAppearanceModeChange: (ThemeMode) -> Void

    @State private var renameOpen = false
    @State private var appearanceSheetOpen = false
    @State private var rowCache = TranscriptRows()
    @Environment(\.hermoTokens) private var tokens

    public init(
        value: Value,
        onMenuTap: @escaping () -> Void,
        onTitleTap: @escaping () -> Void,
        onRename: @escaping (String) -> Void,
        onSelectTab: @escaping (String) -> Void,
        onCloseTab: @escaping (String) -> Void,
        onAddTab: @escaping () -> Void,
        onDraftChanged: @escaping (String) -> Void,
        onDismissCompletions: @escaping () -> Void,
        onSend: @escaping (String) -> Void,
        onStop: @escaping () -> Void,
        onApprovalChoice: @escaping (_ requestId: String, _ choice: String) -> Void,
        onClarifyAnswer: @escaping (_ requestId: String, _ answer: String, _ questionId: String?) -> Void,
        onSessionPickerRowTap: @escaping (String) -> Void,
        onSessionPickerNewChat: @escaping () -> Void,
        onSessionPickerDismiss: @escaping () -> Void,
        onBotDrawerProfileTap: @escaping (String) -> Void,
        onBotDrawerRetry: @escaping () -> Void,
        onBotDrawerDismiss: @escaping () -> Void,
        onBotDrawerLogout: @escaping () -> Void,
        onAppearanceModeChange: @escaping (ThemeMode) -> Void
    ) {
        self.value = value
        self.onMenuTap = onMenuTap
        self.onTitleTap = onTitleTap
        self.onRename = onRename
        self.onSelectTab = onSelectTab
        self.onCloseTab = onCloseTab
        self.onAddTab = onAddTab
        self.onDraftChanged = onDraftChanged
        self.onDismissCompletions = onDismissCompletions
        self.onSend = onSend
        self.onStop = onStop
        self.onApprovalChoice = onApprovalChoice
        self.onClarifyAnswer = onClarifyAnswer
        self.onSessionPickerRowTap = onSessionPickerRowTap
        self.onSessionPickerNewChat = onSessionPickerNewChat
        self.onSessionPickerDismiss = onSessionPickerDismiss
        self.onBotDrawerProfileTap = onBotDrawerProfileTap
        self.onBotDrawerRetry = onBotDrawerRetry
        self.onBotDrawerDismiss = onBotDrawerDismiss
        self.onBotDrawerLogout = onBotDrawerLogout
        self.onAppearanceModeChange = onAppearanceModeChange
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
                    onAppearanceTap: { appearanceSheetOpen = true }
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
                SlashCompletionsPopup(completions: value.slashCompletions) { item in
                    let next = SlashPolicy.insertCompletion(value.draft, item.text, value.slashReplaceFrom)
                    onDismissCompletions()
                    onDraftChanged(next)
                }
                SlashBanner(text: value.slashBanner)
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

            BotDrawer(
                isOpen: value.botDrawerOpen,
                state: value.botDrawerState,
                onProfileTap: { row in onBotDrawerProfileTap(row.profile) },
                onRetry: onBotDrawerRetry,
                onDismiss: onBotDrawerDismiss,
                onLogout: onBotDrawerLogout
            )
        }
        .onChange(of: value.session.key) { _, _ in renameOpen = false }
        .sheet(
            isPresented: Binding(
                get: { value.sessionPickerPresented },
                set: { isPresented in
                    if !isPresented { onSessionPickerDismiss() }
                }
            )
        ) {
            SessionPickerSheet(
                sessions: value.sessionPickerSessions,
                loading: value.sessionPickerLoading,
                onResume: onSessionPickerRowTap,
                onNewChat: onSessionPickerNewChat
            )
        }
        .sheet(isPresented: $appearanceSheetOpen) {
            AppearanceSheet(
                mode: value.themeMode,
                onModeChange: { mode in
                    appearanceSheetOpen = false
                    onAppearanceModeChange(mode)
                }
            )
        }
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
        onRename: { _ in },
        onSelectTab: { _ in },
        onCloseTab: { _ in },
        onAddTab: {},
        onDraftChanged: { _ in },
        onDismissCompletions: {},
        onSend: { _ in },
        onStop: {},
        onApprovalChoice: { _, _ in },
        onClarifyAnswer: { _, _, _ in },
        onSessionPickerRowTap: { _ in },
        onSessionPickerNewChat: {},
        onSessionPickerDismiss: {},
        onBotDrawerProfileTap: { _ in },
        onBotDrawerRetry: {},
        onBotDrawerDismiss: {},
        onBotDrawerLogout: {},
        onAppearanceModeChange: { _ in }
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
        onRename: { _ in },
        onSelectTab: { _ in },
        onCloseTab: { _ in },
        onAddTab: {},
        onDraftChanged: { _ in },
        onDismissCompletions: {},
        onSend: { _ in },
        onStop: {},
        onApprovalChoice: { _, _ in },
        onClarifyAnswer: { _, _, _ in },
        onSessionPickerRowTap: { _ in },
        onSessionPickerNewChat: {},
        onSessionPickerDismiss: {},
        onBotDrawerProfileTap: { _ in },
        onBotDrawerRetry: {},
        onBotDrawerDismiss: {},
        onBotDrawerLogout: {},
        onAppearanceModeChange: { _ in }
    )
    .hermoTheme(.dark)
}
