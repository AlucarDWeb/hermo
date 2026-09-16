import XCTest
import HermoLogic

final class PhaseMachineTests: XCTestCase {

    func testPairingToReadyThenOfflineThenReloginFollowsThePlannedSequence() {
        let ep = "http://192.168.1.48:9123 (hermo)"
        var p: AppPhase = .unpaired

        p = PhaseMachine.reduce(phase: p, event: .needsPassword, savedEndpoint: ep)
        XCTAssertEqual(p, .needsPassword(endpoint: ep))

        p = PhaseMachine.reduce(phase: p, event: .connecting, savedEndpoint: ep)
        XCTAssertEqual(p, .connecting)

        p = PhaseMachine.reduce(phase: p, event: .open, savedEndpoint: ep)
        XCTAssertEqual(p, .connecting)
        p = PhaseMachine.ready("claude-sonnet-4-5")
        XCTAssertEqual(p, .ready(model: "claude-sonnet-4-5"))

        p = PhaseMachine.reduce(phase: p, event: .closed(reason: "timeout"), savedEndpoint: ep)
        XCTAssertEqual(p, .offline(reason: "timeout"))

        p = PhaseMachine.reduce(phase: p, event: .connecting, savedEndpoint: ep)
        XCTAssertEqual(p, .connecting)
        p = PhaseMachine.reduce(phase: p, event: .open, savedEndpoint: ep)
        p = PhaseMachine.ready("claude-sonnet-4-5")
        XCTAssertEqual(p, .ready(model: "claude-sonnet-4-5"))
    }

    func testSessionExpiryAsksForThePasswordKeepingEndpoint() {
        let ep = "hermo-lan"
        var p: AppPhase = PhaseMachine.ready("m")
        p = PhaseMachine.reduce(phase: p, event: .needsPassword, savedEndpoint: ep)
        XCTAssertEqual(p, .needsPassword(endpoint: ep))
    }

    func testDeliberateCloseWithNoEndpointReturnsToUnpaired() {
        var p: AppPhase = .needsPassword(endpoint: "x")
        p = PhaseMachine.reduce(phase: p, event: .closed(reason: "user logout"), savedEndpoint: "")
        XCTAssertEqual(p, .unpaired)
    }

    func testNeedsPasswordWithNoSavedEndpointCannotFakeAPhase() {
        var p: AppPhase = .unpaired
        p = PhaseMachine.reduce(phase: p, event: .needsPassword, savedEndpoint: "")
        XCTAssertEqual(p, .unpaired)
    }

    func testReadyIsStickyThroughAnOpenEvent() {
        let p = PhaseMachine.ready("m")
        XCTAssertEqual(p, PhaseMachine.reduce(phase: p, event: .open, savedEndpoint: "e"))
    }

    func testOfflineCarriesTheReasonText() {
        let p = PhaseMachine.offline("DNS failure")
        if case .offline(let reason) = p {
            XCTAssertEqual(reason, "DNS failure")
        } else {
            XCTFail("expected .offline")
        }
    }

    func testAHeaderLandingAfterReadyRefreshesTheModelName() {
        var p = PhaseMachine.ready("")
        p = PhaseMachine.reduce(phase: p, event: .header(model: "claude-sonnet-4-5"), savedEndpoint: "e")
        XCTAssertEqual(p, .ready(model: "claude-sonnet-4-5"))
    }

    func testAHeaderNeverWipesTheModelAndNeverLeavesReady() {
        let ready = PhaseMachine.ready("m")
        XCTAssertEqual(ready, PhaseMachine.reduce(phase: ready, event: .header(model: ""), savedEndpoint: "e"))
        let connecting: AppPhase = .connecting
        XCTAssertEqual(connecting, PhaseMachine.reduce(phase: connecting, event: .header(model: "m"), savedEndpoint: "e"))
    }

    func testAuthShapedFailureWithASavedEndpointAsksForThePassword() {
        let ep = "http://192.168.1.48:9123 (hermo)"
        var p: AppPhase = PhaseMachine.reduce(phase: .connecting, event: .authFailed, savedEndpoint: ep)
        XCTAssertEqual(p, .needsPassword(endpoint: ep))

        p = PhaseMachine.reduce(phase: PhaseMachine.ready("m"), event: .authFailed, savedEndpoint: ep, hasLiveSession: true)
        XCTAssertEqual(p, .needsPassword(endpoint: ep, overlay: true))
    }

    func testAuthFailureWithNoSavedEndpointStaysUnpaired() {
        let p = PhaseMachine.reduce(phase: .connecting, event: .authFailed, savedEndpoint: "")
        XCTAssertEqual(p, .unpaired)
    }

    func testGenuineTransportCloseStaysOffline() {
        let p = PhaseMachine.reduce(phase: PhaseMachine.ready("m"), event: .closed(reason: "connection reset"), savedEndpoint: "e")
        XCTAssertEqual(p, .offline(reason: "connection reset"))
    }

    func testPasswordAskAfterFirstPairHasNoOverlay() {
        let p = PhaseMachine.reduce(phase: .connecting, event: .authFailed, savedEndpoint: "ep", hasLiveSession: false)
        XCTAssertEqual(p, .needsPassword(endpoint: "ep", overlay: false))
    }

    func testSinkNeedsPasswordOverlaysWhenASessionIsLive() {
        let p = PhaseMachine.reduce(
            phase: PhaseMachine.ready("m"),
            event: .needsPassword,
            savedEndpoint: "ep",
            hasLiveSession: true
        )
        XCTAssertEqual(p, .needsPassword(endpoint: "ep", overlay: true))
    }
}
