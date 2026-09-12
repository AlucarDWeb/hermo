package sh.mo

import org.junit.Assert.assertEquals
import org.junit.Test

/** Pins the cols clamp: width/13sp monospace glyph, coerced to 40..80. */
class ColsTest {

    @Test
    fun `1080p xhdpi phone lands inside the clamp`() {
        // 1080 px / 2.625 density = 411 dp; /13 = 31 -> clamped to 40.
        assertEquals(40, Cols.from(1080, 2.625f))
    }

    @Test
    fun `wide emulator screen clamps to 40 as well`() {
        // 1080/1.0 = 1080 dp; /13 = 83 -> clamped to 80.
        assertEquals(80, Cols.from(1080, 1.0f))
    }

    @Test
    fun `mid value passes through unclamped`() {
        // 800 px / 2.0 = 400 dp; /13 = 30 -> 40. Pick one in range: 1040/1.0/13=80 exactly.
        assertEquals(80, Cols.from(1040, 1.0f))
        // 520/1.0/13 = 40 exactly.
        assertEquals(40, Cols.from(520, 1.0f))
        // 650/1.0/13 = 50.
        assertEquals(50, Cols.from(650, 1.0f))
    }

    @Test
    fun `degenerate density falls back to max, never zero`() {
        assertEquals(80, Cols.from(0, 0f))
        assertEquals(80, Cols.from(1000, 0f))
    }
}
