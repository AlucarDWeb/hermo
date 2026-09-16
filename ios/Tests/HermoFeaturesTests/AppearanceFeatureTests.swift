import ComposableArchitecture
import HermoFeatures
import HermoLogic
import XCTest

@MainActor
final class AppearanceFeatureTests: XCTestCase {

    func testTaskLoadsTheModeFromTheStoredRawValue() async {
        let store = TestStore(initialState: AppearanceFeature.State()) {
            AppearanceFeature()
        } withDependencies: {
            $0.appearanceStorage.load = { "Dark" }
        }

        await store.send(.task) {
            $0.mode = .dark
        }
    }

    func testModeSelectedSavesTheRawValueThatLoadRoundTrips() async {
        let saved = LockIsolated<String?>(nil)
        let store = TestStore(initialState: AppearanceFeature.State()) {
            AppearanceFeature()
        } withDependencies: {
            $0.appearanceStorage.save = { saved.setValue($0) }
        }

        await store.send(.modeSelected(.light)) {
            $0.mode = .light
        }
        XCTAssertEqual(saved.value, "Light")

        let reloadStore = TestStore(initialState: AppearanceFeature.State()) {
            AppearanceFeature()
        } withDependencies: {
            $0.appearanceStorage.load = { saved.value }
        }

        await reloadStore.send(.task) {
            $0.mode = .light
        }
    }

    func testUnknownStoredValueFallsBackToSystemLikeThemeModeFromStored() async {
        let store = TestStore(initialState: AppearanceFeature.State(mode: .dark)) {
            AppearanceFeature()
        } withDependencies: {
            $0.appearanceStorage.load = { "Sepia" }
        }

        await store.send(.task) {
            $0.mode = .system
        }
    }

    func testMissingStoredValueFallsBackToSystemLikeThemeModeFromStored() async {
        let store = TestStore(initialState: AppearanceFeature.State(mode: .dark)) {
            AppearanceFeature()
        } withDependencies: {
            $0.appearanceStorage.load = { nil }
        }

        await store.send(.task) {
            $0.mode = .system
        }
    }
}
