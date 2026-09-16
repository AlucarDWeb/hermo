import ComposableArchitecture
import Foundation
import HermesCore
import HermoGateway
import HermoLogic

@Reducer
public struct AppFeature: Sendable {
    @Dependency(\.gatewayClient) var gatewayClient
    @Dependency(\.screenCols) var screenCols

    public init() {}

    private enum CancelID: Hashable {
        case events
        case connect
    }

    @ObservableState
    public struct State: Equatable, Sendable {
        public var phase: AppPhase
        public var endpointText: String
        public var lastReadyModel: String
        public var errorText: String
        public var pairingPayload: String

        public init(
            phase: AppPhase = .unpaired,
            endpointText: String = "",
            lastReadyModel: String = "",
            errorText: String = "",
            pairingPayload: String = ""
        ) {
            self.phase = phase
            self.endpointText = endpointText
            self.lastReadyModel = lastReadyModel
            self.errorText = errorText
            self.pairingPayload = pairingPayload
        }

        /// Every phase change is routed through here so `lastReadyModel` (the T11 re-login overlay's fallback) tracks Ready the same way `AppViewModel`'s separate collector did.
        fileprivate mutating func setPhase(_ newPhase: AppPhase) {
            phase = newPhase
            if case let .ready(model) = newPhase {
                lastReadyModel = model
            }
        }
    }

    public enum Action: Equatable, Sendable {
        case task
        case tryResume
        case pair(String)
        case loginAndConnect(String)
        case forgetGateway
        case resetSessionsAndRestart
        case appDidForeground
        case disconnect
        case networkAvailable
        case cameraPermissionDenied
        case pairingPayloadChanged(String)
        case core(CoreEvent)

        case resumeStarted(endpointText: String)
        case paired(endpointText: String)
        case pairFailed(errorText: String)
        case connectFailed(errorText: String, isAuthShape: Bool, closedReason: String)
        case mainSessionReady
        case mainSessionClosed(reason: String, errorText: String?)
        case forgotGateway
        case forgetGatewayFailed(errorText: String)
        case resetSessionsFailed(errorText: String)
        case sessionsCleared
        case newSessionFailed(errorText: String)
        case disconnectFailed(errorText: String)
    }

    public var body: some ReducerOf<Self> {
        Reduce { state, action in
            switch action {
            case .task:
                return .run { [gatewayClient] send in
                    for await event in gatewayClient.events() {
                        await send(.core(event))
                    }
                }
                .cancellable(id: CancelID.events, cancelInFlight: true)

            case .tryResume:
                return .run { [gatewayClient, screenCols] send in
                    guard let endpoint = await gatewayClient.savedEndpoint() else { return }
                    await send(.resumeStarted(endpointText: endpoint.displayText))
                    let cols = await screenCols.columns()
                    let outcome = await Self.connectThenOpenMainSession(cols: cols, gatewayClient: gatewayClient)
                    await send(outcome)
                }
                .cancellable(id: CancelID.connect, cancelInFlight: true)

            case let .pair(payload):
                return .run { [gatewayClient] send in
                    do {
                        let endpoint = try await gatewayClient.pair(payload: payload)
                        await send(.paired(endpointText: endpoint.displayText))
                    } catch {
                        await send(.pairFailed(errorText: ErrorMessages.of(error)))
                    }
                }

            case let .loginAndConnect(password):
                state.setPhase(.connecting)
                return .run { [gatewayClient, screenCols] send in
                    let cols = await screenCols.columns()
                    let outcome = await Self.loginConnectThenOpenMainSession(
                        password: password, cols: cols, gatewayClient: gatewayClient
                    )
                    await send(outcome)
                }
                .cancellable(id: CancelID.connect, cancelInFlight: true)

            case .forgetGateway:
                return .run { [gatewayClient] send in
                    do {
                        try await gatewayClient.forgetGateway()
                        await send(.forgotGateway)
                    } catch {
                        await send(.forgetGatewayFailed(errorText: ErrorMessages.of(error)))
                    }
                }

            case .resetSessionsAndRestart:
                return .run { [gatewayClient, screenCols] send in
                    do {
                        try await gatewayClient.clearSessions()
                    } catch {
                        await send(.resetSessionsFailed(errorText: ErrorMessages.of(error)))
                        return
                    }
                    await send(.sessionsCleared)
                    let cols = await screenCols.columns()
                    do {
                        _ = try await gatewayClient.openSession(storedId: nil, cols: Int64(cols))
                        await send(.mainSessionReady)
                    } catch {
                        await send(.newSessionFailed(errorText: ErrorMessages.of(error)))
                    }
                }

            case .appDidForeground:
                return .run { [gatewayClient] _ in
                    try? await gatewayClient.appDidForeground()
                }

            case .disconnect:
                return .run { [gatewayClient] send in
                    do {
                        try await gatewayClient.disconnect()
                    } catch {
                        await send(.disconnectFailed(errorText: ErrorMessages.of(error)))
                    }
                }

            case .networkAvailable:
                guard case .offline = state.phase else { return .none }
                return .send(.tryResume)

            case .cameraPermissionDenied:
                state.errorText = "Camera permission is needed to scan the QR — or paste the payload below"
                return .none

            case let .pairingPayloadChanged(text):
                state.pairingPayload = text
                return .none

            case let .core(event):
                if case let .connection(status) = event {
                    Self.onConnectionStatus(status, state: &state)
                }
                return .none

            case let .resumeStarted(endpointText):
                state.endpointText = endpointText
                state.setPhase(.connecting)
                return .none

            case let .paired(endpointText):
                state.endpointText = endpointText
                state.errorText = ""
                state.setPhase(.needsPassword(endpoint: endpointText))
                return .none

            case let .pairFailed(errorText):
                state.errorText = errorText
                return .none

            case let .connectFailed(errorText, isAuthShape, closedReason):
                // Applied before the shape check, same as the Kotlin: the auth branch below discards `closedReason` but the error line must still land.
                state.errorText = errorText
                if isAuthShape {
                    state.setPhase(PhaseMachine.reduce(phase: state.phase, event: .authFailed, savedEndpoint: state.endpointText))
                } else {
                    state.setPhase(
                        PhaseMachine.reduce(phase: state.phase, event: .closed(reason: closedReason), savedEndpoint: state.endpointText)
                    )
                }
                return .none

            case .mainSessionReady:
                Self.afterOpen(state: &state)
                return .none

            case let .mainSessionClosed(reason, errorText):
                if let errorText {
                    state.errorText = errorText
                }
                state.setPhase(PhaseMachine.reduce(phase: state.phase, event: .closed(reason: reason), savedEndpoint: state.endpointText))
                return .none

            case .forgotGateway:
                state.errorText = ""
                state.endpointText = ""
                state.pairingPayload = ""
                state.setPhase(.unpaired)
                return .none

            case let .forgetGatewayFailed(errorText):
                state.errorText = errorText
                return .none

            case let .resetSessionsFailed(errorText):
                state.errorText = errorText
                return .none

            case .sessionsCleared:
                state.errorText = ""
                return .none

            case let .newSessionFailed(errorText):
                state.errorText = errorText
                return .none

            case let .disconnectFailed(errorText):
                state.errorText = errorText
                return .none
            }
        }
    }

