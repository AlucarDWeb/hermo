import ComposableArchitecture
import HermesCore
import HermoFeatures
import HermoGateway
import HermoLogic
import XCTest

private let testEndpoint = EndpointDto(
    baseUrl: "https://gw.example.com", username: "fer", displayName: "Home Gateway"
)
private let testDisplayText = "Home Gateway (fer) — https://gw.example.com"

@MainActor
final class AppFeatureTests: XCTestCase {

    // MARK: 1. Pairing to password to connecting to ready

    func testPairThenLoginConnectReachesReadyThroughConnecting() async {
        let store = TestStore(initialState: AppFeature.State()) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
            $0.gatewayClient.pair = { _ in testEndpoint }
            $0.gatewayClient.login = { _ in }
            $0.gatewayClient.connect = {}
            $0.gatewayClient.openSessions = { [] }
            $0.gatewayClient.lastActiveSession = { nil }
            $0.gatewayClient.openSession = { _, _ in "session-1" }
        }

        await store.send(.pair("hermes://connect?payload"))
        await store.receive(.paired(endpointText: testDisplayText)) {
            $0.endpointText = testDisplayText
            $0.phase = .needsPassword(endpoint: testDisplayText, overlay: false)
        }

        await store.send(.loginAndConnect("secret")) {
            $0.phase = .connecting
        }
        await store.receive(.resumePlanned(keys: [])) {
            $0.inFlightOpens = 1
        }
        await store.receive(.newSessionOpened(key: "session-1")) {
            $0.sessions = ["session-1": SessionUiState(key: "session-1")]
            $0.tabs = TabSet(keys: ["session-1"], current: "session-1")
            $0.currentKey = "session-1"
            $0.inFlightOpens = 0
        }
        await store.receive(.sessionReady(key: "session-1", summary: nil)) {
            $0.phase = .ready(model: "")
        }
    }

    // MARK: 2. SessionExpired after a session is already live

    func testSessionExpiredAfterALiveSessionReturnsToPasswordPrompt() async {
        let store = TestStore(
            initialState: AppFeature.State(
                phase: .ready(model: "gpt-4"),
                endpointText: testDisplayText,
                lastReadyModel: "gpt-4"
            )
        ) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
            $0.gatewayClient.savedEndpoint = { testEndpoint }
            $0.gatewayClient.connect = { throw CoreError.SessionExpired }
        }

        await store.send(.tryResume)
        await store.receive(.resumeStarted(endpointText: testDisplayText)) {
            $0.phase = .connecting
        }
        await store.receive(
            .connectFailed(
                errorText: "Session expired — enter the password again.",
                isAuthShape: true,
                closedReason: "error: session expired — re-login required"
            )
        ) {
            $0.errorText = "Session expired — enter the password again."
            // `hasLiveSession` is hardcoded false in this half (T09 wires the real
            // check off `currentKey`), so the overlay flag never flips to true here.
            $0.phase = .needsPassword(endpoint: testDisplayText, overlay: false)
        }
    }

    // MARK: 3. Every registry resume fails

    func testAllRegistryResumesFailingGoesOffline() async {
        let store = TestStore(
            initialState: AppFeature.State(
                phase: .needsPassword(endpoint: testDisplayText),
                endpointText: testDisplayText
            )
        ) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
            $0.gatewayClient.login = { _ in }
            $0.gatewayClient.connect = {}
            $0.gatewayClient.openSessions = {
                [SessionSummary(key: "s1", title: "Session", running: true, headerJson: "{}", profileName: "")]
            }
            $0.gatewayClient.lastActiveSession = { "s1" }
            $0.gatewayClient.openSession = { _, _ in throw CoreError.NotConnected }
        }

        await store.send(.loginAndConnect("secret")) {
            $0.phase = .connecting
        }
        await store.receive(.resumePlanned(keys: ["s1"])) {
            $0.pendingKeys = ["s1"]
            $0.inFlightOpens = 1
        }
        await store.receive(
            .mainSessionRestoreFailed(planned: ["s1"], errorText: "Not connected to the gateway yet.")
        ) {
            $0.pendingKeys = []
            $0.inFlightOpens = 0
            $0.errorText = "Not connected to the gateway yet."
            $0.phase = .offline(reason: "error: no session could be resumed")
        }
    }

    // MARK: 4. forgetGateway resets to Unpaired

    func testForgetGatewayResetsPhaseAndEndpoint() async {
        let store = TestStore(
            initialState: AppFeature.State(
                phase: .ready(model: "gpt-4"),
                endpointText: testDisplayText,
                lastReadyModel: "gpt-4",
                errorText: "stale error"
            )
        ) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
            $0.gatewayClient.forgetGateway = {}
        }

        await store.send(.forgetGateway)
        await store.receive(.forgotGateway) {
            $0.errorText = ""
            $0.endpointText = ""
            $0.phase = .unpaired
        }
    }

    // MARK: Other branches the reducer needs covered

    func testPairFailureSurfacesErrorTextOnly() async {
        let store = TestStore(initialState: AppFeature.State()) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
            $0.gatewayClient.pair = { _ in throw CoreError.InvalidQr }
        }

        await store.send(.pair("garbage"))
        await store.receive(.pairFailed(errorText: "That pairing payload is not valid.")) {
            $0.errorText = "That pairing payload is not valid."
        }
    }

    func testNetworkAvailableRetriesFromOffline() async {
        let store = TestStore(
            initialState: AppFeature.State(phase: .offline(reason: "error: no session could be resumed"))
        ) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
            $0.gatewayClient.savedEndpoint = { nil }
        }

        await store.send(.networkAvailable)
        await store.receive(.tryResume)
    }

    func testNetworkAvailableDoesNothingWhenNotOffline() async {
        let store = TestStore(initialState: AppFeature.State(phase: .ready(model: "gpt-4"))) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
        }

        await store.send(.networkAvailable)
    }

    func testNetworkMonitorYieldWhileOfflineRetriesResume() async {
        let (stream, continuation) = AsyncStream<Void>.makeStream()
        let store = TestStore(
            initialState: AppFeature.State(phase: .offline(reason: "error: no session could be resumed"))
        ) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
            $0.gatewayClient.events = { AsyncStream { $0.finish() } }
            $0.networkMonitor.events = { stream }
            $0.gatewayClient.savedEndpoint = { nil }
        }

        await store.send(.task)
        continuation.yield()
        await store.receive(.networkAvailable)
        await store.receive(.tryResume)

        continuation.finish()
        await store.finish()
    }

    func testNetworkMonitorYieldWhileReadyDoesNothing() async {
        let (stream, continuation) = AsyncStream<Void>.makeStream()
        let store = TestStore(initialState: AppFeature.State(phase: .ready(model: "gpt-4"))) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
            $0.gatewayClient.events = { AsyncStream { $0.finish() } }
            $0.networkMonitor.events = { stream }
        }

        await store.send(.task)
        continuation.yield()
        await store.receive(.networkAvailable)

        continuation.finish()
        await store.finish()
    }

    func testCameraPermissionDeniedSetsErrorText() async {
        let store = TestStore(initialState: AppFeature.State()) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
        }

        await store.send(.cameraPermissionDenied) {
            $0.errorText = "Camera permission is needed to scan the QR — or paste the payload below"
        }
    }

    func testPairingPayloadChangedUpdatesState() async {
        let store = TestStore(initialState: AppFeature.State()) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
        }

        await store.send(.pairingPayloadChanged("hermes://connect?payload")) {
            $0.pairingPayload = "hermes://connect?payload"
        }
    }

    // MARK: 5. Unknown-key change parked during an in-flight open, then applied

    func testUnknownKeyChangeDuringInFlightOpenIsParkedThenApplied() async {
        let change = TranscriptChangeDto(key: "new-key", kind: .rowAppended, index: 0, rowJson: "{\"row\":1}", title: "")
        let store = TestStore(initialState: AppFeature.State(inFlightOpens: 1)) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
            $0.gatewayClient.openSessions = { [] }
        }

        await store.send(.core(.transcript(change))) {
            $0.parkedChanges = [change]
        }

        await store.send(.newSessionOpened(key: "new-key")) {
            $0.sessions = ["new-key": SessionUiState(key: "new-key", rows: ["{\"row\":1}"])]
            $0.tabs = TabSet(keys: ["new-key"], current: "new-key")
            $0.currentKey = "new-key"
            $0.inFlightOpens = 0
            $0.parkedChanges = []
        }

        await store.receive(.sessionReady(key: "new-key", summary: nil)) {
            $0.phase = .ready(model: "")
        }
    }

    // MARK: 6. Unknown-key change with no open in flight is dropped

    func testUnknownKeyChangeWithNoOpenInFlightIsDropped() async {
        let change = TranscriptChangeDto(key: "unknown", kind: .rowAppended, index: 0, rowJson: "{\"row\":1}", title: "")
        let store = TestStore(initialState: AppFeature.State()) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
        }

        await store.send(.core(.transcript(change)))
    }

    // MARK: 7. headerUpdated change updates the session title

    func testHeaderUpdatedChangeUpdatesSessionTitle() async {
        let change = TranscriptChangeDto(
            key: "s1", kind: .headerUpdated, index: UInt32.max, rowJson: "", title: "New title"
        )
        let store = TestStore(
            initialState: AppFeature.State(
                sessions: ["s1": SessionUiState(key: "s1", title: "Old title")],
                tabs: TabSet(keys: ["s1"], current: "s1"),
                currentKey: "s1"
            )
        ) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
            $0.gatewayClient.openSessions = { [] }
        }

        await store.send(.core(.transcript(change))) {
            $0.sessions["s1"]?.title = "New title"
        }
    }

    // MARK: 8. Closing the last tab mints a new session

    func testClosingLastTabMintsANewSession() async {
        let store = TestStore(
            initialState: AppFeature.State(
                sessions: ["s1": SessionUiState(key: "s1", title: "Chat")],
                tabs: TabSet(keys: ["s1"], current: "s1"),
                currentKey: "s1"
            )
        ) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
            $0.gatewayClient.closeSession = { _ in }
            $0.gatewayClient.openSession = { _, _ in "s2" }
            $0.gatewayClient.openSessions = { [] }
        }

        await store.send(.closeTab(key: "s1"))
        await store.receive(.tabClosed(key: "s1")) {
            $0.tabs = TabSet()
            $0.sessions = [:]
            $0.currentKey = nil
        }
        await store.receive(.openNewSession) {
            $0.inFlightOpens = 1
        }
        await store.receive(.newSessionOpened(key: "s2")) {
            $0.sessions = ["s2": SessionUiState(key: "s2")]
            $0.tabs = TabSet(keys: ["s2"], current: "s2")
            $0.currentKey = "s2"
            $0.inFlightOpens = 0
        }
        await store.receive(.sessionReady(key: "s2", summary: nil)) {
            $0.phase = .ready(model: "")
        }
    }

    // MARK: 9. Drawer tap for an already-open profile switches tabs, never opens a bot chat

    func testBotChatTapForExistingProfileSwitchesTabWithoutCallingBotChatVerb() async {
        let store = TestStore(
            initialState: AppFeature.State(
                sessions: [
                    "s1": SessionUiState(key: "s1", profile: "echo"),
                    "s2": SessionUiState(key: "s2", profile: "other"),
                ],
                tabs: TabSet(keys: ["s1", "s2"], current: "s2"),
                currentKey: "s2"
            )
        ) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
            $0.gatewayClient.openSessions = { [] }
        }

        await store.send(.openBotChat(profile: "echo")) {
            $0.tabs = TabSet(keys: ["s1", "s2"], current: "s1")
            $0.currentKey = "s1"
        }
        await store.receive(.sessionReady(key: "s1", summary: nil)) {
            $0.phase = .ready(model: "")
        }
    }

    // MARK: 10. SessionExpired with a session already open shows the re-login overlay

    func testSessionExpiredWithSessionOpenShowsOverlay() async {
        let store = TestStore(
            initialState: AppFeature.State(
                phase: .ready(model: "gpt-4"),
                endpointText: testDisplayText,
                lastReadyModel: "gpt-4",
                sessions: ["s1": SessionUiState(key: "s1")],
                tabs: TabSet(keys: ["s1"], current: "s1"),
                currentKey: "s1"
            )
        ) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
            $0.gatewayClient.savedEndpoint = { testEndpoint }
            $0.gatewayClient.connect = { throw CoreError.SessionExpired }
        }

        await store.send(.tryResume)
        await store.receive(.resumeStarted(endpointText: testDisplayText)) {
            $0.phase = .connecting
        }
        await store.receive(
            .connectFailed(
                errorText: "Session expired — enter the password again.",
                isAuthShape: true,
                closedReason: "error: session expired — re-login required"
            )
        ) {
            $0.errorText = "Session expired — enter the password again."
            $0.phase = .needsPassword(endpoint: testDisplayText, overlay: true)
        }
    }

    // MARK: 11. Changes parked for a different key are re-queued, not discarded

    func testParkedChangesForADifferentKeyAreRequeuedNotDiscarded() async {
        let changeA = TranscriptChangeDto(key: "keyA", kind: .rowAppended, index: 0, rowJson: "{\"a\":1}", title: "")
        let changeB = TranscriptChangeDto(key: "keyB", kind: .rowAppended, index: 0, rowJson: "{\"b\":1}", title: "")
        let store = TestStore(initialState: AppFeature.State(inFlightOpens: 2)) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
            $0.gatewayClient.openSessions = { [] }
        }

        await store.send(.core(.transcript(changeA))) {
            $0.parkedChanges = [changeA]
        }
        await store.send(.core(.transcript(changeB))) {
            $0.parkedChanges = [changeA, changeB]
        }

        await store.send(.existingSessionOpened(key: "keyA")) {
            $0.sessions = ["keyA": SessionUiState(key: "keyA", rows: ["{\"a\":1}"])]
            $0.tabs = TabSet(keys: ["keyA"], current: "keyA")
            $0.currentKey = "keyA"
            $0.inFlightOpens = 1
            $0.parkedChanges = [changeB]
        }
        await store.receive(.sessionReady(key: "keyA", summary: nil)) {
            $0.phase = .ready(model: "")
        }

        await store.send(.existingSessionOpened(key: "keyB")) {
            $0.sessions = [
                "keyA": SessionUiState(key: "keyA", rows: ["{\"a\":1}"]),
                "keyB": SessionUiState(key: "keyB", rows: ["{\"b\":1}"]),
            ]
            $0.tabs = TabSet(keys: ["keyA", "keyB"], current: "keyB")
            $0.currentKey = "keyB"
            $0.inFlightOpens = 0
            $0.parkedChanges = []
        }
        await store.receive(.sessionReady(key: "keyB", summary: nil))
    }

    // MARK: 12. A cold-start restore registers every resumed key as a tab

    func testResumeRegistersTabsAndSeedsThemFromTheRegistrySnapshot() async {
        let summaries = [
            SessionSummary(key: "s1", title: "First", running: false, headerJson: "{}", profileName: "bot"),
            SessionSummary(key: "s2", title: "Second", running: true, headerJson: "{}", profileName: ""),
        ]
        let store = TestStore(initialState: AppFeature.State()) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
            $0.gatewayClient.savedEndpoint = { testEndpoint }
            $0.gatewayClient.connect = {}
            $0.gatewayClient.openSessions = { summaries }
            $0.gatewayClient.lastActiveSession = { "s1" }
            $0.gatewayClient.openSession = { storedId, _ in storedId ?? "minted" }
        }

        await store.send(.tryResume)
        await store.receive(.resumeStarted(endpointText: testDisplayText)) {
            $0.endpointText = testDisplayText
            $0.phase = .connecting
        }
        await store.receive(.resumePlanned(keys: ["s1", "s2"])) {
            $0.pendingKeys = ["s1", "s2"]
            $0.inFlightOpens = 1
        }
        await store.receive(
            .mainSessionRestored(planned: ["s1", "s2"], resumed: ["s1", "s2"], summaries: summaries, current: "s1")
        ) {
            $0.pendingKeys = []
            $0.inFlightOpens = 0
            $0.sessions = [
                "s1": SessionUiState(key: "s1", title: "First", running: false, profile: "bot"),
                "s2": SessionUiState(key: "s2", title: "Second", running: true, profile: ""),
            ]
            $0.tabs = TabSet(keys: ["s1", "s2"], current: "s1")
            $0.currentKey = "s1"
            $0.phase = .ready(model: "")
        }
    }

    // MARK: 13. The composed ChatFeature's reportError delegate surfaces on the parent errorText

    func testChatDelegateReportErrorSurfacesOnAppErrorText() async {
        let store = TestStore(initialState: AppFeature.State()) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
        }

        await store.send(.chat(.delegate(.reportError("boom")))) {
            $0.errorText = "boom"
        }
    }

    // MARK: 14. Sending through the composed child reaches the gateway client

    func testSendingThroughComposedChatReachesTheClient() async {
        let store = TestStore(
            initialState: AppFeature.State(chat: ChatFeature.State(draft: "hello"))
        ) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
            $0.gatewayClient.send = { _, _ in throw CoreError.NotConnected }
        }

        await store.send(.chat(.send(key: "s1"))) {
            $0.chat.draft = ""
        }
        await store.receive(.chat(.delegate(.reportError("Not connected to the gateway yet.")))) {
            $0.errorText = "Not connected to the gateway yet."
        }
    }

    // MARK: 15. A picker row tap opens that session and dismisses the sheet

    func testSessionPickerRowTappedOpensThatSessionAndDismisses() async {
        let store = TestStore(
            initialState: AppFeature.State(sessionPicker: SessionPickerFeature.State(isPresented: true))
        ) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
            $0.gatewayClient.openSession = { _, _ in "s1" }
            $0.gatewayClient.openSessions = { [] }
        }

        await store.send(.sessionPicker(.delegate(.rowTapped(storedId: "s1")))) {
            $0.sessionPicker.isPresented = false
        }
        await store.receive(.openExistingSession(storedId: "s1")) {
            $0.inFlightOpens = 1
        }
        await store.receive(.existingSessionOpened(key: "s1")) {
            $0.sessions = ["s1": SessionUiState(key: "s1")]
            $0.tabs = TabSet(keys: ["s1"], current: "s1")
            $0.currentKey = "s1"
            $0.inFlightOpens = 0
        }
        await store.receive(.sessionReady(key: "s1", summary: nil)) {
            $0.phase = .ready(model: "")
        }
    }

    // MARK: 16. A failed drawer profile open clears botOpenInFlight so a second tap is accepted

    func testDrawerProfileOpenFailureClearsBotOpenInFlightForASecondTap() async {
        let store = TestStore(
            initialState: AppFeature.State(botDrawer: BotDrawerFeature.State(botOpenInFlight: true))
        ) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
            $0.gatewayClient.openBotChat = { _, _ in throw CoreError.NotConnected }
        }

        await store.send(.botDrawer(.delegate(.openProfile("echo")))) {
            $0.inFlightOpens = 1
        }
        await store.receive(.sessionOpenFailed(errorText: "Not connected to the gateway yet.")) {
            $0.errorText = "Not connected to the gateway yet."
            $0.inFlightOpens = 0
        }
        await store.receive(.botDrawer(.botChatOpenFailed)) {
            $0.botDrawer.botOpenInFlight = false
        }

        // The flag is clear, so a second tap is accepted rather than dropped.
        await store.send(.botDrawer(.profileTapped(profile: "echo"))) {
            $0.botDrawer.botOpenInFlight = true
        }
        await store.receive(.botDrawer(.delegate(.openProfile("echo")))) {
            $0.inFlightOpens = 1
        }
        await store.receive(.sessionOpenFailed(errorText: "Not connected to the gateway yet.")) {
            $0.errorText = "Not connected to the gateway yet."
            $0.inFlightOpens = 0
        }
        await store.receive(.botDrawer(.botChatOpenFailed)) {
            $0.botDrawer.botOpenInFlight = false
        }
    }

    // MARK: 18. Forgetting the gateway drops the drawer rows the next one would inherit

    func testForgetGatewayClearsTheDrawerAndPickerRows() async {
        let rows = [botDrawerRow(name: "echo", model: "m", description: "")]
        let store = TestStore(
            initialState: AppFeature.State(
                phase: .ready(model: "gpt-4"),
                endpointText: testDisplayText,
                sessionPicker: SessionPickerFeature.State(
                    sessions: [RemoteSessionRow(id: "s1", title: "Old", preview: "", messageCount: 1)]
                ),
                botDrawer: BotDrawerFeature.State(drawerState: .ready(rows), isOpen: true)
            )
        ) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
            $0.gatewayClient.forgetGateway = {}
        }

        await store.send(.forgetGateway)
        await store.receive(.forgotGateway) {
            $0.endpointText = ""
            $0.phase = .unpaired
            $0.sessionPicker = SessionPickerFeature.State()
            $0.botDrawer = BotDrawerFeature.State()
        }
    }

    // MARK: 17. The chat's openSessionPicker delegate opens the picker

    func testChatOpenSessionPickerDelegateOpensThePicker() async {
        let store = TestStore(
            initialState: AppFeature.State(endpointText: testDisplayText)
        ) {
            AppFeature()
        } withDependencies: {
            $0.screenCols = ScreenCols(columns: { 80 })
            $0.gatewayClient.listRemoteSessions = { [] }
        }

        await store.send(.chat(.delegate(.openSessionPicker)))
        await store.receive(.sessionPicker(.open(hasEndpoint: true))) {
            $0.sessionPicker.isPresented = true
            $0.sessionPicker.isLoading = true
        }
        await store.receive(.sessionPicker(.sessionsLoaded([]))) {
            $0.sessionPicker.isLoading = false
        }
    }
}
