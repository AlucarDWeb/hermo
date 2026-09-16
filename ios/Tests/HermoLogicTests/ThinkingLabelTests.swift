import XCTest
@testable import HermoLogic

final class ThinkingLabelTests: XCTestCase {

    func testTheSettledLabelHasTheDesktopsThreeStates() {
        XCTAssertEqual("Thought", ThinkingLabel.settledLabel(measuredS: nil)) // never watched running
        XCTAssertEqual("Thought briefly", ThinkingLabel.settledLabel(measuredS: 0)) // rounds to 0s
        XCTAssertEqual("Thought for 42s", ThinkingLabel.settledLabel(measuredS: 42))
        XCTAssertEqual("Thought for 1:05", ThinkingLabel.settledLabel(measuredS: 65)) // formatElapsed's m:ss branch
    }

    func testTheBodyFollowsTheLiveFlagAndTheUserToggleWins() {
        // Streaming: the body is open.
        XCTAssertTrue(ThinkingLabel.isOpen(live: true, userToggle: nil))

        // Settle: `live` flips false and the body collapses, no latch keeps it open.
        XCTAssertFalse(ThinkingLabel.isOpen(live: false, userToggle: nil))

        // The user's explicit toggle outranks the live flag, both ways.
        XCTAssertTrue(ThinkingLabel.isOpen(live: false, userToggle: true))
        XCTAssertFalse(ThinkingLabel.isOpen(live: true, userToggle: false))
    }
}
