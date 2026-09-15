package sh.mo

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * T16c: the bot drawer's pure entity (JVM). Every assertion pins a
 * behaviour that fails on the broken version, stated in a one-line comment.
 */
class BotDrawerTest {

    // ── mapping (botDrawerRow) ──────────────────────────────────────────

    @Test
    fun `mapping keeps the profile key equal to the name`() {
        // Broken version: `profile` taken from a display label or left null —
        // the tap would open the wrong (or a second) identity.
        val row = botDrawerRow("jn-core", "grok-4.6", "Core fixes bot")
        assertEquals("jn-core", row.name)
        assertEquals("jn-core", row.profile)
        assertEquals("grok-4.6", row.model)
        assertEquals("Core fixes bot", row.description)
    }

    @Test
    fun `mapping keeps empty model and description empty — nothing invented`() {
        // Broken version: substituting "Unknown model" / "—", or dropping the
        // row entirely, for a profile the gateway describes incompletely.
        val row = botDrawerRow("default", "", "")
        assertEquals("", row.model)
        assertEquals("", row.description)
        assertEquals("default", row.name)
        assertEquals("default", row.profile)
    }

    // ── load outcome (onProfilesLoaded) ─────────────────────────────────

    @Test
    fun `a successful load reduces to Ready with every row`() {
        // Broken version: filtering or reordering the core's answer — the
        // wire has NO hidden flag, the drawer shows ALL profiles.
        val rows = listOf(
            botDrawerRow("default", "", "Launch profile"),
            botDrawerRow("jn-review", "grok-4.6", ""),
        )
        val next = DrawerUiState.Loading.onProfilesLoaded(rows, message = "unused")
        assertTrue(next is DrawerUiState.Ready)
        assertEquals(rows, (next as DrawerUiState.Ready).rows)
    }

    @Test
    fun `a failed RPC is Failed — never a silent empty drawer`() {
        // Broken version: treating null (failure) as emptyList() and painting
        // Ready(emptyList()) — the error row with a retry never appeared.
        val next = DrawerUiState.Loading.onProfilesLoaded(null, message = "gateway unreachable")
        assertTrue(next is DrawerUiState.Failed)
        assertEquals("gateway unreachable", (next as DrawerUiState.Failed).message)
    }

    @Test
    fun `a successful EMPTY list is Ready — a legit gateway with no profiles`() {
        // Broken version: mapping an empty answer to Failed conflates "the
        // RPC died" with "the gateway has zero profiles".
        val next = DrawerUiState.Loading.onProfilesLoaded(emptyList(), message = "unused")
        assertTrue(next is DrawerUiState.Ready)
        assertTrue((next as DrawerUiState.Ready).rows.isEmpty())
    }

    // ── retry (onRetry) ─────────────────────────────────────────────────

    @Test
    fun `retry from Failed goes back to Loading — no stale rows`() {
        // Broken version: a no-op retry that kept Failed on screen forever,
        // or one that "retried" into Ready with the previous stale rows.
        val next = DrawerUiState.Failed("boom").onRetry()
        assertEquals(DrawerUiState.Loading, next)
    }

    @Test
    fun `retry from Loading stays Loading — double tap is harmless`() {
        val next: DrawerUiState = DrawerUiState.Loading.onRetry()
        assertEquals(DrawerUiState.Loading, next)
    }

    @Test
    fun `retry from Ready keeps the rows — retry is only a failure affordance`() {
        val rows = listOf(botDrawerRow("jn-core", "", ""))
        val next = DrawerUiState.Ready(rows).onRetry()
        assertEquals(DrawerUiState.Ready(rows), next)
    }
}
