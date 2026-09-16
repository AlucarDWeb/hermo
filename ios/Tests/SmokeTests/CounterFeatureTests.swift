import ComposableArchitecture
import XCTest

@testable import Smoke

@MainActor
final class CounterFeatureTests: XCTestCase {
    func testIncrementThenDecrement() async {
        let store = TestStore(initialState: CounterFeature.State()) {
            CounterFeature()
        }

        await store.send(.increment) {
            $0.count = 1
        }
        await store.send(.decrement) {
            $0.count = 0
        }
    }
}
