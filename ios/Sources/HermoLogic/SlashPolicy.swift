import Foundation

public enum SlashPolicy {

    private static let localCommands: Set<String> = ["/clear", "/sessions", "/quit"]

    /// The argument stage is out of scope: a space after the leading slash hides the popup.
    public static func shouldComplete(_ draft: String) -> Bool {
        draft.hasPrefix("/") && !draft.contains(" ")
    }

    /// `replaceFrom` is 1-based; a missing or negative value means replace the whole token.
    /// The position counts UTF-16 units, the unit the gateway's own `text.slice` uses.
    public static func insertCompletion(_ draft: String, _ itemText: String, _ replaceFrom: Int64) -> String {
        let from = replaceFrom < 1 ? 1 : replaceFrom
        let units = draft.utf16
        let keepEnd = min(max(Int(clamping: from) - 1, 0), units.count)
        let prefix = String(decoding: Array(units.prefix(keepEnd)), as: UTF16.self)
        return prefix + itemText + " "
    }

    /// The match is the whole trimmed token, so "/clears" or "/clear now" is not a local command.
    public static func isLocalCommand(_ draft: String) -> Bool {
        localCommands.contains(draft.trimmingCharacters(in: .whitespacesAndNewlines))
    }

    public static func isSlashSubmit(_ draft: String) -> Bool {
        draft.hasPrefix("/")
    }
}

public struct SlashCompletionRow: Equatable, Sendable {
    public let text: String
    public let display: String
    public let kind: String
    public let meta: String

    public init(text: String, display: String, kind: String, meta: String) {
        self.text = text
        self.display = display
        self.kind = kind
        self.meta = meta
    }
}
