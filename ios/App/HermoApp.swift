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
            // T13 replaces the fixed mode with AppearanceFeature's persisted one.
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
                    // The ask arrived mid-session: the sheet opens over the still-mounted ready screen.
                    ZStack {
                        ReadyPlaceholder(model: store.lastReadyModel)
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
                ReadyPlaceholder(model: model)

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
}

/// Stands in for the Ready phase until T13 builds the real chat screen.
private struct ReadyPlaceholder: View {
    let model: String

    var body: some View {
        VStack(spacing: 8) {
            Text("hermo")
                .font(.largeTitle.weight(.semibold))
                .accessibilityIdentifier("hermo.shell.title")
            Text(model.isEmpty ? "Ready" : "Ready, model: \(model)")
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("hermo.shell.subtitle")
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
