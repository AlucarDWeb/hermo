import Foundation

public struct MarkdownBlock: Equatable, Sendable {
    public var language: String
    public var text: String
    public var isFence: Bool
    public var open: Bool

    public init(language: String = "", text: String, isFence: Bool = false, open: Bool = false) {
        self.language = language
        self.text = text
        self.isFence = isFence
        self.open = open
    }
}

public enum MarkdownBlocks {

    private static let minFence = 3

    public static func split(_ text: String) -> [MarkdownBlock] {
        if text.isEmpty { return [] }
        var blocks: [MarkdownBlock] = []
        var current: String?
        var currentLanguage = ""
        var currentIsFence = false
        var open = false
        var fenceTicks = 0
        var fenceTildes = 0

        func flush() {
            if let current {
                blocks.append(MarkdownBlock(language: currentLanguage, text: current, isFence: currentIsFence, open: open && currentIsFence))
            }
            current = nil
        }

        func appendLine(_ line: String) {
            // Appending through the optional keeps one buffer: a local copy would
            // reallocate the whole block on every line of a streamed fence.
            if current == nil { current = "" }
            if !current!.isEmpty { current!.append("\n") }
            current!.append(line)
        }

        for line in splitLines(text) {
            let lineText = String(line)
            let trimmed = lineText.trimmingCharacters(in: .whitespaces)
            if let marker = fenceMarker(trimmed) {
                if fenceTicks == 0 && fenceTildes == 0 {
                    flush()
                    if marker.isTick { fenceTicks = marker.length } else { fenceTildes = marker.length }
                    let infoStart = trimmed.index(trimmed.startIndex, offsetBy: marker.length)
                    currentLanguage = String(trimmed[infoStart...]).trimmingCharacters(in: .whitespaces)
                    currentIsFence = true
                    open = true
                    current = ""
                } else {
                    let closes = (marker.isTick && fenceTicks > 0 && marker.length >= fenceTicks) ||
                        (!marker.isTick && fenceTildes > 0 && marker.length >= fenceTildes)
                    if closes {
                        flush()
                        fenceTicks = 0
                        fenceTildes = 0
                        currentIsFence = false
                        open = false
                        currentLanguage = ""
                    } else {
                        appendLine(lineText)
                    }
                }
                continue
            }
            if current == nil {
                currentIsFence = false
                open = false
                currentLanguage = ""
                current = ""
            }
            appendLine(lineText)
        }
        flush()
        return blocks
    }

    /// Mirror of the core's `fence_marker`: a run of three or more backticks or tildes, with no further one of the same kind in the rest of the line.
    private static func fenceMarker(_ line: String) -> (length: Int, isTick: Bool)? {
        let chars = Array(line)
        var ticks = 0
        while ticks < chars.count && chars[ticks] == "`" { ticks += 1 }
        if ticks >= minFence {
            let info = chars[ticks...]
            return info.contains("`") ? nil : (ticks, true)
        }
        var tildes = 0
        while tildes < chars.count && chars[tildes] == "~" { tildes += 1 }
        if tildes >= minFence {
            let info = chars[tildes...]
            if !info.contains("~") { return (tildes, false) }
        }
        return nil
    }
}

/// Mirror of Kotlin's `String.lines()`: splits on CRLF, LF or CR, keeping a trailing empty line when the text ends with a terminator.
private func splitLines(_ text: String) -> [Substring] {
    var lines: [Substring] = []
    var start = text.startIndex
    var index = text.startIndex
    while index < text.endIndex {
        let char = text[index]
        if char == "\n" {
            lines.append(text[start..<index])
            index = text.index(after: index)
            start = index
        } else if char == "\r" {
            lines.append(text[start..<index])
            let next = text.index(after: index)
            index = (next < text.endIndex && text[next] == "\n") ? text.index(after: next) : next
            start = index
        } else {
            index = text.index(after: index)
        }
    }
    lines.append(text[start..<text.endIndex])
    return lines
}
