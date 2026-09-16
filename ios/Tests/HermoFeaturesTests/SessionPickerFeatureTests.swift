import ComposableArchitecture
import HermesCore
import HermoFeatures
import HermoGateway
import HermoLogic
import XCTest

@MainActor
final class SessionPickerFeatureTests: XCTestCase {

    func testOpenWithNoEndpointIsGuardedOut() async {
        let store = TestStore(initialState: SessionPickerFeature.State()) {
            SessionPickerFeature()
        }

        await store.send(.open(hasEndpoint: false))
    }

    func testOpenWithAnEndpointLoadsSessions() async {
        let dto = RemoteSessionDto(id: "s1", title: "Standup notes", preview: "let's sync", messageCount: 4)
        let store = TestStore(initialState: SessionPickerFeature.State()) {
            SessionPickerFeature()
        } withDependencies: {
            $0.gatewayClient.listRemoteSessions = { [dto] }
        }

        await store.send(.open(hasEndpoint: true)) {
            $0.isPresented = true
            $0.isLoading = true
        }
        await store.receive(.sessionsLoaded([RemoteSessionRow(dto)])) {
            $0.isLoading = false
            $0.sessions = [RemoteSessionRow(dto)]
        }
    }

    func testOpenWithAnEndpointFailingLoadSurfacesLoadFailedAndDropsTheStaleRows() async {
        let stale = RemoteSessionRow(id: "old", title: "Yesterday", preview: "gone", messageCount: 2)
        let store = TestStore(initialState: SessionPickerFeature.State(sessions: [stale])) {
            SessionPickerFeature()
        } withDependencies: {
            $0.gatewayClient.listRemoteSessions = { throw CoreError.NotConnected }
        }

        await store.send(.open(hasEndpoint: true)) {
            $0.isPresented = true
            $0.isLoading = true
        }
        await store.receive(.loadFailed) {
            $0.isLoading = false
            $0.sessions = []
        }
    }
}
