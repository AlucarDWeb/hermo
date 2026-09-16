import ComposableArchitecture
import Foundation
import HermesCore
import HermoGateway
import HermoLogic

@Reducer
public struct ChatFeature: Sendable {
    @Dependency(\.gatewayClient) var gatewayClient
    @Dependency(\.continuousClock) var clock

    public init() {}

    private enum CancelID: Hashable {
        case completion
        case banner
    }

    private static let noOpenSessionToSend = "No open session to send into"
    private static let noOpenSessionToAnswer = "No open session to answer in"
    private static let quitBannerText = "Quitting is a desktop command — the phone stays ready."
    private static let completionDebounce: Duration = .milliseconds(150)
    private static let bannerDuration: Duration = .seconds(5)

    @ObservableState
    public struct State: Equatable, Sendable {
        public var draft: String
        public var slashCompletions: [SlashCompletionRow]
        public var slashReplaceFrom: Int64
        public var slashBanner: String

        public init(
            draft: String = "",
            slashCompletions: [SlashCompletionRow] = [],
            slashReplaceFrom: Int64 = 1,
            slashBanner: String = ""
        ) {
            self.draft = draft
            self.slashCompletions = slashCompletions
            self.slashReplaceFrom = slashReplaceFrom
            self.slashBanner = slashBanner
        }
    }

    public enum Action: Equatable, Sendable {
        case draftChanged(String)
        case dismissCompletions
        case completionsLoaded(SlashCompletionsDto)

        case send(key: String?)
        case slashOutcomeReceived(SlashOutcome)
        case delegate(Delegate)

        public enum Delegate: Equatable, Sendable {
            /// This reducer has no `errorText`; the app-level state owns it, exactly as the
            /// Kotlin keeps one `_errorText` on the repository.
            case reportError(String)
            case openSessionPicker
        }

        case bannerCleared

        case interrupt(key: String)

        case respondApproval(key: String?, requestId: String, choice: String)

        case respondClarify(key: String?, requestId: String, answer: String, questionId: String)
    }

    public var body: some ReducerOf<Self> {
        Reduce { state, action in
            switch action {
            case let .draftChanged(text):
                state.draft = text
                guard SlashPolicy.shouldComplete(text) else {
                    state.slashCompletions = []
                    return .cancel(id: CancelID.completion)
                }
                return .run { [gatewayClient, clock] send in
                    try await clock.sleep(for: Self.completionDebounce)
                    guard let dto = try? await gatewayClient.completeSlash(text) else { return }
                    await send(.completionsLoaded(dto))
                }
                .cancellable(id: CancelID.completion, cancelInFlight: true)

            case .dismissCompletions:
                state.slashCompletions = []
                return .cancel(id: CancelID.completion)

            case let .completionsLoaded(dto):
                state.slashReplaceFrom = dto.replaceFrom
                state.slashCompletions = dto.items.map {
                    SlashCompletionRow(text: $0.text, display: $0.display, kind: $0.kind, meta: $0.meta)
                }
                return .none

            case let .send(key):
                return Self.handleSend(key: key, state: &state, gatewayClient: gatewayClient, clock: clock)

            case let .slashOutcomeReceived(outcome):
                switch outcome {
                case let .output(text):
                    return Self.showBanner(text, state: &state, clock: clock)
                case let .prefill(text):
                    state.draft = text
                    return .none
                case .submitted, .empty:
                    return .none
                }

            case .bannerCleared:
                state.slashBanner = ""
                return .none

            case let .interrupt(key):
                return .run { [gatewayClient] send in
                    do {
                        try await gatewayClient.interrupt(key)
                    } catch {
                        await send(.delegate(.reportError(ErrorMessages.of(error))))
                    }
                }

            case let .respondApproval(key, requestId, choice):
                guard let key else {
                    return .send(.delegate(.reportError(Self.noOpenSessionToAnswer)))
                }
                return .run { [gatewayClient] send in
                    do {
                        try await gatewayClient.respondApproval(key, requestId, choice)
                    } catch {
                        await send(.delegate(.reportError(Self.approvalErrorCopy(error))))
                    }
                }

            case .delegate:
                return .none

            case let .respondClarify(key, requestId, answer, questionId):
                guard let key else {
                    return .send(.delegate(.reportError(Self.noOpenSessionToAnswer)))
                }
                let normalizedQuestionId = questionId.isEmpty ? nil : questionId
                return .run { [gatewayClient] send in
                    do {
                        try await gatewayClient.respondClarify(key, requestId, answer, normalizedQuestionId)
                    } catch {
                        await send(.delegate(.reportError(Self.approvalErrorCopy(error))))
                    }
                }

            }
        }
    }

    /// Routes on the trimmed draft, per `AppViewModel.send`: blank is dropped before any
    /// branch runs, a local command never reaches the client, a slash command runs through
    /// `runSlash`, anything else is a plain prompt on the untrimmed text.
    private static func handleSend(
        key: String?, state: inout State, gatewayClient: GatewayClient, clock: any Clock<Duration>
    ) -> Effect<Action> {
        let text = state.draft
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return .none }
        state.draft = ""
        state.slashCompletions = []
        let dismissPending = Effect<Action>.cancel(id: CancelID.completion)
        let command = text.trimmingCharacters(in: .whitespacesAndNewlines)

        if SlashPolicy.isLocalCommand(command) {
            return .merge(dismissPending, Self.handleLocalCommand(command, state: &state, clock: clock))
        }

        if SlashPolicy.isSlashSubmit(command) {
            guard let key else {
                return .merge(dismissPending, .send(.delegate(.reportError(Self.noOpenSessionToSend))))
            }
            return .merge(
                dismissPending,
                .run { [gatewayClient] send in
                    do {
                        let outcome = try await gatewayClient.runSlash(key, command)
                        await send(.slashOutcomeReceived(outcome))
                    } catch {
                        await send(.delegate(.reportError(ErrorMessages.of(error))))
                    }
                }
            )
        }

        guard let key else {
            return .merge(dismissPending, .send(.delegate(.reportError(Self.noOpenSessionToSend))))
        }
        return .merge(
            dismissPending,
            .run { [gatewayClient] send in
                do {
                    try await gatewayClient.send(key, text)
                } catch {
                    await send(.delegate(.reportError(ErrorMessages.of(error))))
                }
            }
        )
    }

    /// `/clear` clears nothing here: the transcript comes from the stream, not the composer, so
    /// there is no row for the composer to own.
    private static func handleLocalCommand(
        _ command: String, state: inout State, clock: any Clock<Duration>
    ) -> Effect<Action> {
        if command == "/clear" {
            return .none
        }
        if command == "/sessions" {
            return .send(.delegate(.openSessionPicker))
        }
        if command == "/quit" {
            return Self.showBanner(Self.quitBannerText, state: &state, clock: clock)
        }
        return .none
    }

    /// The `cancelInFlight` clear effect is this port's stand-in for the Kotlin generation
    /// counter: a new banner cancels the previous one's pending clear outright.
    private static func showBanner(_ text: String, state: inout State, clock: any Clock<Duration>) -> Effect<Action> {
        state.slashBanner = text
        return .run { send in
            try await clock.sleep(for: Self.bannerDuration)
            await send(.bannerCleared)
        }
        .cancellable(id: CancelID.banner, cancelInFlight: true)
    }

    private static func approvalErrorCopy(_ error: Error) -> String {
        approvalRespondErrorCopy(ErrorMessages.rustMessage(error)) ?? ErrorMessages.of(error)
    }
}
