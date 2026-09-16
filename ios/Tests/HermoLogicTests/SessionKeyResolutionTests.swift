import XCTest
@testable import HermoLogic

/// Pins the fidelity fix's suspect #2: the screen must render the
/// repository-owned session key, never `sessions.keys.first`. With more than
/// one key in the map, a change arriving for a key nobody opened used to flip
/// the view to a foreign empty session and the rows appeared to vanish
/// mid-turn while the gateway still held every message.
final class SessionKeyResolutionTests: XCTestCase {

    func testAForeignSessionEntryNeverStealsTheView() {
        let mine = SessionUiState(key: "main", rows: (0..<39).map { #"{"kind":"user","text":"r\#($0)"}"# })
        let foreign = SessionUiState(key: "phantom")
        let sessions = ["phantom": foreign, "main": mine]
        let key = resolveSessionKey(currentKey: "main", sessions: sessions)
        XCTAssertEqual(key, "main")
        XCTAssertEqual(sessions[key!]?.rows.count, 39)
    }

    func testWithoutACurrentKeyTheFirstRealTranscriptClaimsIt() {
        let sessions = [
            "main": SessionUiState(key: "main", rows: [#"{"kind":"user","text":"q"}"#]),
            "other": SessionUiState(key: "other"),
        ]
        let key = resolveSessionKey(currentKey: nil, sessions: sessions)
        XCTAssertNotNil(key)
        XCTAssertFalse(sessions[key!]?.rows.isEmpty ?? true)
    }

    func testTheRepositoryExposesTheOwnedKeyViaTheParsedRowSurface() {
        let mine = SessionUiState(key: "main", rows: (0..<3).map { #"{"kind":"user","text":"r\#($0)"}"# })
        let key = resolveSessionKey(currentKey: "main", sessions: ["phantom": SessionUiState(key: "phantom"), "main": mine])
        XCTAssertEqual(key, "main")
        // Every row renders from the owned session's raw JSON.
        XCTAssertEqual(mine.rows.count, 3)
        if case .user(_, let text) = parseChatRow(2, mine.rows[2]) {
            XCTAssertEqual(text, "r2")
        } else {
            XCTFail("expected .user")
        }
    }
}
