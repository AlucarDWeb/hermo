package sh.mo.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import sh.mo.ChatRow
import sh.mo.MarkdownBlock
import sh.mo.MarkdownBlocks
import sh.mo.ui.hermoRadius
import sh.mo.ui.LocalFonts
import sh.mo.ui.LocalHermoTokens

/**
 * One transcript row (PI_TASK_T7A deliverable 2) — DESIGN.md "Chat, tools &
 * boot surfaces" + "Surfaces & elevation":
 *
 * - user rows render flush as Desktop's user message (the phone drops the
 *   sticky bubble: a bubble is an affordance of desktop scroll geometry —
 *   the phone-native equivalent is a bold flush row, stated divergence);
 * - assistant text renders through the markdown block renderer
 *   ([AssistantMarkdown]) fed by the core's `split_markdown`;
 * - thinking / tool / status are `ScaffoldRow`s: one quiet line in
 *   --conversation-scaffold-text with a fixed glyph cell and trailing meta
 *   (components/chat/scaffold-row.tsx) — collapsible detail lands with T8;
 * - error rows are the ErrorState look: no background chip, the message in
 *   --dt-destructive (DESIGN.md "Feedback & empty/error/loading states");
 * - bordered surfaces inside the transcript (code fences) use
 *   --ui-stroke-tertiary, never `border` (DESIGN.md "Chat, tools & boot
 *   surfaces", the bordered-surface rule).
 */
@Composable
fun TranscriptRow(row: ChatRow) {
    val t = LocalHermoTokens.current
    when (row) {
        is ChatRow.User -> Text(
            text = row.text,
            style = androidx.compose.ui.text.TextStyle(
                fontFamily = LocalFonts.current.sans,
                fontWeight = FontWeight.SemiBold,
                fontSize = t.convFontSize.sp,
                lineHeight = t.convLineHeight.sp,
            ),
            color = t.text,
            modifier = Modifier
                .fillMaxWidth()
                .padding(bottom = t.paragraphGapDp.dp),
        )

        is ChatRow.Assistant -> AssistantMarkdown(row.text)

        is ChatRow.Thinking -> ScaffoldLine(label = "Thinking", detail = firstLine(row.text))

        is ChatRow.Tool -> ScaffoldLine(
            label = row.name,
            detail = if (row.context.isNotBlank()) firstLine(row.context) else "",
            meta = if (row.complete) formatDuration(row.durationS) else "running",
        )

        is ChatRow.Approval -> ScaffoldLine(
            label = "Approval",
            detail = if (row.command.isNotBlank()) firstLine(row.command) else row.description,
            meta = if (row.resolved) "" else "pending",
        )

        is ChatRow.Clarify -> ScaffoldLine(label = "Question", meta = if (row.resolved) "" else "pending")

        is ChatRow.Status -> if (row.text.isNotBlank() || row.kind.isNotBlank()) {
            ScaffoldLine(label = row.kind.replaceFirstChar { it.uppercase() }, detail = firstLine(row.text))
        }

        is ChatRow.Error -> Text(
            text = row.message,
            style = androidx.compose.ui.text.TextStyle(
                fontFamily = LocalFonts.current.sans,
                fontSize = t.convFontSize.sp,
                lineHeight = t.convLineHeight.sp,
            ),
            color = t.destructive,
            modifier = Modifier
                .fillMaxWidth()
                .padding(bottom = t.paragraphGapDp.dp),
        )
    }
}

/**
 * One scaffold line: glyph cell (a 3.5-size dot cell, SCAFFOLD_GLYPH_CLASS),
 * quiet label, detail, trailing meta in --conversation-scaffold-meta.
 */
@Composable
private fun ScaffoldLine(label: String, detail: String = "", meta: String = "") {
    val t = LocalHermoTokens.current
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(bottom = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Spacer(
            modifier = Modifier
                .width(14.dp)
                .padding(top = 5.dp)
                .clip(RoundedCornerShape(2.dp))
                .background(t.scaffoldMeta),
        )
        Text(
            text = buildAnnotatedString {
                append(label)
                if (detail.isNotBlank()) {
                    append("  ·  ")
                    append(detail)
                }
            },
            style = androidx.compose.ui.text.TextStyle(
                fontFamily = LocalFonts.current.sans,
                fontSize = t.convToolFontSize.sp,
                lineHeight = t.convLineHeight.sp,
            ),
            color = t.scaffoldText,
            modifier = Modifier.weight(1f, fill = false),
        )
        if (meta.isNotBlank()) {
            Text(
                text = meta,
                style = androidx.compose.ui.text.TextStyle(
                    fontFamily = LocalFonts.current.sans,
                    fontSize = 10.sp,
                ),
                color = t.scaffoldMeta,
            )
        }
    }
}

/**
 * Assistant markdown (deliverable 2, IN scope): blocks from the core's
 * `split_markdown` — plain text renders as the conversation text style,
 * fenced code as a monospace block with a --ui-stroke-tertiary border and
 * the widget-surface fill (DESIGN.md bordered-surface rule). Inline
 * emphasis/headers are NOT re-implemented here: the core's splitter defines
 * the block grammar, and a third-party markdown library stays out per the
 * brief. Prose margins follow --paragraph-gap with the first block flush
 * (styles.css `.aui-md` rules).
 */
@Composable
fun AssistantMarkdown(text: String) {
    val t = LocalHermoTokens.current
    val blocks = MarkdownBlocks.split(text)
    Column(modifier = Modifier.padding(bottom = t.paragraphGapDp.dp)) {
        blocks.forEachIndexed { i, block ->
            val first = i == 0
            when {
                block.language.isNotEmpty() || block.isFence -> CodeBlock(block)
                else -> Text(
                    text = AnnotatedString(block.text),
                    style = androidx.compose.ui.text.TextStyle(
                        fontFamily = LocalFonts.current.sans,
                        fontSize = t.convFontSize.sp,
                        lineHeight = t.convLineHeight.sp,
                    ),
                    color = t.text,
                    modifier = Modifier.padding(top = if (first) 0.dp else t.paragraphGapDp.dp),
                )
            }
        }
    }
}

@Composable
private fun CodeBlock(block: MarkdownBlock) {
    val t = LocalHermoTokens.current
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(top = t.paragraphGapDp.dp)
            .clip(hermoRadius(t.radiusMd))
            .background(t.widgetSurface)
            .padding(8.dp),
    ) {
        if (block.language.isNotEmpty()) {
            Text(
                text = block.language,
                style = androidx.compose.ui.text.TextStyle(fontFamily = FontFamily.Monospace, fontSize = 10.sp),
                color = t.textTertiary,
            )
        }
        val scroll = rememberScrollState()
        Text(
            text = AnnotatedString(block.text),
            style = androidx.compose.ui.text.TextStyle(
                fontFamily = LocalFonts.current.mono,
                fontSize = t.convToolFontSize.sp,
                lineHeight = t.convLineHeight.sp,
            ),
            color = t.text,
            modifier = Modifier
                .fillMaxWidth()
                .horizontalScroll(scroll)
                .padding(top = if (block.language.isEmpty()) 0.dp else 4.dp),
        )
    }
}

private fun firstLine(s: String): String = s.lineSequence().firstOrNull { it.isNotBlank() } ?: ""

private fun formatDuration(s: Double): String =
    if (s >= 10) "${s.toInt()}s" else "${"%.1f".format(s)}s"
