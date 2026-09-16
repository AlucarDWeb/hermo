import XCTest
import HermoLogic

final class ThemeModeTests: XCTestCase {

    func testSystemTracksTheOsSetting() {
        XCTAssertTrue(resolve(.system, systemDark: true))
        XCTAssertFalse(resolve(.system, systemDark: false))
    }

    func testLightIgnoresTheOsSetting() {
        XCTAssertFalse(resolve(.light, systemDark: false))
        XCTAssertFalse(resolve(.light, systemDark: true))
    }

    func testDarkIgnoresTheOsSetting() {
        XCTAssertTrue(resolve(.dark, systemDark: false))
        XCTAssertTrue(resolve(.dark, systemDark: true))
    }

    func testBlankStoredValueParsesAsSystem() {
        XCTAssertEqual(ThemeMode.system, ThemeMode.fromStored(nil))
        XCTAssertEqual(ThemeMode.system, ThemeMode.fromStored(""))
        XCTAssertEqual(ThemeMode.system, ThemeMode.fromStored("   "))
    }

    func testUnknownStoredValueParsesAsSystem() {
        XCTAssertEqual(ThemeMode.system, ThemeMode.fromStored("midnight"))
        XCTAssertEqual(ThemeMode.system, ThemeMode.fromStored("dark;rm -rf"))
    }

    func testKnownStoredValuesRoundTrip() {
        XCTAssertEqual(ThemeMode.light, ThemeMode.fromStored("Light"))
        XCTAssertEqual(ThemeMode.dark, ThemeMode.fromStored("dark"))
        XCTAssertEqual(ThemeMode.system, ThemeMode.fromStored("SYSTEM"))
    }
}
