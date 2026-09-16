import ComposableArchitecture
import HermesCore
import HermoGateway
import HermoLogic

@Reducer
public struct BotDrawerFeature: Sendable {
    @Dependency(\.gatewayClient) var gatewayClient

    public init() {}

    @ObservableState
    public struct State: Equatable, Sendable {
        public var drawerState: DrawerUiState
        public var botOpenInFlight: Bool

        public init(drawerState: DrawerUiState = .loading, botOpenInFlight: Bool = false) {
            self.drawerState = drawerState
            self.botOpenInFlight = botOpenInFlight
        }
    }

    public enum Action: Equatable, Sendable {
        case drawerOpened
        case retryTapped
        case profileTapped(profile: String)
        case profilesResponse(rows: [BotDrawerRow]?, errorText: String)
        case botChatOpened
        case botChatOpenFailed
        case delegate(Delegate)

        public enum Delegate: Equatable, Sendable {
            case openProfile(String)
        }
    }

    public var body: some ReducerOf<Self> {
        Reduce { state, action in
            switch action {
            case .drawerOpened:
                // Keeps the rows already on screen during a reload; only Loading and Failed reset to Loading.
                switch state.drawerState {
                case .ready:
                    break
                case .loading, .failed:
                    state.drawerState = .loading
                }
                return Self.loadProfiles(gatewayClient: gatewayClient)

            case .retryTapped:
                state.drawerState = state.drawerState.onRetry()
                return Self.loadProfiles(gatewayClient: gatewayClient)

            case let .profilesResponse(rows, errorText):
                state.drawerState = state.drawerState.onProfilesLoaded(
                    rows: rows,
                    message: errorText.isEmpty ? "Could not load profiles" : errorText
                )
                return .none

            case let .profileTapped(profile):
                guard !state.botOpenInFlight else { return .none }
                state.botOpenInFlight = true
                return .send(.delegate(.openProfile(profile)))

            case .botChatOpened, .botChatOpenFailed:
                state.botOpenInFlight = false
                return .none

            case .delegate:
                return .none
            }
        }
    }

    private static func loadProfiles(gatewayClient: GatewayClient) -> Effect<Action> {
        .run { [gatewayClient] send in
            do {
                let profiles = try await gatewayClient.listProfiles()
                let rows = profiles.map { botDrawerRow(name: $0.name, model: $0.model, description: $0.description) }
                await send(.profilesResponse(rows: rows, errorText: ""))
            } catch {
                await send(.profilesResponse(rows: nil, errorText: ErrorMessages.of(error)))
            }
        }
    }
}
