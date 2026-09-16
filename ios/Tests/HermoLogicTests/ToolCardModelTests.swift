import XCTest
import HermoLogic

final class ToolCardModelTests: XCTestCase {

    func testToolDurationsReadLikeTheDesktopsFormatDurationSeconds() {
        // <1s prints milliseconds (format.ts: Math.max(1, round(seconds*1000)))
        XCTAssertEqual("400ms", ToolCardModel.formatToolDuration(0.4))
        XCTAssertEqual("500ms", ToolCardModel.formatToolDuration(0.5))
        XCTAssertEqual("1ms", ToolCardModel.formatToolDuration(0.0001))
        XCTAssertEqual("1.5s", ToolCardModel.formatToolDuration(1.5))
        XCTAssertEqual("12s", ToolCardModel.formatToolDuration(12.0))
        XCTAssertEqual("9.8s", ToolCardModel.formatToolDuration(9.84))
        XCTAssertEqual("45s", ToolCardModel.formatToolDuration(45.0))
        XCTAssertEqual("1m 30s", ToolCardModel.formatToolDuration(90.0))
        XCTAssertEqual("2m", ToolCardModel.formatToolDuration(120.0))
        XCTAssertEqual("", ToolCardModel.formatToolDuration(-1.0))
    }

    func testTheIconMappingIsTheDesktopsToolMetaAndPrefixMeta() {
        XCTAssertEqual("terminal", ToolCardModel.toolGlyph("terminal"))
        XCTAssertEqual("terminal", ToolCardModel.toolGlyph("execute_code"))
        XCTAssertEqual("edit", ToolCardModel.toolGlyph("write_file"))
        XCTAssertEqual("edit", ToolCardModel.toolGlyph("patch"))
        XCTAssertEqual("file", ToolCardModel.toolGlyph("read_file"))
        XCTAssertEqual("search", ToolCardModel.toolGlyph("search_files"))
        XCTAssertEqual("search", ToolCardModel.toolGlyph("web_search"))
        XCTAssertEqual("globe", ToolCardModel.toolGlyph("web_extract"))
        XCTAssertEqual("brain", ToolCardModel.toolGlyph("memory"))
        XCTAssertEqual("eye", ToolCardModel.toolGlyph("vision_analyze"))
        // prefix fallbacks
        XCTAssertEqual("globe", ToolCardModel.toolGlyph("browser_click"))
        XCTAssertEqual("globe", ToolCardModel.toolGlyph("browser_anything_new"))
        XCTAssertEqual("globe", ToolCardModel.toolGlyph("web_custom_tool"))
        // unknown tool: no invented glyph
        XCTAssertNil(ToolCardModel.toolGlyph("future_tool"))
    }

    func testToolTitlesAreTheDesktopsTitleForTool() {
        XCTAssertEqual("Search", ToolCardModel.toolTitle("web_search"))
        XCTAssertEqual("Navigate", ToolCardModel.toolTitle("browser_navigate"))
        XCTAssertEqual("Read File", ToolCardModel.toolTitle("read_file"))
        XCTAssertEqual("Terminal", ToolCardModel.toolTitle("terminal"))
    }

