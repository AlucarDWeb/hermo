import ComposableArchitecture
import HermesCore
import HermoFeatures
import HermoGateway
import HermoLogic
import XCTest

@MainActor
final class BotDrawerFeatureTests: XCTestCase {

    func testReloadKeepsThePreviousRowsVisibleInsteadOfBlanking() async {
        let existingRows = [BotDrawerRow(name: "echo", model: "gpt-4", description: "Echoes back", profile: "echo")]
        let newRows = [BotDrawerRow(name: "helper", model: "gpt-5", description: "Helps out", profile: "helper")]
        let store = TestStore(initialState: BotDrawerFeature.State(drawerState: .ready(existingRows))) {
            BotDrawerFeature()
        } withDependencies: {
            $0.gatewayClient.listProfiles = {
                [ProfileSummaryDto(name: "helper", isDefault: false, model: "gpt-5", description: "Helps out")]
            }
        }

        await store.send(.drawerOpened) {
            $0.isOpen = true
        }
        await store.receive(.profilesResponse(rows: newRows, errorText: "")) {
            $0.drawerState = .ready(newRows)
        }
    }

    func testInFlightGuardRejectsASecondProfileTap() async {
        let store = TestStore(initialState: BotDrawerFeature.State()) {
            BotDrawerFeature()
        }

        await store.send(.profileTapped(profile: "echo")) {
            $0.botOpenInFlight = true
        }
        await store.receive(.delegate(.openProfile("echo")))

        await store.send(.profileTapped(profile: "other"))
    }

    func testFailedLoadWithNoErrorTextShowsTheCouldNotLoadProfilesFallback() async {
        let store = TestStore(initialState: BotDrawerFeature.State()) {
            BotDrawerFeature()
        }

        await store.send(.profilesResponse(rows: nil, errorText: "")) {
            $0.drawerState = .failed("Could not load profiles")
        }
    }

    func testAFailedBotChatKeepsTheDrawerOpen() async {
        let store = TestStore(initialState: BotDrawerFeature.State(drawerState: .ready([]), isOpen: true)) {
            BotDrawerFeature()
        }

        await store.send(.profileTapped(profile: "helper")) {
            $0.botOpenInFlight = true
        }
        await store.receive(.delegate(.openProfile("helper")))
        // The Kotlin closes the drawer only on success, so its error row stays readable.
        await store.send(.botChatOpenFailed) {
            $0.botOpenInFlight = false
        }
        XCTAssertTrue(store.state.isOpen)
    }

    func testASuccessfulBotChatClosesTheDrawer() async {
        let store = TestStore(initialState: BotDrawerFeature.State(drawerState: .ready([]), isOpen: true)) {
            BotDrawerFeature()
        }

        await store.send(.botChatOpened) {
            $0.isOpen = false
        }
    }
}
