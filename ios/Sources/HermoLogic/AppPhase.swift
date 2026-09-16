public enum AppPhase: Equatable, Sendable {
    case unpaired
    case needsPassword(endpoint: String, overlay: Bool = false)
    case connecting
    case ready(model: String)
    case offline(reason: String)
}
