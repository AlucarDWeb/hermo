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
        await store.receive(.mainSessionReady) {
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
        await store.receive(
            .mainSessionClosed(reason: "error: no session could be resumed", errorText: "Not connected to the gateway yet.")
        ) {
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
}
