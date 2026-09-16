import ComposableArchitecture
import HermesCore
import HermoFeatures
import HermoGateway
import HermoLogic
import XCTest

private let quitBannerText = "Quitting is a desktop command — the phone stays ready."
private let noOpenSessionToSend = "No open session to send into"
private let noOpenSessionToAnswer = "No open session to answer in"

@MainActor
final class ChatFeatureTests: XCTestCase {

    // MARK: Completion debounce

    func testCompletionDebounceFiresOnceAfter150msAndASecondKeystrokeCancelsTheFirst() async {
        let clock = TestClock()
        let callCount = LockIsolated(0)
        let dto = SlashCompletionsDto(
            items: [SlashCompletionDto(display: "/help", text: "/help", kind: "command", meta: "")],
            replaceFrom: 1
        )
        let store = TestStore(initialState: ChatFeature.State()) {
            ChatFeature()
        } withDependencies: {
            $0.continuousClock = clock
            $0.gatewayClient.completeSlash = { _ in
                callCount.withValue { $0 += 1 }
                return dto
            }
        }

        await store.send(.draftChanged("/h")) {
            $0.draft = "/h"
        }
        await clock.advance(by: .milliseconds(100))
        await store.send(.draftChanged("/he")) {
            $0.draft = "/he"
        }
        await clock.advance(by: .milliseconds(150))
        await store.receive(.completionsLoaded(dto)) {
            $0.slashReplaceFrom = 1
            $0.slashCompletions = [SlashCompletionRow(text: "/help", display: "/help", kind: "command", meta: "")]
        }
        XCTAssertEqual(callCount.value, 1)
    }

    // MARK: Send routing table

    func testSendWithBlankDraftIsIgnored() async {
        let store = TestStore(initialState: ChatFeature.State(draft: "   ")) {
            ChatFeature()
        }

        await store.send(.send(key: "s1"))
    }

    func testSendClearIsALocalNoOp() async {
        let store = TestStore(initialState: ChatFeature.State(draft: "/clear")) {
            ChatFeature()
        }

        await store.send(.send(key: "s1")) {
            $0.draft = ""
        }
    }

    func testSendSessionsRequestsThePicker() async {
        let store = TestStore(initialState: ChatFeature.State(draft: "/sessions")) {
            ChatFeature()
        }

        await store.send(.send(key: "s1")) {
            $0.draft = ""
        }
        await store.receive(.delegate(.openSessionPicker))
    }

    func testSendQuitShowsTheQuittingBanner() async {
        let clock = TestClock()
        let store = TestStore(initialState: ChatFeature.State(draft: "/quit")) {
            ChatFeature()
        } withDependencies: {
            $0.continuousClock = clock
        }

        await store.send(.send(key: "s1")) {
            $0.draft = ""
            $0.slashBanner = quitBannerText
        }
        await clock.advance(by: .seconds(5))
        await store.receive(.bannerCleared) {
            $0.slashBanner = ""
        }
    }

    func testSendNonLocalSlashRunsThroughRunSlash() async {
        let clock = TestClock()
        let store = TestStore(initialState: ChatFeature.State(draft: "/help")) {
            ChatFeature()
        } withDependencies: {
            $0.continuousClock = clock
            $0.gatewayClient.runSlash = { _, _ in .output(text: "help text") }
        }

        await store.send(.send(key: "s1")) {
            $0.draft = ""
        }
        await store.receive(.slashOutcomeReceived(.output(text: "help text"))) {
            $0.slashBanner = "help text"
        }
        await clock.advance(by: .seconds(5))
        await store.receive(.bannerCleared) {
            $0.slashBanner = ""
        }
    }

    func testSendPlainTextSendsToTheGateway() async {
        let store = TestStore(initialState: ChatFeature.State(draft: "hello there")) {
            ChatFeature()
        } withDependencies: {
            $0.gatewayClient.send = { _, _ in }
        }

        await store.send(.send(key: "s1")) {
            $0.draft = ""
        }
    }

    // MARK: Banner auto-clear survives a second banner raised inside the window

    func testASecondBannerRaisedWithinTheAutoClearWindowSurvivesTheFirstTimer() async {
        let clock = TestClock()
        let store = TestStore(initialState: ChatFeature.State(draft: "/quit")) {
            ChatFeature()
        } withDependencies: {
            $0.continuousClock = clock
        }

        await store.send(.send(key: "s1")) {
            $0.draft = ""
            $0.slashBanner = quitBannerText
        }

        await clock.advance(by: .seconds(3))
        await store.send(.slashOutcomeReceived(.output(text: "second banner"))) {
            $0.slashBanner = "second banner"
        }

        // The first banner's own 5 s window falls here; nothing should clear the second banner yet.
        await clock.advance(by: .seconds(2))

        await clock.advance(by: .seconds(3))
        await store.receive(.bannerCleared) {
            $0.slashBanner = ""
        }
    }

    // MARK: No open session

    func testSendWithNoOpenSessionYieldsNoOpenSessionToSendInto() async {
        let store = TestStore(initialState: ChatFeature.State(draft: "hello")) {
            ChatFeature()
        }

        await store.send(.send(key: nil)) {
            $0.draft = ""
        }
        await store.receive(.delegate(.reportError(noOpenSessionToSend)))
    }

    func testRespondApprovalWithNoOpenSessionYieldsNoOpenSessionToAnswerIn() async {
        let store = TestStore(initialState: ChatFeature.State()) {
            ChatFeature()
        }

        await store.send(.respondApproval(key: nil, requestId: "req-1", choice: "yes"))
        await store.receive(.delegate(.reportError(noOpenSessionToAnswer)))
    }

    func testRespondClarifyWithNoOpenSessionYieldsNoOpenSessionToAnswerIn() async {
        let store = TestStore(initialState: ChatFeature.State()) {
            ChatFeature()
        }

        await store.send(.respondClarify(key: nil, requestId: "req-1", answer: "42", questionId: ""))
        await store.receive(.delegate(.reportError(noOpenSessionToAnswer)))
    }
}
