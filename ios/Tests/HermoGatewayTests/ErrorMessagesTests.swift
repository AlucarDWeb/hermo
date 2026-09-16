import XCTest
import HermesCore
import HermoGateway

private struct StringError: LocalizedError {
    let errorDescription: String?
    init(_ message: String) { errorDescription = message }
}

private func err(_ message: String) -> Error { StringError(message) }

final class ErrorMessagesTests: XCTestCase {

    func testInvalidQr() {
        XCTAssertEqual("That pairing payload is not valid.", ErrorMessages.of(err("invalid QR payload")))
    }

    func testInvalidCredentials() {
        XCTAssertEqual("Wrong username or password.", ErrorMessages.of(err("invalid credentials")))
    }

    func testRateLimited() {
        XCTAssertEqual("Too many attempts — wait a moment and retry.", ErrorMessages.of(err("rate limited")))
    }

    func testSessionExpired() {
        XCTAssertEqual(
            "Session expired — enter the password again.",
            ErrorMessages.of(err("session expired — re-login required"))
        )
    }

    func testUpgradeRejected() {
        XCTAssertEqual(
            "The gateway rejected the connection (host/origin guard).",
            ErrorMessages.of(err("upgrade rejected by gateway"))
        )
    }

    func testNetwork() {
        XCTAssertEqual("Network error — check the connection.", ErrorMessages.of(err("network failure: boom")))
    }

    func testInvalidEndpoint() {
        XCTAssertEqual("That gateway URL is not valid.", ErrorMessages.of(err("invalid endpoint url: nope")))
    }

    func testUnknownErrorKeepsDetail() {
        XCTAssertEqual("Error: mystery", ErrorMessages.of(err("mystery")))
    }

    func testEmptyMessageSessionExpiredIsAuthShaped() {
        XCTAssertTrue(ErrorMessages.isAuthShape(CoreError.SessionExpired))
        XCTAssertEqual(
            "Session expired — enter the password again.",
            ErrorMessages.of(CoreError.SessionExpired)
        )
    }

    func testCoreErrorCasesMapWithoutTheirMessage() {
        XCTAssertEqual("That pairing payload is not valid.", ErrorMessages.of(CoreError.InvalidQr))
        XCTAssertEqual("Not connected to the gateway yet.", ErrorMessages.of(CoreError.NotConnected))
        XCTAssertEqual("The gateway did not answer in time.", ErrorMessages.of(CoreError.Timeout))
        XCTAssertEqual("Too many attempts — wait a moment and retry.", ErrorMessages.of(CoreError.RateLimited))
        XCTAssertEqual(
            "The gateway has no password auth enabled.",
            ErrorMessages.of(CoreError.UnknownProvider)
        )
        XCTAssertEqual("Wrong username or password.", ErrorMessages.of(CoreError.InvalidCredentials))
        XCTAssertEqual(
            "The gateway rejected the connection (host/origin guard).",
            ErrorMessages.of(CoreError.UpgradeRejected)
        )
        XCTAssertEqual(
            "That gateway URL is not valid.",
            ErrorMessages.of(CoreError.InvalidEndpoint("ws://nope"))
        )
        XCTAssertEqual(
            "Network error — check the connection.",
            ErrorMessages.of(CoreError.Network("boom"))
        )
    }

    func testCoreErrorFallbackKeepsRustDetail() {
        XCTAssertEqual(
            "Error: rpc error -32601: no such method",
            ErrorMessages.of(CoreError.Rpc(code: -32601, detail: "no such method"))
        )
        XCTAssertEqual("Error: io error: disk full", ErrorMessages.of(CoreError.Io("disk full")))
        XCTAssertEqual("Error: http status 503", ErrorMessages.of(CoreError.Http(503)))
        XCTAssertEqual("Error: unexpected submit status", ErrorMessages.of(CoreError.UnexpectedStatus))
    }

    func testEmptyMessageTransportIsNotAuthShaped() {
        XCTAssertFalse(ErrorMessages.isAuthShape(CoreError.Timeout))
        XCTAssertFalse(ErrorMessages.isAuthShape(err("")))
        XCTAssertTrue(ErrorMessages.isAuthShape(CoreError.InvalidCredentials))
        XCTAssertTrue(ErrorMessages.isAuthShape(CoreError.UpgradeRejected))
    }
}
