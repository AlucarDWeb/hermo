package sh.mo

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/**
 * T16b: the ordered tab set behind the session strip (pure entity, JVM).
 *
 * The tests pin the behaviours that TODAY'S repository gets wrong: a single
 * `currentKey` with no ordered set, `open_session` re-issued for a key that
 * is already open (which rebuilds the LiveSession and RESET-clears the
 * transcript), and a transcript change for an unknown key inventing state.
 * Every assertion here would fail against that code shape.
 */
class SessionTabsTest {

    // ── select ──────────────────────────────────────────────────────────

    @Test
    fun `select of a key not in the set is a no-op`() {
        val tabs = TabSet(keys = listOf("a", "b"), current = "a")
        val next = tabs.select("zzz")
        assertEquals(listOf("a", "b"), next.keys)
        assertEquals("a", next.current)
    }

    @Test
    fun `select of an open key only moves current`() {
        val tabs = TabSet(keys = listOf("a", "b", "c"), current = "a")
        val next = tabs.select("c")
        assertEquals(listOf("a", "b", "c"), next.keys)
        assertEquals("c", next.current)
    }

    // ── add ─────────────────────────────────────────────────────────────

    @Test
    fun `add of a new key appends and selects it`() {
        val tabs = TabSet(keys = listOf("a"), current = "a")
        val next = tabs.add("b")
        assertEquals(listOf("a", "b"), next.keys)
        assertEquals("b", next.current)
    }

    @Test
    fun `add of an already-open key only selects it — picker-already-open`() {
        val tabs = TabSet(keys = listOf("a", "b"), current = "a")
        val next = tabs.add("b")
        // Re-adding must NOT grow the list (the old code called open_session
        // again: list grew in the registry and the transcript RESET).
        assertEquals(listOf("a", "b"), next.keys)
        assertEquals("b", next.current)
    }

    // ── close ───────────────────────────────────────────────────────────

    @Test
    fun `close of the last remaining tab is a no-op`() {
        val tabs = TabSet(keys = listOf("only"), current = "only")
        val next = tabs.close("only")
        assertEquals(listOf("only"), next.keys)
        assertEquals("only", next.current)
    }

    @Test
    fun `close of an unknown key is a no-op`() {
        val tabs = TabSet(keys = listOf("a", "b"), current = "b")
        val next = tabs.close("zzz")
        assertEquals(listOf("a", "b"), next.keys)
        assertEquals("b", next.current)
    }

    @Test
    fun `close of a middle current tab selects the left neighbor`() {
        val tabs = TabSet(keys = listOf("a", "b", "c"), current = "b")
        val next = tabs.close("b")
        assertEquals(listOf("a", "c"), next.keys)
        assertEquals("a", next.current)
    }

    @Test
    fun `close of index 0 selects the new first`() {
        val tabs = TabSet(keys = listOf("a", "b", "c"), current = "a")
        val next = tabs.close("a")
        assertEquals(listOf("b", "c"), next.keys)
        assertEquals("b", next.current)
    }

    @Test
    fun `close of a non-current tab keeps current`() {
        val tabs = TabSet(keys = listOf("a", "b", "c"), current = "c")
        val next = tabs.close("a")
        assertEquals(listOf("b", "c"), next.keys)
        assertEquals("c", next.current)
    }

    // ── restorePlan (T5: tabs survive restart) ──────────────────────────

    @Test
    fun `restorePlan resumes every key in order — not just lastActive`() {
        // RED against a restore that only returns lastActive and drops the
        // rest: resumeKeys must carry the WHOLE registry order.
        val plan = restorePlan(keys = listOf("a", "b", "c"), lastActive = "b")
        assertEquals(listOf("a", "b", "c"), plan.resumeKeys)
        assertEquals("b", plan.current)
    }

    @Test
    fun `restorePlan falls back to the last key when lastActive is absent`() {
        val plan = restorePlan(keys = listOf("a", "b"), lastActive = null)
        assertEquals(listOf("a", "b"), plan.resumeKeys)
        assertEquals("b", plan.current)
    }

    @Test
    fun `restorePlan ignores a lastActive that is not in the registry`() {
        val plan = restorePlan(keys = listOf("a", "b"), lastActive = "ghost")
        assertEquals(listOf("a", "b"), plan.resumeKeys)
        assertEquals("b", plan.current)
    }

    @Test
    fun `restorePlan with an empty registry is an empty plan — caller mints`() {
        val plan = restorePlan(keys = emptyList(), lastActive = null)
        assertEquals(emptyList<String>(), plan.resumeKeys)
        assertNull(plan.current)
    }
}
