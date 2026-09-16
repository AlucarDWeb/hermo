import XCTest
@testable import HermoLogic

final class SessionTabsTests: XCTestCase {

    func testSelectOfAKeyNotInTheSetIsANoOp() {
        let tabs = TabSet(keys: ["a", "b"], current: "a")
        let next = tabs.select("zzz")
        XCTAssertEqual(next.keys, ["a", "b"])
        XCTAssertEqual(next.current, "a")
    }

    func testSelectOfAnOpenKeyOnlyMovesCurrent() {
        let tabs = TabSet(keys: ["a", "b", "c"], current: "a")
        let next = tabs.select("c")
        XCTAssertEqual(next.keys, ["a", "b", "c"])
        XCTAssertEqual(next.current, "c")
    }

    func testAddOfANewKeyAppendsAndSelectsIt() {
        let tabs = TabSet(keys: ["a"], current: "a")
        let next = tabs.add("b")
        XCTAssertEqual(next.keys, ["a", "b"])
        XCTAssertEqual(next.current, "b")
    }

    func testAddOfAnAlreadyOpenKeyOnlySelectsItPickerAlreadyOpen() {
        let tabs = TabSet(keys: ["a", "b"], current: "a")
        let next = tabs.add("b")
        XCTAssertEqual(next.keys, ["a", "b"])
        XCTAssertEqual(next.current, "b")
    }

    func testCloseOfTheLastRemainingTabEmptiesTheSet() {
        let tabs = TabSet(keys: ["only"], current: "only")
        let next = tabs.close("only")
        XCTAssertEqual(next.keys, [])
        XCTAssertNil(next.current)
    }

    func testCloseOfAnUnknownKeyIsANoOp() {
        let tabs = TabSet(keys: ["a", "b"], current: "b")
        let next = tabs.close("zzz")
        XCTAssertEqual(next.keys, ["a", "b"])
        XCTAssertEqual(next.current, "b")
    }

    func testCloseOfAMiddleCurrentTabSelectsTheLeftNeighbor() {
        let tabs = TabSet(keys: ["a", "b", "c"], current: "b")
        let next = tabs.close("b")
        XCTAssertEqual(next.keys, ["a", "c"])
        XCTAssertEqual(next.current, "a")
    }

    func testCloseOfIndex0SelectsTheNewFirst() {
        let tabs = TabSet(keys: ["a", "b", "c"], current: "a")
        let next = tabs.close("a")
        XCTAssertEqual(next.keys, ["b", "c"])
        XCTAssertEqual(next.current, "b")
    }

    func testCloseOfANonCurrentTabKeepsCurrent() {
        let tabs = TabSet(keys: ["a", "b", "c"], current: "c")
        let next = tabs.close("a")
        XCTAssertEqual(next.keys, ["b", "c"])
        XCTAssertEqual(next.current, "c")
    }

    func testRestorePlanResumesEveryKeyInOrderNotJustLastActive() {
        let plan = restorePlan(keys: ["a", "b", "c"], lastActive: "b")
        XCTAssertEqual(plan.resumeKeys, ["a", "b", "c"])
        XCTAssertEqual(plan.current, "b")
    }

    func testRestorePlanFallsBackToTheLastKeyWhenLastActiveIsAbsent() {
        let plan = restorePlan(keys: ["a", "b"], lastActive: nil)
        XCTAssertEqual(plan.resumeKeys, ["a", "b"])
        XCTAssertEqual(plan.current, "b")
    }

    func testRestorePlanIgnoresALastActiveThatIsNotInTheRegistry() {
        let plan = restorePlan(keys: ["a", "b"], lastActive: "ghost")
        XCTAssertEqual(plan.resumeKeys, ["a", "b"])
        XCTAssertEqual(plan.current, "b")
    }

    func testRestorePlanWithAnEmptyRegistryIsAnEmptyPlanCallerMints() {
        let plan = restorePlan(keys: [], lastActive: nil)
        XCTAssertEqual(plan.resumeKeys, [])
        XCTAssertNil(plan.current)
    }

    func testLaunchStepTreatsAFailedRegistryReadAsListFailedNotEmpty() {
        XCTAssertEqual(launchStep(registryKeys: nil, lastActive: nil), .listFailed)
        let step = launchStep(registryKeys: [], lastActive: nil)
        guard case .runPlan(let plan) = step else {
            return XCTFail("expected .runPlan")
        }
        XCTAssertEqual(plan.resumeKeys, [])
    }

    func testLaunchStepPlansASuccessfulRegistryRead() {
        let step = launchStep(registryKeys: ["a", "b"], lastActive: "b")
        guard case .runPlan(let plan) = step else {
            return XCTFail("expected .runPlan")
        }
        XCTAssertEqual(plan.resumeKeys, ["a", "b"])
        XCTAssertEqual(plan.current, "b")
    }

    func testTabTapOnTheCurrentTabIsANoOp() {
        let tabs = TabSet(keys: ["a", "b"], current: "a")
        XCTAssertNil(tabTap(tabs: tabs, currentKey: "a", key: "a"))
    }

    func testTabTapOutsideTheSetIsANoOpToo() {
        let tabs = TabSet(keys: ["a", "b"], current: "a")
        XCTAssertNil(tabTap(tabs: tabs, currentKey: "a", key: "zzz"))
    }

    func testTabTapOnAnotherOpenTabSelectsIt() {
        let tabs = TabSet(keys: ["a", "b"], current: "a")
        XCTAssertEqual(tabTap(tabs: tabs, currentKey: "a", key: "b")?.current, "b")
    }
}
