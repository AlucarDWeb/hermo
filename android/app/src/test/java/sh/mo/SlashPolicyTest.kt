package sh.mo

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * T10b composer slash policy (PI_TASK_T10B "Behaviour", closed decisions):
 * when the completion popup opens, how a picked row rewrites the draft from
 * `replace_from`, and which drafts the phone answers locally instead of
 * sending to the core. Pure Kotlin — the composable and the view model only
 * read these answers.
 */
class SlashPolicyTest {

    // ── when to complete ────────────────────────────────────────────────

    @Test
    fun completesOnLeadingSlashWithoutSpace() {
        assertTrue(SlashPolicy.shouldComplete("/"))
        assertTrue(SlashPolicy.shouldComplete("/he"))
        assertTrue(SlashPolicy.shouldComplete("/help"))
    }

    @Test
    fun hidesOnSpaceArgumentStage() {
        // Argument stage is out of scope (decision 1): a space hides the popup.
        assertFalse(SlashPolicy.shouldComplete("/help "))
        assertFalse(SlashPolicy.shouldComplete("/model opus"))
    }

    @Test
    fun hidesOnEmptyOrNonSlashDraft() {
        assertFalse(SlashPolicy.shouldComplete(""))
        assertFalse(SlashPolicy.shouldComplete("hello"))
        assertFalse(SlashPolicy.shouldComplete(" /cmd"))
    }

    // ── insert from replace_from ────────────────────────────────────────

    @Test
    fun insertReplacesFromIndexAndAddsTrailingSpace() {
        // replace_from = 1 (the whole token): "/he" + pick "/help" → "/help "
        assertEquals("/help ", SlashPolicy.insertCompletion("/he", "/help", 1L))
    }

    @Test
    fun insertKeepsPrefixBeforeReplaceFrom() {
        // `/model o` picking arg completion `opus` at replace_from = 8
        // (Desktop rewrites the same way — use-slash-completions.ts prefix).
        assertEquals("/model opus ", SlashPolicy.insertCompletion("/model o", "opus", 8L))
    }

    @Test
    fun insertTreatsMissingOrNegativeReplaceFromAsOne() {
        // `-1`/absent means "replace the whole token" (decision 2).
        assertEquals("/clear ", SlashPolicy.insertCompletion("/cle", "/clear", -1L))
        assertEquals("/clear ", SlashPolicy.insertCompletion("/cle", "/clear", 0L))
    }

    // ── local commands: never reach core.send / run_slash ───────────────

    @Test
    fun clearSessionsQuitAreLocal() {
        assertTrue(SlashPolicy.isLocalCommand("/clear"))
        assertTrue(SlashPolicy.isLocalCommand("/sessions"))
        assertTrue(SlashPolicy.isLocalCommand("/quit"))
    }

    @Test
    fun everythingElseIsNotLocal() {
        // Today's tree has NO local branch at all (`AppViewModel.send` always
        // calls `repo.send`) — these assertions are red against that shape.
        assertFalse(SlashPolicy.isLocalCommand("/help"))
        assertFalse(SlashPolicy.isLocalCommand("/model"))
        assertFalse(SlashPolicy.isLocalCommand("hello"))
        assertFalse(SlashPolicy.isLocalCommand(""))
    }

    @Test
    fun localMatchIsTheWholeToken() {
        // `/clears` is NOT `/clear`; `/clear now` carries an argument and is
        // not the bare local command either.
        assertFalse(SlashPolicy.isLocalCommand("/clears"))
        assertFalse(SlashPolicy.isLocalCommand("/clear now"))
    }

    // ── which drafts run through the slash ladder ───────────────────────

    @Test
    fun submitRoutesLeadingSlashToRunSlash() {
        assertTrue(SlashPolicy.isSlashSubmit("/help"))
        assertTrue(SlashPolicy.isSlashSubmit("/model opus"))
        assertFalse(SlashPolicy.isSlashSubmit("hello"))
        assertFalse(SlashPolicy.isSlashSubmit(""))
    }
}