    private static func onConnectionStatus(_ status: ConnectionStatus, state: inout State) {
        switch status {
        case .connecting:
            state.setPhase(PhaseMachine.reduce(phase: state.phase, event: .connecting, savedEndpoint: state.endpointText))
        case .open:
            state.setPhase(PhaseMachine.reduce(phase: state.phase, event: .open, savedEndpoint: state.endpointText))
        case let .closed(reason):
            state.setPhase(PhaseMachine.reduce(phase: state.phase, event: .closed(reason: reason), savedEndpoint: state.endpointText))
        case .needsPassword:
            state.setPhase(PhaseMachine.reduce(phase: state.phase, event: .needsPassword, savedEndpoint: state.endpointText))
        }
    }

    /// The connection half owns no session map yet (T09), so the header model is always unknown here.
    private static func afterOpen(state: inout State) {
        state.errorText = ""
        state.setPhase(PhaseMachine.ready(""))
    }

    private static func connectThenOpenMainSession(cols: Int, gatewayClient: GatewayClient) async -> Action {
        do {
            try await gatewayClient.connect()
        } catch {
            return connectFailure(error)
        }
        return await openMainSession(cols: cols, gatewayClient: gatewayClient)
    }

    private static func loginConnectThenOpenMainSession(
        password: String, cols: Int, gatewayClient: GatewayClient
    ) async -> Action {
        do {
            try await gatewayClient.login(password: password)
            try await gatewayClient.connect()
        } catch {
            return connectFailure(error)
        }
        return await openMainSession(cols: cols, gatewayClient: gatewayClient)
    }

    private static func connectFailure(_ error: Error) -> Action {
        .connectFailed(
            errorText: ErrorMessages.of(error),
            isAuthShape: ErrorMessages.isAuthShape(error),
            closedReason: "error: \(ErrorMessages.rustMessage(error))"
        )
    }

    /// `GatewayClient.openSessions()` cannot fail on this platform (the core call has no `Result`), so the registry-unreachable branch Android guards against never fires through this dependency.
    private static func openMainSession(cols: Int, gatewayClient: GatewayClient) async -> Action {
        let summaries = await gatewayClient.openSessions()
        let lastActive = await gatewayClient.lastActiveSession()
        let plan = restorePlan(keys: summaries.map(\.key), lastActive: lastActive)
        var resumed: [String] = []
        var lastErrorText: String?
        for key in plan.resumeKeys {
            do {
                _ = try await gatewayClient.openSession(storedId: key, cols: Int64(cols))
                resumed.append(key)
            } catch {
                lastErrorText = ErrorMessages.of(error)
            }
        }
        if !resumed.isEmpty {
            return .mainSessionReady
        }
        if !plan.resumeKeys.isEmpty {
            return .mainSessionClosed(reason: "error: no session could be resumed", errorText: lastErrorText)
        }
        do {
            _ = try await gatewayClient.openSession(storedId: nil, cols: Int64(cols))
            return .mainSessionReady
        } catch {
            return .mainSessionClosed(reason: "error: no session could be resumed", errorText: ErrorMessages.of(error))
        }
    }
}
