package sh.mo.ui

import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawOutline
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import sh.mo.ChatRow
import sh.mo.MarkdownBlock
import sh.mo.MarkdownBlocks
import sh.mo.formatToolDuration
import sh.mo.technicalTrace
import sh.mo.toolGlyph
import sh.mo.toolTitle
import sh.mo.ui.hermoRadius
import sh.mo.ui.LocalFonts
import sh.mo.ui.LocalHermoTokens

/**
 * One transcript row (PI_TASK_T7A deliverable 2 + T8) — DESIGN.md "Chat,
 * tools & boot surfaces" + "Surfaces & elevation":
 *
 * - user rows render as Desktop's user bubble (--dt-user-bubble fill over
 *   --ui-stroke-tertiary border, rounded-xl), the T7 divergence work;
 * - assistant text renders through the markdown block renderer
 *   ([AssistantMarkdown]) fed by the core's `split_markdown`;
 * - thinking is Desktop's ThinkingDisclosure (message-parts.tsx): a
 *   ScaffoldRow whose label flips `Thinking` → `Thought` when the block
 *   settles, a chevron to the RIGHT of the label (styles.css
 *   `--disclosure-caret-rest: 0.4` — the one disclosure with a resting
 *   caret), body flush under the header, no indent;
 * - tool rows are Desktop's ToolEntry: label = the tool's title, trailing
 *   duration meta, expandable payload (`Arguments`/`Result` pretty-printed),
 *   the inline diff rendered verbatim when present (T8 scope 2);
 * - approval / clarify stay Desktop-neutral placeholder rows (NOT this
 *   stream's deliverable — T9), one quiet scaffold line, never raw JSON;
 * - error rows are the ErrorState look: no background chip, the message in
 *   --dt-destructive (DESIGN.md "Feedback & empty/error/loading states");
 * - bordered surfaces inside the transcript (code fences) use
 *   --ui-stroke-tertiary, never `border` (DESIGN.md bordered-surface rule).
 */
