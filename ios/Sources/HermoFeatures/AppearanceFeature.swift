import ComposableArchitecture
import Foundation
import HermoLogic

@DependencyClient
public struct AppearanceStorage: Sendable {
    public var load: @Sendable () -> String? = { nil }
    public var save: @Sendable (_ rawValue: String) -> Void
}

extension AppearanceStorage: TestDependencyKey {
    public static let testValue = Self()
}

extension AppearanceStorage: DependencyKey {
    private static let key = "theme_mode"

    public static let liveValue = Self(
        load: { UserDefaults.standard.string(forKey: key) },
        save: { rawValue in UserDefaults.standard.set(rawValue, forKey: key) }
    )
}

extension DependencyValues {
    public var appearanceStorage: AppearanceStorage {
        get { self[AppearanceStorage.self] }
        set { self[AppearanceStorage.self] = newValue }
    }
}

@Reducer
public struct AppearanceFeature: Sendable {
    @Dependency(\.appearanceStorage) var appearanceStorage

    public init() {}

    @ObservableState
    public struct State: Equatable, Sendable {
        public var mode: ThemeMode

        public init(mode: ThemeMode = .system) {
            self.mode = mode
        }
    }

    public enum Action: Equatable, Sendable {
        case task
        case modeSelected(ThemeMode)
    }

    public var body: some ReducerOf<Self> {
        Reduce { state, action in
            switch action {
            case .task:
                state.mode = ThemeMode.fromStored(appearanceStorage.load())
                return .none

            case let .modeSelected(mode):
                state.mode = mode
                appearanceStorage.save(mode.rawValue)
                return .none
            }
        }
    }
}
