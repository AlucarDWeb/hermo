public struct BotDrawerRow: Equatable, Sendable {
    public let name: String
    public let model: String
    public let description: String
    /// The profile the tap opens; kept separate from `name` so a display label can never substitute for the key the core's create-or-resume verb needs.
    public let profile: String

    public init(name: String, model: String, description: String, profile: String) {
        self.name = name
        self.model = model
        self.description = description
        self.profile = profile
    }
}

public enum DrawerUiState: Equatable, Sendable {
    case loading
    case ready([BotDrawerRow])
    case failed(String)
}

/// One open session tab; the profile is stamped locally at open time and never overwritten once the gateway's own header value lands.
public struct SessionUiState: Equatable, Sendable {
    public var key: String
    public var title: String
    public var model: String
    public var rows: [String]
    public var running: Bool
    public var profile: String

    public init(
        key: String,
        title: String = "",
        model: String = "",
        rows: [String] = [],
        running: Bool = false,
        profile: String = ""
    ) {
        self.key = key
        self.title = title
        self.model = model
        self.rows = rows
        self.running = running
        self.profile = profile
    }
}

public func botDrawerRow(name: String, model: String, description: String) -> BotDrawerRow {
    BotDrawerRow(name: name, model: model, description: description, profile: name)
}

extension DrawerUiState {
    public func onProfilesLoaded(rows: [BotDrawerRow]?, message: String) -> DrawerUiState {
        guard let rows else { return .failed(message) }
        return .ready(rows)
    }

    /// Retry always goes back to loading, never leaves a failed screen up or re-shows stale rows.
    public func onRetry() -> DrawerUiState {
        switch self {
        case .failed, .loading: return .loading
        case .ready: return self
        }
    }
}

/// The already-open tab for a profile, so a drawer tap switches to it instead of re-issuing the core verb and resetting the transcript; a blank profile matches nothing.
public func openTabForProfile(sessions: [String: SessionUiState], profile: String) -> String? {
    if profile.isEmpty { return nil }
    return sessions.first(where: { $0.value.profile == profile })?.key
}

public func withProfile(sessions: [String: SessionUiState], key: String, profile: String) -> [String: SessionUiState] {
    if profile.isEmpty { return sessions }
    guard var current = sessions[key] else { return sessions }
    if !current.profile.isEmpty { return sessions }
    current.profile = profile
    var updated = sessions
    updated[key] = current
    return updated
}
