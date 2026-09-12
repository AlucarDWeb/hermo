package sh.mo

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Pins the app-side markdown splitter (ui/MarkdownBlocks.kt) against the
 * core's `split_blocks` rules — the renderer's block grammar must match the
 * core's or a streamed assistant message renders code as prose.
 */
class MarkdownBlocksTest {

    @Test
    fun `fence with language and surrounding prose`() {
        val blocks = sh.mo.MarkdownBlocks.split("before\n```rust\nfn main() {}\n```\nafter")
        assertEquals(3, blocks.size)
        assertEquals("before", blocks[0].text)
        assertEquals("rust", blocks[1].language)
        assertEquals("fn main() {}", blocks[1].text)
        assertTrue(blocks[1].isFence)
        assertEquals("after", blocks[2].text)
    }

    @Test
    fun `unterminated final fence stays open`() {
        val blocks = sh.mo.MarkdownBlocks.split("prose\n```python\nprint(1)")
        assertEquals(2, blocks.size)
        assertTrue(blocks[1].open)
        assertEquals("python", blocks[1].language)
        assertEquals("print(1)", blocks[1].text)
    }

    @Test
    fun `inner shorter backtick run does not close the fence`() {
        val blocks = sh.mo.MarkdownBlocks.split("```\ncode with ``` inside\nmore\n```")
        assertEquals(1, blocks.size)
        assertTrue(blocks[0].text.contains("``` inside"))
    }

    @Test
    fun `tilde fences work`() {
        val blocks = sh.mo.MarkdownBlocks.split("~~~\nx\n~~~")
        assertEquals(1, blocks.size)
        assertTrue(blocks[0].isFence)
        assertEquals("x", blocks[0].text)
    }

    @Test
    fun `empty and plain text`() {
        assertEquals(0, sh.mo.MarkdownBlocks.split("").size)
        val one = sh.mo.MarkdownBlocks.split("just text")
        assertEquals(1, one.size)
        assertTrue(!one[0].isFence)
    }
}

/** Pins the row-JSON decoding (PI_TASK_T7A "Facts you must not re-derive"). */
class ChatRowParseTest {

    @Test
    fun `every core row kind decodes`() {
        assertEquals(
            sh.mo.ChatRow.User(0, "hello"),
            sh.mo.parseChatRow(0, """{"kind":"user","text":"hello"}"""),
        )
        val a = sh.mo.parseChatRow(1, """{"kind":"assistant","text":"hi","streaming":true}""")
        assertTrue(a is sh.mo.ChatRow.Assistant && a.streaming)
        assertTrue(sh.mo.parseChatRow(2, """{"kind":"thinking","text":"hm"}""") is sh.mo.ChatRow.Thinking)
        val t = sh.mo.parseChatRow(3, """{"kind":"tool","name":"ls","complete":true,"duration_s":0.5}""")
        assertTrue(t is sh.mo.ChatRow.Tool && t.complete && t.durationS == 0.5)
        assertTrue(
            sh.mo.parseChatRow(4, """{"kind":"status","status":"Done","text":"x"}""") is sh.mo.ChatRow.Status,
        )
        val e = sh.mo.parseChatRow(5, """{"kind":"error","message":"boom"}""")
        assertTrue(e is sh.mo.ChatRow.Error && e.message == "boom")
    }

    @Test
    fun `garbage degrades, never throws`() {
        assertTrue(sh.mo.parseChatRow(0, "") is sh.mo.ChatRow.Status)
        assertTrue(sh.mo.parseChatRow(0, "not json") is sh.mo.ChatRow.Status)
        assertTrue(sh.mo.parseChatRow(0, """{"kind":"mystery"}""") is sh.mo.ChatRow.Status)
    }
}
