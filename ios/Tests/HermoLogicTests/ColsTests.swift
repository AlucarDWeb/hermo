import XCTest
import HermoLogic

final class ColsTests: XCTestCase {

    func test1080pXhdpiPhoneLandsInsideTheClamp() {
        XCTAssertEqual(40, Cols.from(widthPx: 1080, density: 2.625))
    }

    func testWideEmulatorScreenClampsTo40AsWell() {
        XCTAssertEqual(80, Cols.from(widthPx: 1080, density: 1.0))
    }

    func testMidValuePassesThroughUnclamped() {
        XCTAssertEqual(80, Cols.from(widthPx: 1040, density: 1.0))
        XCTAssertEqual(40, Cols.from(widthPx: 520, density: 1.0))
        XCTAssertEqual(50, Cols.from(widthPx: 650, density: 1.0))
    }

    func testDegenerateDensityFallsBackToMaxNeverZero() {
        XCTAssertEqual(80, Cols.from(widthPx: 0, density: 0))
        XCTAssertEqual(80, Cols.from(widthPx: 1000, density: 0))
    }
}
