import XCTest
import HermoLogic

final class BotDrawerTests: XCTestCase {

    func testMappingKeepsTheProfileKeyEqualToTheName() {
        let row = botDrawerRow(name: "jn-core", model: "grok-4.6", description: "Core fixes bot")
        XCTAssertEqual(row.name, "jn-core")
        XCTAssertEqual(row.profile, "jn-core")
        XCTAssertEqual(row.model, "grok-4.6")
        XCTAssertEqual(row.description, "Core fixes bot")
    }

    func testMappingKeepsEmptyModelAndDescriptionEmptyNothingInvented() {
        let row = botDrawerRow(name: "default", model: "", description: "")
        XCTAssertEqual(row.model, "")
        XCTAssertEqual(row.description, "")
        XCTAssertEqual(row.name, "default")
        XCTAssertEqual(row.profile, "default")
    }

    func testASuccessfulLoadReducesToReadyWithEveryRow() {
        let rows = [
            botDrawerRow(name: "default", model: "", description: "Launch profile"),
            botDrawerRow(name: "jn-review", model: "grok-4.6", description: ""),
        ]
        let next = DrawerUiState.loading.onProfilesLoaded(rows: rows, message: "unused")
        XCTAssertEqual(next, .ready(rows))
    }

    func testAFailedRPCIsFailedNeverASilentEmptyDrawer() {
        let next = DrawerUiState.loading.onProfilesLoaded(rows: nil, message: "gateway unreachable")
        XCTAssertEqual(next, .failed("gateway unreachable"))
    }

    func testASuccessfulEMPTYListIsReadyALegitGatewayWithNoProfiles() {
        let next = DrawerUiState.loading.onProfilesLoaded(rows: [], message: "unused")
        XCTAssertEqual(next, .ready([]))
    }

    func testRetryFromFailedGoesBackToLoadingNoStaleRows() {
        let next = DrawerUiState.failed("boom").onRetry()
        XCTAssertEqual(next, .loading)
    }

    func testRetryFromLoadingStaysLoadingDoubleTapIsHarmless() {
        let next = DrawerUiState.loading.onRetry()
        XCTAssertEqual(next, .loading)
    }

    func testRetryFromReadyKeepsTheRowsRetryIsOnlyAFailureAffordance() {
        let rows = [botDrawerRow(name: "jn-core", model: "", description: "")]
        let next = DrawerUiState.ready(rows).onRetry()
        XCTAssertEqual(next, .ready(rows))
    }

    func testOpenTabForProfileFindsTheOpenTabByProfile() {
        let sessions = [
            "k-launch": SessionUiState(key: "k-launch"),
            "k-bot": SessionUiState(key: "k-bot", profile: "jn-core"),
        ]
        XCTAssertEqual(openTabForProfile(sessions: sessions, order: ["k-launch", "k-bot"], profile: "jn-core"), "k-bot")
    }

    func testOpenTabForProfileMissesUnknownAndBlankProfiles() {
        let sessions = ["k-bot": SessionUiState(key: "k-bot", profile: "jn-core")]
        XCTAssertNil(openTabForProfile(sessions: sessions, order: ["k-bot"], profile: "jn-android"))
        XCTAssertNil(openTabForProfile(sessions: sessions, order: ["k-bot"], profile: ""))
    }

    func testOpenTabForProfileBreaksATieBetweenSharedProfilesByTabOrder() {
        let sessions = [
            "k-first": SessionUiState(key: "k-first", profile: "jn-core"),
            "k-second": SessionUiState(key: "k-second", profile: "jn-core"),
        ]
        XCTAssertEqual(openTabForProfile(sessions: sessions, order: ["k-first", "k-second"], profile: "jn-core"), "k-first")
        XCTAssertEqual(openTabForProfile(sessions: sessions, order: ["k-second", "k-first"], profile: "jn-core"), "k-second")
    }

    func testWithProfileStampsAFreshEntryButNeverClobbersTheWireValue() {
        let seeded = ["k-bot": SessionUiState(key: "k-bot")]
        let fresh = withProfile(sessions: seeded, key: "k-bot", profile: "jn-core")
        XCTAssertEqual(fresh["k-bot"]?.profile, "jn-core")

        let landed = ["k-bot": SessionUiState(key: "k-bot", profile: "wire")]
        XCTAssertEqual(withProfile(sessions: landed, key: "k-bot", profile: "jn-core"), landed)

        XCTAssertEqual(withProfile(sessions: landed, key: "k-bot", profile: ""), landed)
        XCTAssertEqual(withProfile(sessions: [:], key: "k-x", profile: "jn-core"), [:])
    }
}
