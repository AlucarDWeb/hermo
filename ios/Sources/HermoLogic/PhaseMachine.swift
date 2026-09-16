public enum PhaseMachine {

    public enum ConnEvent: Equatable, Sendable {
        case connecting
        case open
        case closed(reason: String)
        case needsPassword
        case authFailed
        case header(model: String)
    }

    public static func reduce(
        phase: AppPhase,
        event: ConnEvent,
        savedEndpoint: String,
        hasLiveSession: Bool = false
    ) -> AppPhase {
        switch event {
        case .connecting:
            return .connecting
        case .open:
            if case .ready = phase {
                return phase
            } else {
                return .connecting
            }
        case .closed(let reason):
            if savedEndpoint.isEmpty {
                return .unpaired
            } else if reason.contains("user logout") || reason.contains("session expired") {
                return .needsPassword(endpoint: savedEndpoint, overlay: hasLiveSession)
            } else {
                return .offline(reason: reason)
            }
        case .needsPassword:
            if savedEndpoint.isEmpty {
                return .unpaired
            } else {
                return .needsPassword(endpoint: savedEndpoint, overlay: hasLiveSession)
            }
        case .authFailed:
            if savedEndpoint.isEmpty {
                return .unpaired
            } else {
                return .needsPassword(endpoint: savedEndpoint, overlay: hasLiveSession)
            }
        case .header(let model):
            if case .ready = phase, !model.isEmpty {
                return .ready(model: model)
            } else {
                return phase
            }
        }
    }

    public static func ready(_ model: String) -> AppPhase {
        .ready(model: model)
    }

    public static func offline(_ reason: String) -> AppPhase {
        .offline(reason: reason)
    }
}
