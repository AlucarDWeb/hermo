import ComposableArchitecture
import HermesCore
import HermoGateway
import HermoLogic

@Reducer
public struct SessionPickerFeature: Sendable {
    @Dependency(\.gatewayClient) var gatewayClient

    public init() {}

    @ObservableState
    public struct State: Equatable, Sendable {
        public var isPresented: Bool
        public var isLoading: Bool
        public var sessions: [RemoteSessionRow]

        public init(
            isPresented: Bool = false,
            isLoading: Bool = false,
            sessions: [RemoteSessionRow] = []
        ) {
            self.isPresented = isPresented
            self.isLoading = isLoading
            self.sessions = sessions
        }
    }

    public enum Action: Equatable, Sendable {
        case open(hasEndpoint: Bool)
        case dismiss
        case sessionsLoaded([RemoteSessionRow])
        case loadFailed
        case delegate(Delegate)

        public enum Delegate: Equatable, Sendable {
            case rowTapped(storedId: String)
            case newChatTapped
        }
    }

    public var body: some ReducerOf<Self> {
        Reduce { state, action in
            switch action {
            case let .open(hasEndpoint):
                guard hasEndpoint else { return .none }
                state.isPresented = true
                state.isLoading = true
                return .run { [gatewayClient] send in
                    do {
                        let dtos = try await gatewayClient.listRemoteSessions()
                        await send(.sessionsLoaded(dtos.map(RemoteSessionRow.init)))
                    } catch {
                        await send(.loadFailed)
                    }
                }

            case .dismiss:
                state.isPresented = false
                return .none

            case let .sessionsLoaded(sessions):
                state.isLoading = false
                state.sessions = sessions
                return .none

            case .loadFailed:
                state.isLoading = false
                // A failed reload drops the previous rows: with no error copy on the sheet,
                // leaving them up reads as a successful load and offers stale ids to resume.
                state.sessions = []
                return .none

            case .delegate:
                return .none
            }
        }
    }
}

extension RemoteSessionRow {
    public init(_ dto: RemoteSessionDto) {
        self.init(id: dto.id, title: dto.title, preview: dto.preview, messageCount: dto.messageCount)
    }
}
