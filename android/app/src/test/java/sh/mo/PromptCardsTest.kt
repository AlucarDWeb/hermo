package sh.mo

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * T9 pure logic: parse the core's row_json shapes, map Desktop labels,
 * never synthesise a choice the payload omitted.
 *
 * The pre-fix parser dropped `choices` entirely — these tests fail against
 * that parser (no field, empty list) and pass once the field is read.
 */
class PromptCardsTest {

    @Test
    fun `approval choices are the server's list`() {
        val row = parseChatRow(
            0,
            """{"kind":"approval","request_id":"r1","command":"ls","description":"list","choices":["once","session","always","deny"],"resolved":false}""",
        ) as ChatRow.Approval
        assertEquals(listOf("once", "session", "always", "deny"), row.choices)
        assertEquals(ApprovalChoice.RUN, primaryApprovalChoice(row.choices))
        assertEquals(
            listOf(ApprovalChoice.ALLOW_SESSION, ApprovalChoice.ALWAYS, ApprovalChoice.REJECT),
            secondaryApprovalChoices(row.choices),
        )
        assertTrue(shouldRenderAlways(row.choices))
    }

    @Test
    fun `missing always is not synthesised`() {
        val row = parseChatRow(
            1,
            """{"kind":"approval","request_id":"r2","command":"rm","description":"","choices":["once","deny"],"resolved":false}""",
        ) as ChatRow.Approval
        assertEquals(listOf("once", "deny"), row.choices)
        assertFalse(shouldRenderAlways(row.choices))
        assertEquals(ApprovalChoice.RUN, primaryApprovalChoice(row.choices))
        assertEquals(listOf(ApprovalChoice.REJECT), secondaryApprovalChoices(row.choices))
    }

    @Test
    fun `absent choices array is empty never invented`() {
        val row = parseChatRow(
            2,
            """{"kind":"approval","request_id":"r3","command":"x","description":"d","resolved":false}""",
        ) as ChatRow.Approval
        assertEquals(emptyList<String>(), row.choices)
        assertNull(primaryApprovalChoice(row.choices))
        assertFalse(shouldRenderAlways(row.choices))
    }

    @Test
    fun `desktop labels`() {
        assertEquals("Run", ApprovalChoice.RUN.label)
        assertEquals("Allow this session", ApprovalChoice.ALLOW_SESSION.label)
        assertEquals("Always allow", ApprovalChoice.ALWAYS.label)
        assertEquals("Reject", ApprovalChoice.REJECT.label)
    }

    @Test
    fun `4009 and 4018 are answered elsewhere not an error dialog`() {
        assertEquals(
            ApprovalCopy.RESOLVED,
            approvalRespondErrorCopy(RuntimeException("rpc error 4009: session busy")),
        )
        assertEquals(
            ApprovalCopy.RESOLVED,
            approvalRespondErrorCopy(RuntimeException("rpc error 4018: unknown slash command")),
        )
        assertNull(approvalRespondErrorCopy(RuntimeException("network failure: boom")))
    }

    @Test
    fun `clarify batch shape`() {
        val json = """[{"qid":"q0","question":"Pick one","choices":["a","b"],"multi_select":false},{"qid":"q1","question":"Pick many","choices":["x","y"],"multi_select":true}]"""
        val qs = parseClarifyQuestions(json)
        assertEquals(2, qs.size)
        assertEquals("q0", qs[0].qid)
        assertEquals(listOf("a", "b"), qs[0].choices)
        assertFalse(qs[0].multiSelect)
        assertTrue(qs[1].multiSelect)
        assertEquals("1 of 2 answered", clarifyProgressLabel(1, 2))
        assertEquals("""["x","y"]""", encodeClarifyAnswer(qs[1], listOf("x", "y"), ""))
        assertEquals("hello", encodeClarifyAnswer(qs[0], emptyList(), " hello "))
    }

    @Test
    fun `clarify garbage is empty never throws`() {
        assertEquals(emptyList<ClarifyQuestionUi>(), parseClarifyQuestions(""))
        assertEquals(emptyList<ClarifyQuestionUi>(), parseClarifyQuestions("not-json"))
        assertEquals(emptyList<ClarifyQuestionUi>(), parseClarifyQuestions("{}"))
    }
}