    func testTechnicalTracePrettyPrintsJsonLookingPayloadsAndPassesTheRestThrough() {
        // pretty-printed object (Desktop: JSON.stringify(parsed, null, 2))
        let trace = ToolCardModel.technicalTrace(argsJson: #"{"path":"/tmp/x","limit":5}"#, resultJson: "plain output")
        XCTAssertTrue(trace.contains("Arguments:"))
        XCTAssertTrue(trace.contains("Result:"))
        XCTAssertTrue(trace.contains(#""path": "/tmp/x""#)) // pretty-printed: space after colon
        XCTAssertTrue(trace.hasSuffix("plain output"))
        // non-JSON strings pass through untouched
        XCTAssertEqual("just text", ToolCardModel.prettyTechnicalValue("just text"))
        // malformed JSON-looking string is returned as-is, never thrown
        XCTAssertEqual("{broken", ToolCardModel.prettyTechnicalValue("{broken"))
    }

    func testUsageLabelIsTheDesktopsUsageContextLabel() {
        XCTAssertEqual("", ToolCardModel.usageLabel(""))
        XCTAssertEqual("", ToolCardModel.usageLabel("not json"))
        XCTAssertEqual("1.2k tok", ToolCardModel.usageLabel(#"{"total":1234}"#))
        XCTAssertEqual("12.3k/200k", ToolCardModel.usageLabel(#"{"context_used":12340,"context_max":200000}"#))
        XCTAssertEqual(
            "~12.3k/200k",
            ToolCardModel.usageLabel(#"{"context_used":12340,"context_max":200000,"context_estimated":true}"#)
        )
        XCTAssertEqual("", ToolCardModel.usageLabel(#"{"total":0}"#))
    }

    func testElapsedReadsLikeTheDesktopsFormatElapsed() {
        XCTAssertEqual("42s", ToolCardModel.formatElapsed(42))
        XCTAssertEqual("1:05", ToolCardModel.formatElapsed(65))
    }

    /// The hours branch (format.ts:145-152) was missing on the phone: 3760s printed "62m 40s"
    /// where the Desktop prints "1h 2m". Pinned at the boundaries, not just inside the branch.
    func testToolDurationsRollOverIntoTheDesktopsHoursBranch() {
        XCTAssertEqual("59m 59s", ToolCardModel.formatToolDuration(3599.0)) // still minutes
        XCTAssertEqual("1h", ToolCardModel.formatToolDuration(3600.0))      // exact hour
        XCTAssertEqual("1h", ToolCardModel.formatToolDuration(3600.4))      // rounds to 3600
        XCTAssertEqual("1h 1m", ToolCardModel.formatToolDuration(3660.0))   // rem minutes
        XCTAssertEqual("1h 2m", ToolCardModel.formatToolDuration(3760.0))   // the reviewer's own case
        XCTAssertEqual("2h", ToolCardModel.formatToolDuration(7200.0))      // exact hours
        XCTAssertEqual("2h 1m", ToolCardModel.formatToolDuration(7260.0))
    }

    /// Desktop's clampForDisplay (format.ts:72-80): over 20 000 chars the payload is cut to
    /// the cap and the omitted count is stated.
    func testExpandedPayloadClampsAtTheDesktops20000Chars() {
        let under = String(repeating: "x", count: ToolCardModel.maxToolRenderChars)
        XCTAssertEqual(under, ToolCardModel.clampForDisplay(under)) // exactly at the cap: untouched
        let big = String(repeating: "a", count: 25_000) + "TAIL"
        let clamped = ToolCardModel.clampForDisplay(big)
        XCTAssertTrue(clamped.hasPrefix(String(repeating: "a", count: 100))) // cut, not emptied
        let ns = clamped as NSString
        XCTAssertEqual(20_000, ns.range(of: "\n\n").location)
        let ellipsisAt = ns.range(of: "… ").location
        XCTAssertEqual(
            "… 5004 more characters truncated — use Copy for the full output.",
            ns.substring(from: ellipsisAt)
        )
    }

    /// Desktop's stripInlineDiffChrome (index.ts:771-781): ANSI + header line.
    func testInlineDiffChromeTheDesktopStripsIsStrippedHereToo() {
        // ANSI SGR sequences, including combined codes, disappear…
        XCTAssertEqual(
            "--- a\n+++ b\n+hi",
            ToolCardModel.stripInlineDiffChrome("\u{1B}[1;32m--- a\u{1B}[0m\n\u{1B}[31m+++ b\u{1B}[0m\n+hi")
        )
        // …so does the leading `┊ review diff` header line (case-insensitive)…
        XCTAssertEqual("--- a\n+hi", ToolCardModel.stripInlineDiffChrome("┊ Review Diff\n--- a\n+hi"))
        // …and both at once, with surrounding whitespace trimmed like Desktop's.
        XCTAssertEqual(
            "+hi",
            ToolCardModel.stripInlineDiffChrome("\u{1B}[36m┊ review diff\u{1B}[0m\n\n+hi\n")
        )
        // no chrome: the text passes through, only trimmed
        XCTAssertEqual("--- a\n+hi", ToolCardModel.stripInlineDiffChrome("--- a\n+hi"))
        // empty stays empty
        XCTAssertEqual("", ToolCardModel.stripInlineDiffChrome(""))
    }

    /// The two composes the ToolCard's expanded shell actually performs: a payload run through
    /// technicalTrace then clamped, and a raw diff run through stripInlineDiffChrome, so the UI
    /// cannot wire the wrong order or forget the compose entirely.
    func testTheToolCardsComposesClampTheTraceAndStripTheDiff() {
        let huge = "{\"x\":\"" + String(repeating: "y", count: 30_000) + "\"}"
        let trace = ToolCardModel.clampForDisplay(ToolCardModel.technicalTrace(argsJson: huge, resultJson: ""))
        XCTAssertTrue(trace.hasPrefix("Arguments:"))
        XCTAssertTrue(trace.contains("more characters truncated"))
        XCTAssertTrue(trace.utf16.count < 25_000)
        let diff = ToolCardModel.stripInlineDiffChrome("\u{1B}[32m┊ review diff\u{1B}[0m\n┊--- a\n+hi")
        XCTAssertTrue(diff.hasPrefix("┊--- a"))
        XCTAssertFalse(diff.contains("\u{1B}"))
    }

    /// The formats are the Desktop's, which uses `toFixed(1)`, always a dot. A locale-sensitive
    /// formatter would pick up the device's own decimal separator instead (an it_IT device read
    /// the status chip as "17,7k/1,M" and a 0.5s tool as "0,5s" — device evidence, T7b).
    func testFormatsStayLocaleIndependentLikeTheDesktop() {
        // Prove the Italian locale really does use a comma decimal separator, so the
        // assertions below are not vacuous.
        let italian = Locale(identifier: "it_IT")
        XCTAssertEqual("1,5", String(format: "%.1f", locale: italian, 1.5))

        XCTAssertEqual("12.3k/200k", ToolCardModel.usageLabel(#"{"context_used":12340,"context_max":200000}"#))
        XCTAssertEqual("1M", ToolCardModel.compactNumber(1_000_000))
        XCTAssertEqual("17.7k", ToolCardModel.compactNumber(17_700))
        XCTAssertEqual("1.5s", ToolCardModel.formatToolDuration(1.5))
        XCTAssertEqual("9.8s", ToolCardModel.formatToolDuration(9.84))
    }
}
