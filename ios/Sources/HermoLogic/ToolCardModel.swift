import Foundation

/// Port of `ToolCardModel.kt`, itself mirrored from Desktop's
/// `apps/desktop/src/components/assistant-ui/tool/fallback-model/`. Every formatter here
/// restates a Desktop formatter, not an invention; see the Kotlin file's header for the
/// per-function provenance.
public enum ToolCardModel {

    private static let posix = Locale(identifier: "en_US_POSIX")

    public static let maxToolRenderChars = 20_000

    /// Desktop's `formatDurationSeconds`. Empty for a negative/absent duration.
    public static func formatToolDuration(_ seconds: Double) -> String {
        if !seconds.isFinite || seconds < 0 { return "" }
        if seconds < 1 {
            let ms = max(1, Int((seconds * 1000).rounded()))
            return "\(ms)ms"
        }
        if seconds < 60 {
            if seconds >= 10 { return "\(Int(seconds))s" }
            return String(format: "%.1f", locale: posix, seconds) + "s"
        }
        let whole = Int(seconds.rounded())
        let minutes = whole / 60
        let rem = whole % 60
        if minutes < 60 {
            return rem != 0 ? "\(minutes)m \(rem)s" : "\(minutes)m"
        }
        let hours = minutes / 60
        let remMinutes = minutes % 60
        return remMinutes != 0 ? "\(hours)h \(remMinutes)m" : "\(hours)h"
    }

    /// Desktop's `formatElapsed`, the live turn timer (`42s`, then `1:05`).
    public static func formatElapsed(_ seconds: Int64) -> String {
        if seconds < 60 { return "\(seconds)s" }
        let rem = String(format: "%02lld", locale: posix, seconds % 60)
        return "\(seconds / 60):\(rem)"
    }

    /// Desktop's `compactNumber` (lib/format.ts).
    public static func compactNumber(_ value: Int64) -> String {
        let num = Double(value)
        if num <= 0 { return "0" }
        func scaled(_ v: Double, _ suffix: String) -> String {
            var text = String(format: "%.1f", locale: posix, v)
            while text.hasSuffix("0") { text.removeLast() }
            if text.hasSuffix(".") { text.removeLast() }
            return text + suffix
        }
        if num >= 999_950 { return scaled(num / 1_000_000, "M") }
        if num >= 999.5 { return scaled(num / 1_000, "k") }
        return String(Int64(num.rounded()))
    }

    /// The status strip's usage chip, Desktop's `usageContextLabel`.
    public static func usageLabel(_ usageJson: String) -> String {
        if usageJson.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return "" }
        guard let u = LooseJSON(usageJson) else { return "" }
        let contextMax = u.optLong("context_max")
        if contextMax > 0 {
            let tilde = u.optBool("context_estimated") ? "~" : ""
            return "\(tilde)\(compactNumber(u.optLong("context_used")))/\(compactNumber(contextMax))"
        }
        let total = u.optLong("total")
        return total > 0 ? "\(compactNumber(total)) tok" : ""
    }

    /// Desktop's `prettyTechnicalValue`: a JSON-looking string is pretty-printed, any other
    /// string passes through untouched.
    public static func prettyTechnicalValue(_ value: String) -> String {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.hasPrefix("{") || trimmed.hasPrefix("[") else { return value }
        guard let parsed = LooseJSON(trimmed) else { return value }
        return parsed.pretty(indent: 2)
    }

    /// Desktop's `technicalTrace`, the expanded row's payload body.
    public static func technicalTrace(argsJson: String, resultJson: String) -> String {
        var parts: [String] = []
        if !argsJson.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            parts.append("Arguments:\n" + prettyTechnicalValue(argsJson))
        }
        if !resultJson.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            parts.append("Result:\n" + prettyTechnicalValue(resultJson))
        }
        return parts.joined(separator: "\n\n")
    }

    /// Desktop's tool icon mapping (`fallback-model/index.ts` TOOL_META + PREFIX_META). Exact
    /// names first, then the `browser_`/`web_` prefix rule; anything else carries no glyph.
    public static func toolGlyph(_ name: String) -> String? {
        switch name {
        case "browser_click", "browser_fill", "browser_navigate", "browser_snapshot", "browser_type":
            return "globe"
        case "browser_take_screenshot":
            return "image"
        case "clarify":
            return "question"
        case "cronjob":
            return "watch"
        case "edit_file", "patch", "write_file":
            return "edit"
        case "execute_code", "terminal":
            return "terminal"
        case "image_generate":
            return "image"
        case "list_files":
            return "files"
        case "memory":
            return "brain"
        case "read_file":
            return "file"
        case "search_files", "session_search_recall", "web_search":
            return "search"
        case "todo":
            return "tools"
        case "vision_analyze":
            return "eye"
        case "web_extract":
            return "globe"
        default:
            if name.hasPrefix("browser_") { return "globe" }
            if name.hasPrefix("web_") { return "globe" }
            return nil
        }
    }

    /// Desktop's `titleForTool`: `web_search` → "Search", `browser_navigate` → "Navigate".
    public static func toolTitle(_ name: String) -> String {
        var stripped = name
        if stripped.hasPrefix("browser_") { stripped.removeFirst("browser_".count) }
        if stripped.hasPrefix("web_") { stripped.removeFirst("web_".count) }
        let words = stripped.split(separator: "_").filter { !$0.isEmpty }
        let title = words.map { word -> String in
            guard let first = word.first else { return String(word) }
            return first.uppercased() + word.dropFirst()
        }.joined(separator: " ")
        return title.isEmpty ? name : title
    }

    /// Desktop's status ladder (`toolStatus` + `leadingStatus`): a card still running is
    /// `running`; once complete, success is silent and only error/warning get a glyph.
    public static func toolStatus(complete: Bool) -> String {
        complete ? "done" : "running"
    }

    /// Desktop's `clampForDisplay` (`fallback-model/format.ts:72-80`): over the cap the
    /// payload is cut to exactly `max` characters and a continuation line states the omitted
    /// count. Length and slicing run over UTF-16 units, matching Kotlin's `String.length`.
    public static func clampForDisplay(_ value: String, max: Int = ToolCardModel.maxToolRenderChars) -> String {
        let units = value.utf16
        if units.count <= max { return value }
        let omitted = units.count - max
        let clamped = String(decoding: Array(units.prefix(max)), as: UTF16.self)
        return "\(clamped)\n\n… \(omitted) more characters truncated — use Copy for the full output."
    }

    /// Desktop's `stripInlineDiffChrome` (`fallback-model/index.ts:771-781`): strips ANSI SGR
    /// escape sequences and the leading `┊ review diff` header line (case-insensitive, leading
    /// blanks tolerated).
    public static func stripInlineDiffChrome(_ value: String) -> String {
        if value.isEmpty { return "" }
        let noAnsi = value.replacingOccurrences(
            of: "\u{1B}\\[[0-9;]*m",
            with: "",
            options: .regularExpression
        )
        let noHeader = noAnsi.replacingOccurrences(
            of: "^\\s*┊\\s*review diff\\s*\\n",
            with: "",
            options: [.regularExpression, .caseInsensitive]
        )
        return noHeader.trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
