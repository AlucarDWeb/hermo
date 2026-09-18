import ComposableArchitecture
import HermoFeatures
import HermoLogic
import HermoUI
import SwiftUI

@main
struct HermoApp: App {
    static let store = Store(initialState: AppFeature.State()) {
        AppFeature()
    }

    var body: some Scene {
        WindowGroup {
            // Fixed until AppearanceFeature is composed into AppFeature (T17).
            RootView(store: Self.store)
                .hermoTheme(.system)
        }
    }
}

/// Renders the screen for the current `AppFeature.State.phase`, porting `MainActivity.kt:154-209`.
struct RootView: View {
    let store: StoreOf<AppFeature>

    @Environment(\.hermoTokens) private var tokens
    @State private var showScanner = false
    @State private var lastOpenedURL: URL?

    var body: some View {
        Group {
            switch store.phase {
            case .unpaired:
                PairingScreen(
                    pairingPayload: store.pairingPayload,
                    errorText: store.errorText,
                    onPairingPayloadChanged: { store.send(.pairingPayloadChanged($0)) },
                    onScanRequested: { showScanner = true },
                    onPair: { store.send(.pair(store.pairingPayload)) }
                )
                .sheet(isPresented: $showScanner) {
                    QrScanView(
                        onDecoded: { payload in
                            showScanner = false
                            store.send(.pair(payload))
                        },
                        onCancel: { showScanner = false },
                        onPermissionDenied: {
                            // The message lands on the pairing screen, so the sheet has to go.
                            showScanner = false
                            store.send(.cameraPermissionDenied)
                        }
                    )
                }

            case let .needsPassword(endpoint, overlay):
                if overlay {
                    // The ask arrived mid-session: the sheet opens over the still-mounted chat screen.
                    ZStack {
                        chatScreen(model: store.lastReadyModel)
                        tokens.scrim.ignoresSafeArea()
                        PasswordSheet(
                            endpoint: endpoint,
                            errorText: store.errorText,
                            overlay: true,
                            onSubmit: { store.send(.loginAndConnect($0)) },
                            onLogout: { store.send(.forgetGateway) }
                        )
                    }
                } else {
                    PasswordSheet(
                        endpoint: endpoint,
                        errorText: store.errorText,
                        onSubmit: { store.send(.loginAndConnect($0)) },
                        onLogout: { store.send(.forgetGateway) }
                    )
                }

            case .connecting:
                ConnectingScreen()

            case let .ready(model):
                chatScreen(model: model)

            case let .offline(reason):
                OfflineScreen(
                    reason: reason,
                    onRetry: { store.send(.tryResume) },
                    onResetSessions: { store.send(.resetSessionsAndRestart) },
                    onLogout: { store.send(.forgetGateway) }
                )
            }
        }
        .task {
            store.send(.tryResume)
            store.send(.task)
        }
        .onOpenURL { url in
            guard url.scheme == "hermes" else { return }
            guard url != lastOpenedURL else { return }
            lastOpenedURL = url
            store.send(.pair(url.absoluteString))
        }
        .onChange(of: store.phase) { _, phase in
            if case .unpaired = phase {
                // Back at the pairing screen after a logout: the same link has to work again.
                lastOpenedURL = nil
            } else {
                showScanner = false
            }
        }
        .onChange(of: store.errorText) { _, text in
            // A failed pair leaves the phase untouched, so only the error line marks the retry point.
            if !text.isEmpty { lastOpenedURL = nil }
        }
    }

    /// Builds the chat screen's display value from `AppFeature.State` and wires its closures to
    /// the actions that exist today. The bot drawer and appearance sheet have no reducer
    /// composed into `AppFeature` yet (T17), so their taps are no-ops until then.
    private func chatScreen(model: String) -> ChatScreen {
        let tabs = store.tabs.keys.map { key in
            SessionTabStrip.Tab(
                key: key,
                title: store.sessions[key]?.title ?? "",
                running: store.sessions[key]?.running ?? false
            )
        }
        let session = store.currentKey.flatMap { store.sessions[$0] } ?? SessionUiState(key: "")
        let value = ChatScreen.Value(
            themeMode: .system,
            model: model,
            tabs: tabs,
            currentKey: store.currentKey,
            errorText: store.errorText,
            session: session,
            draft: store.chat.draft,
            slashCompletions: store.chat.slashCompletions,
            slashReplaceFrom: store.chat.slashReplaceFrom,
            slashBanner: store.chat.slashBanner
        )
        return ChatScreen(
            value: value,
            onMenuTap: {},
            onTitleTap: { store.send(.chat(.delegate(.openSessionPicker))) },
            onAppearanceTap: {},
            onRename: { store.send(.setSessionTitle(title: $0)) },
            onSelectTab: { store.send(.switchTab(key: $0)) },
            onCloseTab: { store.send(.closeTab(key: $0)) },
            onAddTab: { store.send(.openNewSession) },
            onDraftChanged: { store.send(.chat(.draftChanged($0))) },
            onDismissCompletions: { store.send(.chat(.dismissCompletions)) },
            onSend: { _ in store.send(.chat(.send(key: store.currentKey))) },
            onStop: {
                guard let key = store.currentKey else { return }
                store.send(.chat(.interrupt(key: key)))
            },
            onApprovalChoice: { requestId, choice in
                store.send(.chat(.respondApproval(key: store.currentKey, requestId: requestId, choice: choice)))
            },
            onClarifyAnswer: { requestId, answer, questionId in
                store.send(.chat(.respondClarify(key: store.currentKey, requestId: requestId, answer: answer, questionId: questionId ?? "")))
            }
        )
    }
}
