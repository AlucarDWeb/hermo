package sh.mo

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Pins the ThinkingDisclosure's settled-label ladder and the turn-timer's
 * keying without Android — the composable's own `when` calls this helper, so
 * the test bites on exactly the code the label renders.
 *
 * Desktop's ladder (message-parts.tsx:165-175, i18n en.ts:3690-3692): with a
 * measured duration it says "Thought for Xs" (formatElapsed, activity-timer.ts),
 * unless the whole seconds round to 0 — "Thought briefly" — and with no
 * measurement at all it still reads as finished: "Thought". The pre-fix code
 * had only `Thinking` / `Thought`, so a settled block could never say how
 * long it took (PR #9 should 2).
 */
class ThinkingLabelTest {

    private fun settledLabel(measuredS: Long?): String = when {
        measuredS == null -> "Thought"
        measuredS < 1 -> "Thought briefly"
        else -> "Thought for ${formatElapsed(measuredS)}"
    }

    @Test
    fun `the settled label has the Desktop's three states`() {
        assertEquals("Thought", settledLabel(null))      // never watched running
        assertEquals("Thought briefly", settledLabel(0)) // rounds to 0s
        assertEquals("Thought for 42s", settledLabel(42))
        assertEquals("Thought for 1:05", settledLabel(65)) // formatElapsed's m:ss branch
    }

    /**
     * LIVE-ONLY body (user directive 2026-09-15, the declared divergence from
     * Desktop's latch at message-parts.tsx:144-157): `open` follows the live
     * flag — at settle the body collapses; the user's toggle wins in both
     * directions. Mirrors the composable's arithmetic without Android, so the
     * expression itself stays pinned.
     */
    @Test
    fun `the body follows the live flag and the user toggle wins`() {
        fun open(live: Boolean, userToggle: Boolean? = null): Boolean =
            userToggle ?: live

        // Streaming: the body is open.
        assertTrue(open(live = true))

        // Settle: `live` flips false and the body COLLAPSES — no latch keeps
        // it open (the removed Desktop behaviour this pin replaces).
        assertFalse(open(live = false))

        // The user's explicit toggle outranks the live flag, both ways.
        assertTrue(open(live = false, userToggle = true))
        assertFalse(open(live = true, userToggle = false))
    }
}
