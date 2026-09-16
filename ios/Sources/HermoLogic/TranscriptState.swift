import Foundation

/// One row of the session picker sheet (the phone's view of `session.list`):
/// `RemoteSessionDto` flattened into a pure value the sheet renders. Pure so the
/// logic suite can pin the title and preview fallbacks without the gateway.
public struct RemoteSessionRow: Equatable, Sendable, Identifiable {
    public let id: String
    public let title: String
    public let preview: String
    public let messageCount: Int64

    public init(id: String, title: String, preview: String, messageCount: Int64) {
        self.id = id
        self.title = title
        self.preview = preview
        self.messageCount = messageCount
    }

    /// Titlebar copy: the session's title, or "New session" when unnamed.
    public var displayTitle: String {
        title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "New session" : title
    }

    /// The one-line preview, blank when the session has no text yet.
    public var displayPreview: String {
        preview.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}

/// Applies one change-stream event to the per-key session map. A key outside
/// `knownKeys` (not an open-or-restoring tab) is dropped rather than minted,
/// including for `headerUpdated`, so a late change for a key already closed
/// via `closeTab` cannot resurrect a phantom entry.
public func applySessionChange(
    sessions: [String: SessionUiState],
    key: String,
    kind: String,
    index: Int64,
    rowJson: String,
    knownKeys: Set<String>,
    title: String = ""
) -> [String: SessionUiState] {
    guard knownKeys.contains(key) else { return sessions }
    var current = sessions[key] ?? SessionUiState(key: key)
    var updated = sessions
    if kind == "headerUpdated" {
        // An empty title means "no rename on this event" and must never blank a known title.
        if !title.isEmpty { current.title = title }
        updated[key] = current
        return updated
    }
    current.rows = applyTranscriptChange(current.rows, kind: kind, index: index, rowJson: rowJson)
    updated[key] = current
    return updated
}

/// Registers a tab's key without disturbing rows the change stream already delivered for it.
public func ensureSession(_ sessions: [String: SessionUiState], _ key: String) -> [String: SessionUiState] {
    if sessions[key] != nil { return sessions }
    var updated = sessions
    updated[key] = SessionUiState(key: key)
    return updated
}

/// Applies one transcript change to `rows`, keyed by transcript index rather
/// than list position. `rowUpdated` past the end of the list pads with empty
/// rows instead of dropping the update, which used to desynchronise every
/// index after the gap. `reset` clears the list; the core re-delivers the
/// rebuilt transcript right after as one `rowAppended` per row.
public func applyTranscriptChange(_ rows: [String], kind: String, index: Int64, rowJson: String) -> [String] {
    switch kind {
    case "rowAppended", "rowUpdated":
        // A negative index can only come from a malformed change; padding to it
        // is impossible and subscripting with it would trap.
        guard index >= 0, index <= Int64(Int.max) else { return rows }
        let i = Int(index)
        var out = rows
        while out.count <= i { out.append("") }
        out[i] = rowJson
        return out
    case "reset":
        return []
    default:
        return rows
    }
}

/// Incremental transcript parser, keyed by transcript index. `applyTranscriptChange`
/// pads gaps with empty strings, so identity has to survive on the index, not
/// on filtered list position, or every row after a gap would look like a new
/// item the moment the gap fills in.
///
/// A streaming turn rewrites one row's JSON per delta while unrelated
/// recomposition (elapsed-time tick, IME insets, focus) fires far more often,
/// so a row whose JSON string is unchanged since the last call reuses its
/// previously parsed value instead of re-parsing. Swift arrays carry no cheap
/// reference identity the way a Kotlin `List` does, so the whole-list
/// short-circuit the Kotlin version takes before this loop is dropped; the
/// per-index string comparison below is what actually saves the work, and it
/// runs unconditionally here.
public final class TranscriptRows {
    private var lastRaw: [String] = []
    private var lastParsed: [ChatRow?] = []

    public init() {}

    public func of(_ raw: [String]) -> [ChatRow] {
        var parsed: [ChatRow?] = []
        parsed.reserveCapacity(raw.count)
        for index in raw.indices {
            let json = raw[index]
            if index < lastRaw.count, lastRaw[index] == json {
                parsed.append(lastParsed[index])
            } else if json.isEmpty {
                parsed.append(nil)
            } else {
                parsed.append(parseChatRow(index, json))
            }
        }
        lastRaw = raw
        lastParsed = parsed
        return parsed.compactMap { $0 }
    }
}
