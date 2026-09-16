import XCTest
import HermesCore

final class HermesCoreLinkTests: XCTestCase {
    private var tempDir: URL!

    override func setUpWithError() throws {
        tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: tempDir)
        tempDir = nil
    }

    func testSavedEndpointIsNilForFreshCore() async {
        let core = HermesCore(dataDir: tempDir.path)
        let endpoint = await core.savedEndpoint()
        XCTAssertNil(endpoint)
    }

    func testParseQrReturnsEndpointDto() async throws {
        let core = HermesCore(dataDir: tempDir.path)
        let endpoint = try await core.parseQr(
            payload: "hermes://connect?v=1&url=http%3A%2F%2F127.0.0.1%3A9123&user=hermo&name=fake"
        )
        XCTAssertEqual(endpoint.baseUrl, "http://127.0.0.1:9123/")
        XCTAssertEqual(endpoint.username, "hermo")
        XCTAssertEqual(endpoint.displayName, "fake")
    }

    func testParseQrThrowsInvalidQrForGarbage() async {
        let core = HermesCore(dataDir: tempDir.path)
        do {
            _ = try await core.parseQr(payload: "nope")
            XCTFail("expected parseQr to throw")
        } catch CoreError.InvalidQr {
        } catch {
            XCTFail("expected CoreError.InvalidQr, got \(error)")
        }
    }
}
