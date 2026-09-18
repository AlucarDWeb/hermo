import ComposableArchitecture
import Foundation
import HermesCore
import HermoGateway
import HermoLogic

@Reducer
public struct AppFeature: Sendable {
    @Dependency(\.gatewayClient) var gatewayClient
    @Dependency(\.screenCols) var screenCols
    @Dependency(\.networkMonitor) var networkMonitor

    public init() {}

    private enum CancelID: Hashable {
        case events
        case connect
        case network
    }

    @ObservableState
    public struct State: Equatable, Sendable {
        public var phase: AppPhase
        public var endpointText: String
        public var lastReadyModel: String
        public var errorText: String
        public var pairingPayload: String
        public var sessions: [String: SessionUiState]
        public var tabs: TabSet
        public var currentKey: String?
        public var pendingKeys: Set<String>
        public var parkedChanges: [TranscriptChangeDto]
        public var inFlightOpens: Int
        public var chat: ChatFeature.State
        public var sessionPicker: SessionPickerFeature.State
        public var botDrawer: BotDrawerFeature.State
        public var appearance: AppearanceFeature.State

        public init(
            phase: AppPhase = .unpaired,
            endpointText: String = "",
            lastReadyModel: String = "",
            errorText: String = "",
            pairingPayload: String = "",
            sessions: [String: SessionUiState] = [:],
            tabs: TabSet = TabSet(),
            currentKey: String? = nil,
            pendingKeys: Set<String> = [],
            parkedChanges: [TranscriptChangeDto] = [],
            inFlightOpens: Int = 0,
            chat: ChatFeature.State = ChatFeature.State(),
            sessionPicker: SessionPickerFeature.State = SessionPickerFeature.State(),
            botDrawer: BotDrawerFeature.State = BotDrawerFeature.State(),
            appearance: AppearanceFeature.State = AppearanceFeature.State()
        ) {
            self.phase = phase
            self.endpointText = endpointText
            self.lastReadyModel = lastReadyModel
            self.errorText = errorText
            self.pairingPayload = pairingPayload
            self.sessions = sessions
            self.tabs = tabs
            self.currentKey = currentKey
            self.pendingKeys = pendingKeys
            self.parkedChanges = parkedChanges
            self.inFlightOpens = inFlightOpens
            self.chat = chat
            self.sessionPicker = sessionPicker
            self.botDrawer = botDrawer
            self.appearance = appearance
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
        case resumePlanned(keys: [String])
        case mainSessionRestored(planned: [String], resumed: [String], summaries: [SessionSummary], current: String?)
        case mainSessionRestoreFailed(planned: [String], errorText: String?)
        case forgotGateway
        case forgetGatewayFailed(errorText: String)
        case resetSessionsFailed(errorText: String)
        case sessionsCleared
        case newSessionFailed(errorText: String)
        case disconnectFailed(errorText: String)
        case headerRefreshed(key: String, summary: SessionSummary)

        case openNewSession
        case openExistingSession(storedId: String)
        case openBotChat(profile: String)
        case switchTab(key: String)
        case closeTab(key: String)
        case setSessionTitle(title: String)

        case newSessionOpened(key: String)
        case existingSessionOpened(key: String)
        case botChatOpened(key: String, profile: String)
        case sessionOpenFailed(errorText: String)
        case sessionReady(key: String, summary: SessionSummary?)
        case tabClosed(key: String)
        case tabCloseFailed(errorText: String)
        case sessionTitleSet(key: String)
        case sessionTitleFailed(errorText: String)

        case chat(ChatFeature.Action)
        case sessionPicker(SessionPickerFeature.Action)
        case botDrawer(BotDrawerFeature.Action)
        case appearance(AppearanceFeature.Action)
    }

