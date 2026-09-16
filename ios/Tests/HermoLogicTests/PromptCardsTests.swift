import XCTest
import HermoLogic

/// `ChatRow` is not part of this port; the row's `choices` field is inlined
/// as a plain array where the Kotlin fixture used `parseChatRow` to reach it.
final class PromptCardsTests: XCTestCase {

    func testApprovalChoicesAreTheServersList() {
        let choices = ["once", "session", "always", "deny"]
        XCTAssertEqual(primaryApprovalChoice(choices), .run)
        XCTAssertEqual(secondaryApprovalChoices(choices), [.allowSession, .always, .reject])
        XCTAssertTrue(shouldRenderAlways(choices))
    }

    func testMissingAlwaysIsNotSynthesised() {
        let choices = ["once", "deny"]
        XCTAssertFalse(shouldRenderAlways(choices))
        XCTAssertEqual(primaryApprovalChoice(choices), .run)
        XCTAssertEqual(secondaryApprovalChoices(choices), [.reject])
    }

    func testAbsentChoicesArrayIsEmptyNeverInvented() {
        let choices: [String] = []
        XCTAssertNil(primaryApprovalChoice(choices))
        XCTAssertFalse(shouldRenderAlways(choices))
    }

    func testDesktopLabels() {
        XCTAssertEqual(ApprovalChoice.run.label, "Run")
        XCTAssertEqual(ApprovalChoice.allowSession.label, "Allow this session")
        XCTAssertEqual(ApprovalChoice.always.label, "Always allow")
        XCTAssertEqual(ApprovalChoice.reject.label, "Reject")
    }

    func test4009And4018AreAnsweredElsewhereNotAnErrorDialog() {
        XCTAssertEqual(approvalRespondErrorCopy("rpc error 4009: session busy"), ApprovalCopy.resolved)
        XCTAssertEqual(approvalRespondErrorCopy("rpc error 4018: unknown slash command"), ApprovalCopy.resolved)
        XCTAssertNil(approvalRespondErrorCopy("network failure: boom"))
    }

    func testClarifyBatchShape() {
        let json = #"[{"qid":"q0","question":"Pick one","choices":["a","b"],"multi_select":false},{"qid":"q1","question":"Pick many","choices":["x","y"],"multi_select":true}]"#
        let qs = parseClarifyQuestions(json)
        XCTAssertEqual(qs.count, 2)
        XCTAssertEqual(qs[0].qid, "q0")
        XCTAssertEqual(qs[0].choices, ["a", "b"])
        XCTAssertFalse(qs[0].multiSelect)
        XCTAssertTrue(qs[1].multiSelect)
        XCTAssertEqual(clarifyProgressLabel(1, 2), "1 of 2 answered")
        XCTAssertEqual(encodeClarifyAnswer(qs[1], picks: ["x", "y"], draft: ""), #"["x","y"]"#)
        XCTAssertEqual(encodeClarifyAnswer(qs[0], picks: [], draft: " hello "), "hello")
    }

    func testSingleSelectChipPickEncodesThePickNotTheEmptyDraft() {
        let json = #"[{"qid":"q0","question":"Pick one","choices":["a","b"],"multi_select":false}]"#
        let q = parseClarifyQuestions(json)[0]
        XCTAssertEqual(encodeClarifyAnswer(q, picks: ["a"], draft: ""), "a")
    }

    func testClarifyGarbageIsEmptyNeverThrows() {
        XCTAssertEqual(parseClarifyQuestions(""), [])
        XCTAssertEqual(parseClarifyQuestions("not-json"), [])
        XCTAssertEqual(parseClarifyQuestions("{}"), [])
    }
}
