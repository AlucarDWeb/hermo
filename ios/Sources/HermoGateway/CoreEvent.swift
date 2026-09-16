import HermesCore

public enum CoreEvent: Sendable, Equatable {
    case transcript(TranscriptChangeDto)
    case connection(ConnectionStatus)
}
