package sh.mo.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import sh.mo.ApprovalChoice
import sh.mo.ApprovalCopy
import sh.mo.ChatRow
import sh.mo.ClarifyQuestionUi
import sh.mo.encodeClarifyAnswer
import sh.mo.parseClarifyQuestions
import sh.mo.primaryApprovalChoice

/**
 * T9 cards — DESIGN.md "Chat, tools & boot surfaces" inline widgets:
 * shared radius (`--radius-3xl`), `--ui-widget-surface-background`, no
 * border. Actions sit OUTSIDE the panel, below it.
 *
 * Phone-native (declared): Desktop's Run+overflow menu becomes a button
 * row of every choice the server sent. Always-allow is a second tap, not
 * a dialog window. Hover does not exist.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun ApprovalCard(
    row: ChatRow.Approval,
    onChoice: (requestId: String, choice: String) -> Unit,
) {
    val t = LocalHermoTokens.current
    var confirmAlways by remember(row.requestId) { mutableStateOf(false) }
    val body = row.description.ifBlank { row.command }

    Column(modifier = Modifier.fillMaxWidth().padding(bottom = t.paragraphGapDp.dp)) {
        WidgetShell {
            if (body.isNotBlank()) {
                Text(
                    text = body,
                    style = TextStyle(
                        fontFamily = LocalFonts.current.sans,
                        fontSize = t.convFontSize.sp,
                        lineHeight = t.convLineHeight.sp,
                    ),
                    color = t.text,
                )
            }
            if (row.command.isNotBlank() && row.description.isNotBlank()) {
                Text(
                    text = row.command,
                    style = TextStyle(
                        fontFamily = LocalFonts.current.mono,
                        fontSize = t.convToolFontSize.sp,
                    ),
                    color = t.textTertiary,
                    modifier = Modifier.padding(top = 6.dp),
                )
            }
        }
        if (row.resolved) return@Column
        if (confirmAlways) {
            Text(
                text = ApprovalCopy.ALWAYS_TITLE,
                style = TextStyle(
                    fontFamily = LocalFonts.current.sans,
                    fontSize = t.convFontSize.sp,
                    fontWeight = androidx.compose.ui.text.font.FontWeight.Medium,
                ),
                color = t.text,
                modifier = Modifier.padding(top = 8.dp),
            )
            Text(
                text = ApprovalCopy.ALWAYS_BODY,
                style = TextStyle(
                    fontFamily = LocalFonts.current.sans,
                    fontSize = t.convToolFontSize.sp,
                ),
                color = t.textSecondary,
                modifier = Modifier.padding(top = 4.dp, bottom = 8.dp),
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                ChoiceButton(label = ApprovalCopy.ALWAYS_CONFIRM, primary = true) {
                    onChoice(row.requestId, "always")
                    confirmAlways = false
                }
                ChoiceButton(label = ApprovalCopy.ALWAYS_CANCEL, primary = false) {
                    confirmAlways = false
                }
            }
        } else {
            // FlowRow, not a non-wrapping Row: the phone renders every choice
            // the server sent as a button (the phone-native stand-in for
            // Desktop's overflow menu), and four Desktop labels clip on a
            // Pixel-7a-width screen (PI_TASK_FIX5 item 5).
            FlowRow(
                modifier = Modifier.padding(top = 8.dp).fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                val primary = primaryApprovalChoice(row.choices)
                row.choices.forEach { wire ->
                    val known = ApprovalChoice.fromWire(wire)
                    val label = known?.label ?: wire
                    val primaryHere = primary != null && wire == primary.wire
                    ChoiceButton(label = label, primary = primaryHere) {
                        if (wire == "always") confirmAlways = true
                        else onChoice(row.requestId, wire)
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun ClarifyCard(
    row: ChatRow.Clarify,
    onAnswer: (requestId: String, answer: String, questionId: String?) -> Unit,
) {
    val t = LocalHermoTokens.current
    val questions = parseClarifyQuestions(row.questions)
    if (questions.isEmpty()) {
        ScaffoldLine(label = "Question", meta = if (row.resolved) "" else "pending")
        return
    }
    Column(modifier = Modifier.fillMaxWidth().padding(bottom = t.paragraphGapDp.dp)) {
        // One unanswered question at a time: the core `respond_clarify`
        // resolves the WHOLE card after the first successful RPC and the FFI
        // returns `()` (no `remaining` arrives), so a fake q1…n list would
        // vanish after the first Continue while q2…n were never actually
        // asked (PI_TASK_FIX5 item 3). The answered count is the card's real
        // one: every question before the first unanswered one.
        val firstOpen = questions.indexOfFirst { !row.resolved }
        val visible = if (row.resolved) questions else questions.take(firstOpen + 1)
        val answeredCount = if (row.resolved) questions.size else firstOpen.coerceAtLeast(0)
        visible.forEachIndexed { index, q ->
            ClarifyQuestionBlock(
                question = q,
                resolved = row.resolved,
                showProgress = questions.size > 1 && !row.resolved,
                answered = answeredCount + index,
                total = questions.size,
                onSubmit = { answer ->
                    onAnswer(row.requestId, answer, q.qid.ifBlank { null })
                },
            )
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun ClarifyQuestionBlock(
    question: ClarifyQuestionUi,
    resolved: Boolean,
    showProgress: Boolean,
    answered: Int,
    total: Int,
    onSubmit: (String) -> Unit,
) {
    val t = LocalHermoTokens.current
    var selected by remember(question.qid) { mutableStateOf(listOf<String>()) }
    var draft by remember(question.qid) { mutableStateOf("") }

    Column(modifier = Modifier.padding(bottom = if (answered < total - 1 && !resolved) 12.dp else 0.dp)) {
        WidgetShell {
            if (showProgress) {
                Text(
                    text = sh.mo.clarifyProgressLabel(answered, total),
                    style = TextStyle(
                        fontFamily = LocalFonts.current.sans,
                        fontSize = t.convToolFontSize.sp,
                    ),
                    color = t.textTertiary,
                    modifier = Modifier.padding(bottom = 4.dp),
                )
            }
            Text(
                text = question.question,
                style = TextStyle(
                    fontFamily = LocalFonts.current.sans,
                    fontSize = t.convFontSize.sp,
                    lineHeight = t.convLineHeight.sp,
                ),
                color = t.text,
            )
        }
        if (resolved) return@Column
        if (question.choices.isNotEmpty()) {
            // FlowRow: clarify choice chips share the approval labels' clipping
            // problem on a narrow screen (PI_TASK_FIX5 item 5).
            FlowRow(
                modifier = Modifier.padding(top = 8.dp).fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                question.choices.forEach { choice ->
                    val on = selected.contains(choice)
                    ChoiceButton(label = choice, primary = on) {
                        selected = if (question.multiSelect) {
                            if (on) selected - choice else selected + choice
                        } else {
                            listOf(choice)
                        }
                        draft = ""
                    }
                }
            }
        }
        Text(
            text = "Other (type your answer)",
            style = TextStyle(
                fontFamily = LocalFonts.current.sans,
                fontSize = t.convToolFontSize.sp,
            ),
            color = t.textTertiary,
            modifier = Modifier.padding(top = 8.dp, bottom = 4.dp),
        )
        BasicTextField(
            value = draft,
            onValueChange = {
                draft = it
                if (it.isNotBlank()) selected = emptyList()
            },
            textStyle = TextStyle(
                fontFamily = LocalFonts.current.sans,
                fontSize = t.convFontSize.sp,
                color = t.text,
            ),
            cursorBrush = SolidColor(t.primary),
            modifier = Modifier
                .fillMaxWidth()
                .clip(hermoRadius(t.radiusMd))
                .background(t.surface)
                .border(1.dp, t.strokeTertiary, hermoRadius(t.radiusMd))
                .padding(horizontal = 10.dp, vertical = 8.dp),
        )
        Row(modifier = Modifier.padding(top = 8.dp)) {
            ChoiceButton(label = "Continue", primary = true) {
                val answer = encodeClarifyAnswer(question, selected, draft)
                if (answer.isNotBlank()) onSubmit(answer)
            }
        }
    }
}

@Composable
private fun WidgetShell(content: @Composable () -> Unit) {
    val t = LocalHermoTokens.current
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(hermoRadius(t.radius3xl))
            .background(t.widgetSurface)
            .padding(horizontal = 12.dp, vertical = 10.dp),
        content = { content() },
    )
}

@Composable
private fun ChoiceButton(label: String, primary: Boolean, onClick: () -> Unit) {
    val t = LocalHermoTokens.current
    Text(
        text = label,
        style = TextStyle(
            fontFamily = LocalFonts.current.sans,
            fontSize = t.convToolFontSize.sp,
        ),
        color = if (primary) t.onPrimary else t.text,
        modifier = Modifier
            .clip(hermoRadius(t.radiusMd))
            .background(if (primary) t.primarySolid else t.softFill)
            .clickable(onClick = onClick)
            .padding(horizontal = 10.dp, vertical = 6.dp),
    )
}
