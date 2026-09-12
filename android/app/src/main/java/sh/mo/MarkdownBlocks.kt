package sh.mo

/**
 * App-side mirror of the core's `split_blocks` grammar (hermes_core
 * `transcript/markdown.rs`), so the UI can split text for rendering without
 * an FFI round-trip per keystroke. The rules below are the core's, restated:
 * a fence line is a run of ≥3 backticks or ≥3 tildes (optionally with an
 * info string, no further back/tilde in it); an inner run SHORTER than the
 * opening fence stays in the text; the final unterminated fence stays `open`
 * (streaming never drops the code being written); plain text is its own
 * block. Pure Kotlin — JVM-testable, pinned by MarkdownBlocksTest.
 */
data class MarkdownBlock(
    val language: String = "",
    val text: String,
    val isFence: Boolean = false,
    val open: Boolean = false,
)

object MarkdownBlocks {

    private const val MIN_FENCE = 3

    fun split(text: String): List<MarkdownBlock> {
        if (text.isEmpty()) return emptyList()
        val blocks = mutableListOf<MarkdownBlock>()
        var current: StringBuilder? = null
        var currentLanguage = ""
        var currentIsFence = false
        var open = false
        var fenceTicks = 0
        var fenceTildes = 0

        fun flush() {
            current?.let {
                blocks.add(MarkdownBlock(currentLanguage, it.toString(), currentIsFence, open && currentIsFence))
            }
            current = null
        }

        for (line in text.lines()) {
            val trimmed = line.trim()
            val marker = fenceMarker(trimmed)
            if (marker != null) {
                val (len, isTick) = marker
                if (fenceTicks == 0 && fenceTildes == 0) {
                    flush()
                    if (isTick) fenceTicks = len else fenceTildes = len
                    currentLanguage = trimmed.substring(len).trim()
                    currentIsFence = true
                    open = true
                    current = StringBuilder()
                } else {
                    val closes = (isTick && fenceTicks > 0 && len >= fenceTicks) ||
                        (!isTick && fenceTildes > 0 && len >= fenceTildes)
                    if (closes) {
                        flush()
                        fenceTicks = 0
                        fenceTildes = 0
                        currentIsFence = false
                        open = false
                        currentLanguage = ""
                    } else {
                        appendLine(current!!, line)
                    }
                }
                continue
            }
            if (current == null) {
                currentIsFence = false
                open = false
                currentLanguage = ""
                current = StringBuilder()
            }
            appendLine(current!!, line)
        }
        flush()
        return blocks
    }

    private fun appendLine(sb: StringBuilder, line: String) {
        if (sb.isNotEmpty()) sb.append('\n')
        sb.append(line)
    }

    /** Mirror of the core's `fence_marker`. */
    private fun fenceMarker(line: String): Pair<Int, Boolean>? {
        var ticks = 0
        while (ticks < line.length && line[ticks] == '`') ticks++
        if (ticks >= MIN_FENCE) {
            val info = line.substring(ticks)
            if (!info.contains('`')) return Pair(ticks, true)
            return null
        }
        var tildes = 0
        while (tildes < line.length && line[tildes] == '~') tildes++
        if (tildes >= MIN_FENCE) {
            val info = line.substring(tildes)
            if (!info.contains('~')) return Pair(tildes, false)
        }
        return null
    }
}
