import Foundation

/// The chat screen's key resolution: the repository-owned `currentKey` wins
/// whenever it is set. Only when it is nil does the screen fall back, and
/// then to the map's one session with rows, never to `sessions.keys.first`,
/// which used to hand the view whichever phantom (rowless) entry happened to
/// sort first and made the transcript appear to vanish mid-turn.
public func resolveSessionKey(currentKey: String?, sessions: [String: SessionUiState]) -> String? {
    if let currentKey { return currentKey }
    let withRows = sessions.filter { !$0.value.rows.isEmpty }
    guard withRows.count == 1 else { return nil }
    return withRows.first?.key
}