    public var body: some ReducerOf<Self> {
        Scope(state: \.chat, action: \.chat) {
            ChatFeature()
        }
        Scope(state: \.sessionPicker, action: \.sessionPicker) {
            SessionPickerFeature()
        }
        Scope(state: \.botDrawer, action: \.botDrawer) {
            BotDrawerFeature()
        }
        Scope(state: \.appearance, action: \.appearance) {
            AppearanceFeature()
        }
        Reduce { state, action in
            switch action {
            case .task:
                return .merge(
                    .run { [gatewayClient] send in
                        for await event in gatewayClient.events() {
                            await send(.core(event))
                        }
                    }
                    .cancellable(id: CancelID.events, cancelInFlight: true),
                    .run { [networkMonitor] send in
                        for await _ in networkMonitor.events() {
                            await send(.networkAvailable)
                        }
                    }
                    .cancellable(id: CancelID.network, cancelInFlight: true)
                )

            case .tryResume:
                return .run { [gatewayClient, screenCols] send in
                    guard let endpoint = await gatewayClient.savedEndpoint() else { return }
                    await send(.resumeStarted(endpointText: endpoint.displayText))
                    let cols = await screenCols.columns()
                    await Self.connectThenOpenMainSession(cols: cols, gatewayClient: gatewayClient, send: send)
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
                    await Self.loginConnectThenOpenMainSession(
                        password: password, cols: cols, gatewayClient: gatewayClient, send: send
                    )
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
                        let key = try await gatewayClient.openSession(storedId: nil, cols: Int64(cols))
                        await send(.newSessionOpened(key: key))
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
                switch event {
                case let .connection(status):
                    Self.onConnectionStatus(status, state: &state)
                    return .none
                case let .transcript(change):
                    return Self.onTranscriptChange(change, state: &state, gatewayClient: gatewayClient)
                }

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
                    state.setPhase(
                        PhaseMachine.reduce(
                            phase: state.phase, event: .authFailed, savedEndpoint: state.endpointText,
                            hasLiveSession: state.currentKey != nil
                        )
                    )
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

            case let .resumePlanned(keys):
                state.pendingKeys.formUnion(keys)
                state.inFlightOpens += 1
                return .none

            case let .mainSessionRestored(planned, resumed, summaries, planCurrent):
                for key in resumed {
                    Self.drainParked(key: key, state: &state)
                    state.sessions = ensureSession(state.sessions, key)
                    state.tabs = state.tabs.add(key)
                }
                state.pendingKeys.subtract(planned)
                Self.applySummaries(summaries, state: &state)
                let current = planCurrent.flatMap { resumed.contains($0) ? $0 : nil } ?? resumed.last
                if let current {
                    state.tabs = TabSet(keys: state.tabs.keys, current: current)
                    state.currentKey = current
                }
                Self.afterOpen(state: &state)
                Self.finishInFlightOpen(state: &state)
                return .none

            case let .mainSessionRestoreFailed(planned, errorText):
                state.pendingKeys.subtract(planned)
                if let errorText {
                    state.errorText = errorText
                }
                state.setPhase(
                    PhaseMachine.reduce(
                        phase: state.phase,
                        event: .closed(reason: "error: no session could be resumed"),
                        savedEndpoint: state.endpointText
                    )
                )
                Self.finishInFlightOpen(state: &state)
                return .none

            case .forgotGateway:
                state.errorText = ""
                state.endpointText = ""
                state.pairingPayload = ""
                Self.clearSessionState(&state)
                // The drawer keeps `.ready` rows across a reopen, so without this the next
                // gateway's drawer shows the forgotten one's profiles until its load lands.
                state.botDrawer = BotDrawerFeature.State()
                state.sessionPicker = SessionPickerFeature.State()
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
                Self.clearSessionState(&state)
                return .none

            case let .newSessionFailed(errorText):
                state.errorText = errorText
                return .none

            case let .disconnectFailed(errorText):
                state.errorText = errorText
                return .none

            case let .headerRefreshed(key, summary):
                Self.applyHeaderRefresh(key: key, summary: summary, state: &state)
                return .none

            case .openNewSession:
                state.inFlightOpens += 1
                return .run { [gatewayClient, screenCols] send in
                    let cols = await screenCols.columns()
                    do {
                        let key = try await gatewayClient.openSession(storedId: nil, cols: Int64(cols))
                        await send(.newSessionOpened(key: key))
                    } catch {
                        await send(.sessionOpenFailed(errorText: ErrorMessages.of(error)))
                    }
                }

            case let .openExistingSession(storedId):
                if state.tabs.keys.contains(storedId) {
                    return Self.performSwitchTab(key: storedId, state: &state, gatewayClient: gatewayClient)
                }
                state.inFlightOpens += 1
                return .run { [gatewayClient, screenCols] send in
                    let cols = await screenCols.columns()
                    do {
                        let key = try await gatewayClient.openSession(storedId: storedId, cols: Int64(cols))
                        await send(.existingSessionOpened(key: key))
                    } catch {
                        await send(.sessionOpenFailed(errorText: ErrorMessages.of(error)))
                    }
                }

            case let .openBotChat(profile):
                if let openKey = openTabForProfile(sessions: state.sessions, order: state.tabs.keys, profile: profile) {
                    return Self.performSwitchTab(key: openKey, state: &state, gatewayClient: gatewayClient)
                }
                state.inFlightOpens += 1
                return .run { [gatewayClient, screenCols] send in
                    let cols = await screenCols.columns()
                    do {
                        let key = try await gatewayClient.openBotChat(profile: profile, cols: Int64(cols))
                        await send(.botChatOpened(key: key, profile: profile))
                    } catch {
                        await send(.sessionOpenFailed(errorText: ErrorMessages.of(error)))
                    }
                }

            case let .switchTab(key):
                return Self.performSwitchTab(key: key, state: &state, gatewayClient: gatewayClient)

            case let .closeTab(key):
                // A tap on an × that is no longer on screen: closing a key the core never gave us
                // would answer with an empty strip and mint a second replacement.
                guard state.tabs.keys.contains(key) else { return .none }
                return .run { [gatewayClient] send in
                    do {
                        try await gatewayClient.closeSession(key: key)
                        await send(.tabClosed(key: key))
                    } catch {
                        await send(.tabCloseFailed(errorText: ErrorMessages.of(error)))
                    }
                }

            case let .setSessionTitle(title):
                guard let key = state.currentKey else {
                    state.errorText = "No open session to rename"
                    return .none
                }
                return .run { [gatewayClient] send in
                    do {
                        try await gatewayClient.setSessionTitle(key: key, title: title)
                        await send(.sessionTitleSet(key: key))
                    } catch {
                        await send(.sessionTitleFailed(errorText: ErrorMessages.of(error)))
                    }
                }

            case let .newSessionOpened(key):
                Self.registerOpenedSession(key: key, state: &state)
                state.currentKey = key
                return Self.sessionReadyEffect(key: key, gatewayClient: gatewayClient)

            case let .existingSessionOpened(key):
                Self.registerOpenedSession(key: key, state: &state)
                state.currentKey = key
                return Self.sessionReadyEffect(key: key, gatewayClient: gatewayClient)

            case let .botChatOpened(key, profile):
                Self.registerOpenedSession(key: key, state: &state)
                state.sessions = withProfile(sessions: state.sessions, key: key, profile: profile)
                state.currentKey = key
                return Self.sessionReadyEffect(key: key, gatewayClient: gatewayClient)

            case let .sessionOpenFailed(errorText):
                state.errorText = errorText
                Self.finishInFlightOpen(state: &state)
                return .none

            case let .sessionReady(key, summary):
                Self.finishAfterOpen(key: key, summary: summary, state: &state)
                return .none

            case let .tabClosed(key):
                state.tabs = state.tabs.close(key)
                state.sessions.removeValue(forKey: key)
                if state.currentKey == key {
                    // Until the replacement registers, nothing is live: leaving the closed key here
                    // makes `hasLiveSession` true and a re-login would overlay a dead transcript.
                    state.currentKey = state.tabs.current
                }
                if let current = state.tabs.current, current != state.currentKey {
                    state.currentKey = current
                    return Self.sessionReadyEffect(key: current, gatewayClient: gatewayClient)
                }
                if state.tabs.keys.isEmpty {
                    return .send(.openNewSession)
                }
                return .none

            case let .tabCloseFailed(errorText):
                state.errorText = errorText
                return .none

            case let .sessionTitleSet(key):
                state.errorText = ""
                return Self.refreshHeaderEffect(key: key, gatewayClient: gatewayClient)

            case let .sessionTitleFailed(errorText):
                state.errorText = errorText
                return .none

            case .chat(.delegate(let delegate)):
                switch delegate {
                case let .reportError(text):
                    state.errorText = text
                    return .none
                case .openSessionPicker:
                    return .send(.sessionPicker(.open(hasEndpoint: !state.endpointText.isEmpty)))
                }

            case .chat:
                return .none

            case .sessionPicker(.delegate(let delegate)):
                state.sessionPicker.isPresented = false
                switch delegate {
                case let .rowTapped(storedId):
                    return .send(.openExistingSession(storedId: storedId))
                case .newChatTapped:
                    return .send(.openNewSession)
                }

            case .sessionPicker:
                return .none

            case .botDrawer(.delegate(let delegate)):
                switch delegate {
                case let .openProfile(profile):
                    return Self.openBotChatFromDrawer(
                        profile: profile, state: &state, gatewayClient: gatewayClient, screenCols: screenCols
                    )
                }

            case .botDrawer:
                return .none

            case .appearance:
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
            state.setPhase(
                PhaseMachine.reduce(
                    phase: state.phase, event: .needsPassword, savedEndpoint: state.endpointText,
                    hasLiveSession: state.currentKey != nil
                )
            )
        }
    }

    private static let parkedChangesMax = 512

    private static func kindOf(_ kind: TranscriptChangeKind) -> String {
        switch kind {
        case .rowAppended: return "rowAppended"
        case .rowUpdated: return "rowUpdated"
        case .reset: return "reset"
        case .headerUpdated: return "headerUpdated"
        }
    }

    /// The predicate for "the session map may grow for this key": an open tab, or a key whose open is still registering.
    private static func isOpenOrRestoring(_ key: String, tabs: TabSet, pendingKeys: Set<String>) -> Bool {
        tabs.keys.contains(key) || pendingKeys.contains(key)
    }

    private static func onTranscriptChange(
        _ change: TranscriptChangeDto, state: inout State, gatewayClient: GatewayClient
    ) -> Effect<Action> {
        guard Self.isOpenOrRestoring(change.key, tabs: state.tabs, pendingKeys: state.pendingKeys) else {
            // An in-flight open_session can deliver a reset plus rows for the session being opened before its key is known, so those changes are parked rather than applied; with no open running an unknown key is dropped, never minted.
            if state.inFlightOpens > 0 {
                while state.parkedChanges.count >= Self.parkedChangesMax {
                    state.parkedChanges.removeFirst()
                }
                state.parkedChanges.append(change)
            }
            return .none
        }
        state.sessions = applySessionChange(
            sessions: state.sessions,
            key: change.key,
            kind: Self.kindOf(change.kind),
            index: Int64(change.index),
            rowJson: change.rowJson,
            knownKeys: Set(state.tabs.keys).union(state.pendingKeys),
            title: change.title
        )
        guard change.kind == .headerUpdated else { return .none }
        return .run { [gatewayClient] send in
            let summaries = await gatewayClient.openSessions()
            guard let summary = summaries.first(where: { $0.key == change.key }) else { return }
            await send(.headerRefreshed(key: change.key, summary: summary))
        }
    }

    /// Re-checked behind the same predicate as the map write above: the RPC this follows is async, so the tab it targets may have closed while it was in flight.
    private static func applyHeaderRefresh(key: String, summary: SessionSummary, state: inout State) {
        guard Self.isOpenOrRestoring(key, tabs: state.tabs, pendingKeys: state.pendingKeys) else { return }
        var current = state.sessions[key] ?? SessionUiState(key: key)
        current.title = summary.title
        current.model = LooseJSON(summary.headerJson)?.optString("model") ?? ""
        current.running = summary.running
        current.profile = summary.profileName
        state.sessions[key] = current
        guard !current.model.isEmpty else { return }
        state.setPhase(
            PhaseMachine.reduce(phase: state.phase, event: .header(model: current.model), savedEndpoint: state.endpointText)
        )
    }

    /// Reads the just-registered tab's cached header model; `""` before any header has landed, which also covers the connection half's main-session flow, where `currentKey` is never set.
    private static func afterOpen(state: inout State) {
        state.errorText = ""
        state.setPhase(PhaseMachine.ready(state.currentKey.flatMap { state.sessions[$0]?.model } ?? ""))
    }

    private static func performSwitchTab(key: String, state: inout State, gatewayClient: GatewayClient) -> Effect<Action> {
        guard let next = tabTap(tabs: state.tabs, currentKey: state.currentKey, key: key) else { return .none }
        state.tabs = next
        guard let current = next.current else { return .none }
        state.currentKey = current
        return Self.sessionReadyEffect(key: current, gatewayClient: gatewayClient)
    }

    /// The drawer's counterpart to the plain `openBotChat` action: same open-or-switch logic, plus
    /// `botChatOpened` / `botChatOpenFailed` back to the drawer on every path so `botOpenInFlight`
    /// always clears, the way the Kotlin `finally` does.
    private static func openBotChatFromDrawer(
        profile: String, state: inout State, gatewayClient: GatewayClient, screenCols: ScreenCols
    ) -> Effect<Action> {
        if let openKey = openTabForProfile(sessions: state.sessions, order: state.tabs.keys, profile: profile) {
            return .merge(
                Self.performSwitchTab(key: openKey, state: &state, gatewayClient: gatewayClient),
                .send(.botDrawer(.botChatOpened))
            )
        }
        state.inFlightOpens += 1
        return .run { [gatewayClient, screenCols] send in
            let cols = await screenCols.columns()
            do {
                let key = try await gatewayClient.openBotChat(profile: profile, cols: Int64(cols))
                await send(.botChatOpened(key: key, profile: profile))
                await send(.botDrawer(.botChatOpened))
            } catch {
                await send(.sessionOpenFailed(errorText: ErrorMessages.of(error)))
                await send(.botDrawer(.botChatOpenFailed))
            }
        }
    }

    private static func sessionReadyEffect(key: String, gatewayClient: GatewayClient) -> Effect<Action> {
        .run { [gatewayClient] send in
            let summaries = await gatewayClient.openSessions()
            let summary = summaries.first(where: { $0.key == key })
            await send(.sessionReady(key: key, summary: summary))
        }
    }

    private static func refreshHeaderEffect(key: String, gatewayClient: GatewayClient) -> Effect<Action> {
        .run { [gatewayClient] send in
            let summaries = await gatewayClient.openSessions()
            guard let summary = summaries.first(where: { $0.key == key }) else { return }
            await send(.headerRefreshed(key: key, summary: summary))
        }
    }

    /// Writes the fresh summary into the session map (guarded the same way `applyHeaderRefresh` is, since this also follows an async RPC), then forces the phase to Ready the way `afterOpen` always does, whether or not a header landed.
    private static func finishAfterOpen(key: String, summary: SessionSummary?, state: inout State) {
        if Self.isOpenOrRestoring(key, tabs: state.tabs, pendingKeys: state.pendingKeys), let summary {
            var current = state.sessions[key] ?? SessionUiState(key: key)
            current.title = summary.title
            current.model = LooseJSON(summary.headerJson)?.optString("model") ?? ""
            current.running = summary.running
            current.profile = summary.profileName
            state.sessions[key] = current
        }
        Self.afterOpen(state: &state)
    }

    /// Seeds title, profile and running from an `open_sessions()` snapshot; the header refresh only ever covers the current tab.
    private static func applySummaries(_ summaries: [SessionSummary], state: inout State) {
        guard !summaries.isEmpty else { return }
        for summary in summaries {
            guard var existing = state.sessions[summary.key] else { continue }
            existing.title = summary.title
            existing.profile = summary.profileName
            existing.running = summary.running
            state.sessions[summary.key] = existing
        }
    }

    /// The shared tail of `openNewSession` / `openExistingSession` / `openBotChat`: register the newly opened key as a tab, draining any changes the change stream parked for it while its open was in flight.
    private static func registerOpenedSession(key: String, state: inout State) {
        state.pendingKeys.insert(key)
        Self.drainParked(key: key, state: &state)
        state.sessions = ensureSession(state.sessions, key)
        state.tabs = state.tabs.add(key)
        state.pendingKeys.remove(key)
        Self.finishInFlightOpen(state: &state)
    }

    /// Everything the core just invalidated: the tabs, their transcripts, the in-flight bookkeeping.
    private static func clearSessionState(_ state: inout State) {
        state.sessions = [:]
        state.tabs = TabSet()
        state.currentKey = nil
        state.pendingKeys = []
        state.parkedChanges = []
        state.inFlightOpens = 0
    }

    private static func finishInFlightOpen(state: inout State) {
        if state.inFlightOpens > 0 {
            state.inFlightOpens -= 1
        }
        if state.inFlightOpens == 0 {
            state.parkedChanges = []
        }
    }

    /// Re-applies the changes parked for `key` while its open was in flight; a change parked for a different, still-overlapping open is put back so that open can drain it in turn.
    private static func drainParked(key: String, state: inout State) {
        guard !state.parkedChanges.isEmpty else { return }
        let known = Set(state.tabs.keys).union(state.pendingKeys).union([key])
        var sessions = state.sessions
        var remaining: [TranscriptChangeDto] = []
        for change in state.parkedChanges {
            guard change.key == key else {
                remaining.append(change)
                continue
            }
            sessions = applySessionChange(
                sessions: sessions,
                key: key,
                kind: Self.kindOf(change.kind),
                index: Int64(change.index),
                rowJson: change.rowJson,
                knownKeys: known,
                title: change.title
            )
        }
        state.parkedChanges = remaining
        state.sessions = sessions
    }

    private static func connectThenOpenMainSession(
        cols: Int, gatewayClient: GatewayClient, send: Send<Action>
    ) async {
        do {
            try await gatewayClient.connect()
        } catch {
            await send(connectFailure(error))
            return
        }
        await openMainSession(cols: cols, gatewayClient: gatewayClient, send: send)
    }

    private static func loginConnectThenOpenMainSession(
        password: String, cols: Int, gatewayClient: GatewayClient, send: Send<Action>
    ) async {
        do {
            try await gatewayClient.login(password: password)
            try await gatewayClient.connect()
        } catch {
            await send(connectFailure(error))
            return
        }
        await openMainSession(cols: cols, gatewayClient: gatewayClient, send: send)
    }

    private static func connectFailure(_ error: Error) -> Action {
        .connectFailed(
            errorText: ErrorMessages.of(error),
            isAuthShape: ErrorMessages.isAuthShape(error),
            closedReason: "error: \(ErrorMessages.rustMessage(error))"
        )
    }

    /// `GatewayClient.openSessions()` cannot fail on this platform (the core call has no `Result`), so the registry-unreachable branch Android guards against never fires through this dependency.
    ///
    /// The resume keys are announced BEFORE the first open so the change stream can park the
    /// reset and rows the core pushes for a session whose tab does not exist yet.
    private static func openMainSession(
        cols: Int, gatewayClient: GatewayClient, send: Send<Action>
    ) async {
        let summaries = await gatewayClient.openSessions()
        let lastActive = await gatewayClient.lastActiveSession()
        let plan = restorePlan(keys: summaries.map(\.key), lastActive: lastActive)
        await send(.resumePlanned(keys: plan.resumeKeys))
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
            await send(
                .mainSessionRestored(
                    planned: plan.resumeKeys, resumed: resumed, summaries: summaries, current: plan.current
                )
            )
            return
        }
        if !plan.resumeKeys.isEmpty {
            await send(.mainSessionRestoreFailed(planned: plan.resumeKeys, errorText: lastErrorText))
            return
        }
        do {
            let key = try await gatewayClient.openSession(storedId: nil, cols: Int64(cols))
            await send(.newSessionOpened(key: key))
        } catch {
            await send(.mainSessionRestoreFailed(planned: [], errorText: ErrorMessages.of(error)))
        }
    }
}
