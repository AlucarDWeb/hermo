package sh.mo

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.Locale

/**
 * Pins the T8 tool-card/thinking/status-strip formatting, each formatter
 * mirrored from the Desktop's own module (the provenance is in the header of
 * ToolCardModel.kt). These fail on anything that invents its own format —
 * e.g. the T7a-era `"%.1fs"` for every duration, which turned 0.4s into
 * "0.4s" (fine) but 45s into "45.0s" (Desktop prints "45s") and 90s into
 * "1.5m" (Desktop prints "1m 30s").
 */
class ToolCardModelTest {

    @Test
    fun `tool durations read like the Desktop's formatDurationSeconds`() {
        // <1s prints milliseconds (format.ts: Math.max(1, round(seconds*1000)))
        assertEquals("400ms", formatToolDuration(0.4))
        assertEquals("500ms", formatToolDuration(0.5))
        assertEquals("1ms", formatToolDuration(0.0001))
        assertEquals("1.5s", formatToolDuration(1.5))
        assertEquals("12s", formatToolDuration(12.0))
        assertEquals("9.8s", formatToolDuration(9.84))
        assertEquals("45s", formatToolDuration(45.0))
        assertEquals("1m 30s", formatToolDuration(90.0))
        assertEquals("2m", formatToolDuration(120.0))
        assertEquals("", formatToolDuration(-1.0))
    }

    @Test
    fun `the icon mapping is the Desktop's TOOL_META and PREFIX_META`() {
        assertEquals("terminal", toolGlyph("terminal"))
        assertEquals("terminal", toolGlyph("execute_code"))
        assertEquals("edit", toolGlyph("write_file"))
        assertEquals("edit", toolGlyph("patch"))
        assertEquals("file", toolGlyph("read_file"))
        assertEquals("search", toolGlyph("search_files"))
        assertEquals("search", toolGlyph("web_search"))
        assertEquals("globe", toolGlyph("web_extract"))
        assertEquals("brain", toolGlyph("memory"))
        assertEquals("eye", toolGlyph("vision_analyze"))
        // prefix fallbacks
        assertEquals("globe", toolGlyph("browser_click"))
        assertEquals("globe", toolGlyph("browser_anything_new"))
        assertEquals("globe", toolGlyph("web_custom_tool"))
        // unknown tool: no invented glyph
        assertNull(toolGlyph("future_tool"))
    }

    @Test
    fun `tool titles are the Desktop's titleForTool`() {
        assertEquals("Search", toolTitle("web_search"))
        assertEquals("Navigate", toolTitle("browser_navigate"))
        assertEquals("Read File", toolTitle("read_file"))
        assertEquals("Terminal", toolTitle("terminal"))
    }

    @Test
    fun `technical trace pretty-prints JSON-looking payloads and passes the rest through`() {
        // pretty-printed object (Desktop: JSON.stringify(parsed, null, 2))
        val trace = technicalTrace("""{"path":"/tmp/x","limit":5}""", "plain output")
        assertTrue(trace.contains("Arguments:"))
        assertTrue(trace.contains("Result:"))
        assertTrue(trace.contains("\"path\": \"/tmp/x\"")) // pretty-printed: space after colon
        assertTrue(trace.endsWith("plain output"))
        // non-JSON strings pass through untouched
        assertEquals("just text", prettyTechnicalValue("just text"))
        // malformed JSON-looking string is returned as-is, never thrown
        assertEquals("{broken", prettyTechnicalValue("{broken"))
    }

    @Test
    fun `usage label is the Desktop's usageContextLabel`() {
        assertEquals("", usageLabel(""))
        assertEquals("", usageLabel("not json"))
        assertEquals("1.2k tok", usageLabel("""{"total":1234}"""))
        assertEquals("12.3k/200k", usageLabel("""{"context_used":12340,"context_max":200000}"""))
        assertEquals("~12.3k/200k", usageLabel("""{"context_used":12340,"context_max":200000,"context_estimated":true}"""))
        assertEquals("", usageLabel("""{"total":0}"""))
    }

    @Test
    fun `elapsed reads like the Desktop's formatElapsed`() {
        assertEquals("42s", formatElapsed(42))
        assertEquals("1:05", formatElapsed(65))
    }

    /**
     * The hours branch (format.ts:145-152) was missing on the phone: 3760s
     * printed "62m 40s" where the Desktop prints "1h 2m". Pinned at the
     * boundaries, not just inside the branch, so any re-derivation of the
     * ladder shows up here.
     */
    @Test
    fun `tool durations roll over into the Desktop's hours branch`() {
        assertEquals("59m 59s", formatToolDuration(3599.0)) // still minutes
        assertEquals("1h", formatToolDuration(3600.0))      // exact hour
        assertEquals("1h", formatToolDuration(3600.4))      // rounds to 3600
        assertEquals("1h 1m", formatToolDuration(3660.0))   // rem minutes
        assertEquals("1h 2m", formatToolDuration(3760.0))   // the reviewer's own case
        assertEquals("2h", formatToolDuration(7200.0))      // exact hours
        assertEquals("2h 1m", formatToolDuration(7260.0))
    }

