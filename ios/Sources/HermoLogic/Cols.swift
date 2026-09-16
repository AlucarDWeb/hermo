public enum Cols {
    public static let min = 40
    public static let max = 80

    public static func from(widthPx: Int, density: Float, glyphDp: Float = 13) -> Int {
        if density <= 0 || glyphDp <= 0 { return max }
        let glyphs = Float(widthPx) / (density * glyphDp)
        // The bounds are compared in Float: an overflowing quotient would trap
        // the Int conversion instead of saturating the way Kotlin's toInt does.
        if glyphs.isNaN { return min }
        if glyphs >= Float(max) { return max }
        if glyphs <= Float(min) { return min }
        return Int(glyphs)
    }
}
