import XCTest
@testable import HermoLogic

final class SlashPolicyTests: XCTestCase {

    func testCompletesOnLeadingSlashWithoutSpace() {
        XCTAssertTrue(SlashPolicy.shouldComplete("/"))
        XCTAssertTrue(SlashPolicy.shouldComplete("/he"))
        XCTAssertTrue(SlashPolicy.shouldComplete("/help"))
    }

    func testHidesOnSpaceArgumentStage() {
        XCTAssertFalse(SlashPolicy.shouldComplete("/help "))
        XCTAssertFalse(SlashPolicy.shouldComplete("/model opus"))
    }

    func testHidesOnEmptyOrNonSlashDraft() {
        XCTAssertFalse(SlashPolicy.shouldComplete(""))
        XCTAssertFalse(SlashPolicy.shouldComplete("hello"))
        XCTAssertFalse(SlashPolicy.shouldComplete(" /cmd"))
    }

    func testInsertReplacesFromIndexAndAddsTrailingSpace() {
        XCTAssertEqual("/help ", SlashPolicy.insertCompletion("/he", "/help", 1))
    }

    func testInsertKeepsPrefixBeforeReplaceFrom() {
        XCTAssertEqual("/model opus ", SlashPolicy.insertCompletion("/model o", "opus", 8))
    }

    func testInsertTreatsMissingOrNegativeReplaceFromAsOne() {
        XCTAssertEqual("/clear ", SlashPolicy.insertCompletion("/cle", "/clear", -1))
        XCTAssertEqual("/clear ", SlashPolicy.insertCompletion("/cle", "/clear", 0))
    }

    func testClearSessionsQuitAreLocal() {
        XCTAssertTrue(SlashPolicy.isLocalCommand("/clear"))
        XCTAssertTrue(SlashPolicy.isLocalCommand("/sessions"))
        XCTAssertTrue(SlashPolicy.isLocalCommand("/quit"))
    }

    func testEverythingElseIsNotLocal() {
        XCTAssertFalse(SlashPolicy.isLocalCommand("/help"))
        XCTAssertFalse(SlashPolicy.isLocalCommand("/model"))
        XCTAssertFalse(SlashPolicy.isLocalCommand("hello"))
        XCTAssertFalse(SlashPolicy.isLocalCommand(""))
    }

    func testLocalMatchIsTheWholeToken() {
        XCTAssertFalse(SlashPolicy.isLocalCommand("/clears"))
        XCTAssertFalse(SlashPolicy.isLocalCommand("/clear now"))
    }

    func testSubmitRoutesLeadingSlashToRunSlash() {
        XCTAssertTrue(SlashPolicy.isSlashSubmit("/help"))
        XCTAssertTrue(SlashPolicy.isSlashSubmit("/model opus"))
        XCTAssertFalse(SlashPolicy.isSlashSubmit("hello"))
        XCTAssertFalse(SlashPolicy.isSlashSubmit(""))
    }
}