    /**
     * Desktop's clampForDisplay (format.ts:72-80): over 20 000 chars the
     * payload is cut to the cap and the omitted count is stated — stacked
     * unclamped tool rows froze the Desktop renderer (format.ts:66-69).
     */
    @Test
    fun `expanded payload clamps at the Desktop's 20 000 chars`() {
        val under = "x".repeat(MAX_TOOL_RENDER_CHARS)
        assertEquals(under, clampForDisplay(under)) // exactly at the cap: untouched
        val big = "a".repeat(25_000) + "TAIL"
        val clamped = clampForDisplay(big)
        assertTrue(clamped.startsWith("a".repeat(100))) // cut, not emptied
        assertEquals(20_000, clamped.indexOf("\n\n"))
        assertEquals("… 5004 more characters truncated — use Copy for the full output.", clamped.substring(clamped.indexOf("… ")))
    }

    /** Desktop's stripInlineDiffChrome (index.ts:771-781): ANSI + header line. */
    @Test
    fun `inline diff chrome the Desktop strips is stripped here too`() {
        // ANSI SGR sequences, including combined codes, disappear…
        assertEquals("--- a\n+++ b\n+hi", stripInlineDiffChrome("\u001B[1;32m--- a\u001B[0m\n\u001B[31m+++ b\u001B[0m\n+hi"))
        // …so does the leading `┊ review diff` header line (case-insensitive)…
        assertEquals("--- a\n+hi", stripInlineDiffChrome("┊ Review Diff\n--- a\n+hi"))
        // …and both at once, with surrounding whitespace trimmed like Desktop's.
        assertEquals("+hi", stripInlineDiffChrome("\u001B[36m┊ review diff\u001B[0m\n\n+hi\n"))
        // no chrome: the text passes through, only trimmed
        assertEquals("--- a\n+hi", stripInlineDiffChrome("--- a\n+hi"))
        // empty stays empty
        assertEquals("", stripInlineDiffChrome(""))
    }

    /**
     * The two composes the ToolCard's expanded shell actually performs — a
     * payload run through technicalTrace then clamped, and a raw diff run
     * through stripInlineDiffChrome — so the UI cannot wire the wrong order
     * (clamping before pretty-printing) or forget the compose entirely.
     */
    @Test
    fun `the ToolCard's composes clamp the trace and strip the diff`() {
        val huge = "{\"x\":\"" + "y".repeat(30_000) + "\"}"
        val trace = clampForDisplay(technicalTrace(huge, ""))
        assertTrue(trace.startsWith("Arguments:"))
        assertTrue(trace.contains("more characters truncated"))
        assertTrue(trace.length < 25_000)
        val diff = stripInlineDiffChrome("\u001B[32m┊ review diff\u001B[0m\n┊--- a\n+hi")
        assertTrue(diff.startsWith("┊--- a"))
        assertFalse(diff.contains("\u001B"))
    }

    @Test
    fun `row parsing carries the T8 payload fields through`() {
        val tool = parseChatRow(
            0,
            """{"kind":"tool","tool_id":"t1","name":"terminal","complete":true,
               "args":{"command":"ls"},"result":{"exit_code":0,"output":"x"},
               "duration_s":12.0}""",
        ) as ChatRow.Tool
        assertTrue(tool.complete)
        assertTrue(tool.argsJson.contains("command"))
        assertTrue(tool.resultJson.contains("exit_code"))
        assertEquals("", tool.inlineDiff)
        assertEquals(12.0, tool.durationS, 0.001)

        // inline_diff hides under result (Desktop's inlineDiffFromResult)
        val diff = parseChatRow(
            1,
            """{"kind":"tool","name":"patch","complete":true,
               "result":{"inline_diff":"--- a\n+++ b\n+hi"},"duration_s":1.0}""",
        ) as ChatRow.Tool
        assertTrue(diff.inlineDiff.startsWith("---"))

        // assistant usage crosses untyped
        val assistant = parseChatRow(
            2,
            """{"kind":"assistant","text":"ok","streaming":false,
               "usage":{"total":800,"context_used":700,"context_max":1000}}""",
        ) as ChatRow.Assistant
        assertTrue(assistant.usageJson.contains("context_max"))
    }

    @Test
    fun `garbage tool rows degrade instead of throwing`() {
        val row = parseChatRow(0, """{"kind":"tool","name":"x","args":12,"result":[1,2]}""") as ChatRow.Tool
        // args/result of non-object type cross as their JSON text (untyped per PLAN §3)
        assertEquals("12", row.argsJson)
        assertTrue(row.resultJson.isNotEmpty())
        assertFalse(row.complete)
    }

    /**
     * The formats are the Desktop's, which uses `toFixed(1)` — always a dot.
     * A `"%.1f".format(…)` picks up the JVM default locale instead, so on an
     * it_IT device the status chip read `17,7k/1,M` (device evidence, T7b) and
     * a 0.5s tool showed `0,5s`. The suite runs under C.UTF-8 and would have
     * stayed green forever; asserting under Italy is what makes it bite.
     */
    @Test
    fun `formats stay locale-independent like the Desktop`() {
        val previous = Locale.getDefault()
        try {
            Locale.setDefault(Locale.ITALY)
            assertEquals("12.3k/200k", usageLabel("""{"context_used":12340,"context_max":200000}"""))
            assertEquals("1M", compactNumber(1_000_000))
            assertEquals("17.7k", compactNumber(17_700))
            assertEquals("1.5s", formatToolDuration(1.5))
            assertEquals("9.8s", formatToolDuration(9.84))
        } finally {
            Locale.setDefault(previous)
        }
    }
}
