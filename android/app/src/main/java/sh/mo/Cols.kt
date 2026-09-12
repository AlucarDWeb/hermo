package sh.mo

/**
 * Pure helper: terminal columns for `open_session` (PLAN §4 T6 item 7) —
 * screen width in px divided by the 13sp monospace glyph advance, clamped
 * to 40..80. No Android imports: density comes in as a plain number.
 */
object Cols {
    const val MIN = 40
    const val MAX = 80

    /**
     * @param widthPx   usable width of the transcript area in pixels
     * @param density   display density (px per dp)
     * @param glyphDp   monospace glyph advance in dp (13sp at default font scale)
     */
    fun from(widthPx: Int, density: Float, glyphDp: Float = 13f): Int {
        val glyphs = if (density <= 0f || glyphDp <= 0f) {
            MAX
        } else {
            (widthPx / (density * glyphDp)).toInt()
        }
        return glyphs.coerceIn(MIN, MAX)
    }
}