@Composable
fun TranscriptRow(row: ChatRow) {
    val t = LocalHermoTokens.current
    when (row) {
        is ChatRow.User -> UserBubble(row.text)

        is ChatRow.Assistant -> AssistantMarkdown(row.text)

        is ChatRow.Thinking -> ThinkingRow(row.text)

        is ChatRow.Tool -> ToolCard(row)

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
 * Desktop's user message (user-message.tsx `USER_BUBBLE_BASE_CLASS`): a
 * rounded-xl bordered bubble in --dt-user-bubble, text at the conversation
 * size. Phone adaptation, stated: no sticky behaviour, no edit-on-click,
 * no reactions — those are window affordances (T7a's declared divergence,
 * now drawn as the bubble instead of a bold flush row).
 */
@Composable
private fun UserBubble(text: String) {
    val t = LocalHermoTokens.current
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(bottom = t.paragraphGapDp.dp)
            .drawBorder(hermoRadius(t.radiusXl), t.strokeTertiary)
            .clip(hermoRadius(t.radiusXl))
            .background(t.accent)
            .padding(horizontal = 12.dp, vertical = 8.dp),
    ) {
        Text(
            text = text,
            style = androidx.compose.ui.text.TextStyle(
                fontFamily = LocalFonts.current.sans,
                fontSize = t.convFontSize.sp,
                lineHeight = t.convLineHeight.sp,
            ),
            color = t.text,
        )
    }
}

/** A 1dp hairline border behind a clipped surface (Compose border draws INSIDE). */
private fun Modifier.drawBorder(shape: RoundedCornerShape, color: androidx.compose.ui.graphics.Color): Modifier =
    this.drawBehind {
        val stroke = Stroke(width = 1.dp.toPx())
        val outline = shape.createOutline(size, layoutDirection, this)
        drawOutline(outline, color, style = stroke)
    }

/**
 * One scaffold line: glyph cell (SCAFFOLD_GLYPH_CLASS, a fixed 3.5-size box),
 * quiet label in --conversation-scaffold-text, detail, trailing meta in
 * --conversation-scaffold-meta (components/chat/scaffold-row.tsx).
 */
@Composable
internal fun ScaffoldLine(label: String, detail: String = "", meta: String = "") {
    val t = LocalHermoTokens.current
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(bottom = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        ScaffoldGlyph()
        Text(
            text = buildAnnotatedString {
                append(label)
                if (detail.isNotBlank()) {
                    append("  ")
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

/** The fixed glyph cell every scaffold line shares (shared left edge). */
@Composable
internal fun ScaffoldGlyph(glyph: String? = null, tint: androidx.compose.ui.graphics.Color? = null) {
    val t = LocalHermoTokens.current
    Box(
        modifier = Modifier.width(14.dp),
        contentAlignment = Alignment.CenterStart,
    ) {
        Text(
            text = glyph ?: "",
            style = androidx.compose.ui.text.TextStyle(
                fontFamily = LocalFonts.current.sans,
                fontSize = 9.sp,
                fontWeight = FontWeight.Medium,
            ),
            color = tint ?: t.scaffoldMeta,
        )
    }
}

/**
 * Desktop's ThinkingDisclosure (T7 divergence 4): collapsed by default once
 * settled, chevron to the RIGHT of the label, `Thinking` → `Thought` when
 * the block completes, live body with the character count while streaming.
 * The live→settled transition keeps the body mounted (Desktop's latch): a
 * body the reader expanded stays open — nothing jumps when the turn settles.
 */
@Composable
private fun ThinkingRow(text: String) {
    val t = LocalHermoTokens.current
    var userToggledOpen by rememberSaveable(rowIdKey(text)) { mutableStateOf<Boolean?>(null) }
    // The phone's `pending` signal: the core re-delivers the thinking row as
    // its text grows; a settled thought never changes again. Character count
    // is the live marker — pinned at "streamed to completion" once the turn
    // moves on (the next row arriving is Desktop's `completedAt`).
    var wasLive by remember { mutableStateOf(false) }
    val live = rowStreaming
    if (live) wasLive = true
    val open = userToggledOpen ?: (wasLive && live)
    val label = if (live) "Thinking" else "Thought"

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .animateContentSize(),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable { userToggledOpen = !(userToggledOpen ?: wasLive) }
                .padding(vertical = 1.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            ScaffoldGlyph()
            Text(
                text = label,
                style = androidx.compose.ui.text.TextStyle(
                    fontFamily = LocalFonts.current.sans,
                    fontSize = t.convToolFontSize.sp,
                    lineHeight = t.convLineHeight.sp,
                ),
                color = t.scaffoldText,
            )
            // Chevron right of the label, faint at rest (styles.css
            // --disclosure-caret-rest: 0.4 × scaffold fade).
            Chevron(open = open, tint = t.scaffoldMeta.copy(alpha = t.scaffoldMeta.alpha * 0.8f))
            if (live) {
                // Live character count, Desktop-parity (the brief's "keeping
                // the live character count").
                Text(
                    text = "${text.length} chars",
                    style = androidx.compose.ui.text.TextStyle(
                        fontFamily = LocalFonts.current.sans,
                        fontSize = 10.sp,
                    ),
                    color = t.scaffoldMeta,
                )
            }
        }
        if (open && text.isNotBlank()) {
            Text(
                text = AnnotatedString(text),
                style = androidx.compose.ui.text.TextStyle(
                    fontFamily = LocalFonts.current.sans,
                    fontSize = 12.sp,
                    lineHeight = 16.sp,
                ),
                color = t.textTertiary,
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(top = 2.dp, bottom = 4.dp),
            )
        }
    }
}

/** Disclosure chevron (DisclosureCaret: › rotates to ⌄). */
@Composable
internal fun Chevron(open: Boolean, tint: androidx.compose.ui.graphics.Color) {
    Text(
        text = if (open) "⌄" else "›",
        style = androidx.compose.ui.text.TextStyle(
            fontFamily = LocalFonts.current.sans,
            fontSize = 11.sp,
            fontWeight = FontWeight.Medium,
        ),
        color = tint,
    )
}

/**
 * RememberSaveable key: one disclosure state per row content. The row id is
 * positional; keying on the content keeps two thinking rows from sharing a
 * toggle while not resetting the state on every streaming delta.
 */
private fun rowIdKey(text: String): String = "think:${text.hashCode()}"

/**
 * Whether the turn is live, provided from the screen level (the repository's
 * running flag — Desktop's `messageRunning`). Provided through
 * [LocalTurnRunning] so composable rows never infer a phase themselves.
 */
internal val LocalTurnRunning = androidx.compose.runtime.staticCompositionLocalOf { false }
private val rowStreaming: Boolean @Composable get() = LocalTurnRunning.current

/**
 * Desktop's ToolEntry (T8 scope 2): header = glyph (icon mapping) + title +
 * trailing duration; expandable body = the pretty-printed `Arguments` /
 * `Result` payload, or the inline diff verbatim when one is present (the
 * diff is the deliverable — it opens by default, like Desktop's
 * `defaultOpen = Boolean(inlineDiff)`).
 */
@Composable
private fun ToolCard(row: ChatRow.Tool) {
    val t = LocalHermoTokens.current
    val hasDiff = row.inlineDiff.isNotBlank()
    var open by rememberSaveable("tool:${row.id}:${row.name}") { mutableStateOf(hasDiff) }
    val running = !row.complete

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .animateContentSize(),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable { open = !open }
                .padding(vertical = 1.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            ScaffoldGlyph(glyph = toolGlyph(row.name)?.take(1)?.uppercase() ?: "•")
            Text(
                text = buildAnnotatedString {
                    append(toolTitle(row.name))
                    if (running) {
                        withStyle(androidx.compose.ui.text.SpanStyle(color = t.scaffoldMeta)) { append("  ·  running") }
                    } else if (row.exitCode != null) {
                        withStyle(androidx.compose.ui.text.SpanStyle(color = t.destructive)) { append("  ·  exit ${row.exitCode}") }
                    }
                },
                style = androidx.compose.ui.text.TextStyle(
                    fontFamily = LocalFonts.current.sans,
                    fontSize = t.convToolFontSize.sp,
                    lineHeight = t.convLineHeight.sp,
                ),
                color = if (running) t.scaffoldMeta else t.scaffoldText,
                modifier = Modifier.weight(1f, fill = false),
            )
            // Trailing meta: the duration once done (Desktop's durationLabel);
            // running rows keep the slot empty (the live timer is the
            // status strip's job on the phone).
            if (!running) {
                val duration = formatToolDuration(row.durationS)
                if (duration.isNotEmpty()) {
                    Text(
                        text = duration,
                        style = androidx.compose.ui.text.TextStyle(
                            fontFamily = LocalFonts.current.sans,
                            fontSize = 10.sp,
                        ),
                        color = t.scaffoldMeta,
                    )
                }
            }
            Chevron(open = open, tint = t.scaffoldMeta)
        }
        if (open) {
            // Expanded shell: the TOOL_EXPANDED_SHELL_CLASS border + payload.
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(top = 2.dp, bottom = 2.dp)
                    .clip(hermoRadius(t.radiusMd))
                    .background(t.widgetSurface)
                    .padding(8.dp),
            ) {
                if (hasDiff) {
                    // The inline diff renders verbatim, monospace (Desktop's
                    // FileDiffPanel without the shiki theme — stated: no
                    // syntax highlighting on the phone).
                    Text(
                        text = row.inlineDiff,
                        style = monoPayload(t),
                        color = t.textSecondary,
                        modifier = Modifier.fillMaxWidth(),
                    )
                } else {
                    val trace = technicalTrace(row.argsJson, row.resultJson)
                    if (trace.isNotBlank()) {
                        Text(
                            text = trace,
                            style = monoPayload(t),
                            color = t.textSecondary,
                            modifier = Modifier
                                .fillMaxWidth()
                                .horizontalScroll(rememberScrollState()),
                        )
                    }
                }
            }
        }
    }
}

private fun monoPayload(t: sh.mo.ui.HermoTokens) = androidx.compose.ui.text.TextStyle(
    fontFamily = FontFamily.Monospace,
    fontSize = 10.4f.sp, // TOOL_PAYLOAD_PRE_CLASS: 0.65rem
    lineHeight = t.convLineHeight.sp,
)

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
