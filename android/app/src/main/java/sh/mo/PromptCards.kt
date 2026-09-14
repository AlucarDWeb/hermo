package sh.mo

/**
 * T9 card parsing — the pure logic behind the approval + clarify cards
 * (PI_TASK_T9 deliverable 1). No Android imports: the JVM suite links this
 * file directly and the composables only render what it produces.
 *
 * Every mapping here is Desktop copy, verified in
 * `apps/desktop/src/i18n/en.ts` (`assistant.approval` / `assistant.clarify`)
 * — the labels and the missing-choice rule come from there, not from PLAN.
 */

/**
 * The gateway's canonical approval choices (ui-tui prompts.tsx), with
 * Desktop's i18n labels (en.ts approval block). Unknown strings from a
 * future gateway keep the gateway's own text as the label — render what the
 * server sent, never invent a cut.
 */
enum class ApprovalChoice(val wire: String, val label: String) {
    RUN("once", "Run"),
    ALLOW_SESSION("session", "Allow this session"),
    ALWAYS("always", "Always allow"),
    REJECT("deny", "Reject");

    companion object {
        fun fromWire(wire: String): ApprovalChoice? = values().firstOrNull { it.wire == wire }
    }
}

/** The single choice the row highlights (Desktop: primary = Run, else Reject). */
fun primaryApprovalChoice(choices: List<String>): ApprovalChoice? {
    val wire = choices.firstOrNull { it == "once" } ?: choices.firstOrNull { it == "deny" } ?: return null
    return ApprovalChoice.fromWire(wire)
}

/** The secondary choices in the order the server sent them (minus the primary). */
fun secondaryApprovalChoices(choices: List<String>): List<ApprovalChoice> =
    choices.mapNotNull { ApprovalChoice.fromWire(it) }.filter { it != primaryApprovalChoice(choices) }

/**
 * Desktop's `allowAlways` rule (approval.tsx): `always` renders only when the
 * server's `choices` carry it (or when choices were omitted entirely AND
 * `allowPermanent !== false` — our rows always carry choices from
 * `row_json`, so the omitted case is the payload's own business, not ours).
 * A missing choice is never synthesised.
 */
fun shouldRenderAlways(choices: List<String>): Boolean = choices.contains("always")

/**
 * Desktop's confirm copy for the persistent allow (en.ts approval block),
 * adapted to the phone: the desktop names its `~/.hermes/config.yaml` path —
 * a phone has no config file, so the sentence drops that clause (declared
 * divergence, per the brief).
 */
object ApprovalCopy {
    const val ALWAYS_TITLE = "Always allow this command?"
    const val ALWAYS_BODY = "Hermes won't ask again for commands like this — in this session or any future one."
    const val ALWAYS_CONFIRM = "Always allow"
    const val ALWAYS_CANCEL = "Cancel"
    const val JUMP_TO_APPROVAL = "Approval needed"
    const val RESOLVED = "Answered elsewhere"
}

/**
 * The error copy for a failed respond: gateway codes 4009 (no pending
 * request) and 4018 (stale target) mean the approval was answered elsewhere
 * — not an error dialog (PI_TASK_T9 "Code facts"). Anything else falls back
 * to the generic message mapping.
 */
fun approvalRespondErrorCopy(t: Throwable): String? {
    val msg = t.message ?: return null
    return if (msg.contains("rpc error 4009") || msg.contains("rpc error 4018")) ApprovalCopy.RESOLVED else null
}

/** One question of a clarify card, decoded from the core's `questions[]`. */
data class ClarifyQuestionUi(
    val qid: String,
    val question: String,
    val choices: List<String>,
    val multiSelect: Boolean,
)

/**
 * Decode the raw compact JSON of `questions[]` (as `row_json` writes it:
 * `[{qid, question, choices, multi_select}]`). Defensive per PI_ROLE: an
 * empty/garbage string yields an empty list, never an exception. Covers both
 * wire shapes — the single-question shape arrives pre-defaulted to `q0` by
 * the core (`reducer.rs` on_clarify_request), so the app always sees the
 * batch array.
 */
fun parseClarifyQuestions(questionsJson: String): List<ClarifyQuestionUi> {
    if (questionsJson.isBlank()) return emptyList()
    val array = try {
        org.json.JSONArray(questionsJson)
    } catch (_: Exception) {
        return emptyList()
    }
    val out = ArrayList<ClarifyQuestionUi>(array.length())
    for (i in 0 until array.length()) {
        val entry = array.optJSONObject(i) ?: continue
        val text = entry.optString("question").trim()
        if (text.isEmpty()) continue
        val choices = ArrayList<String>()
        val rawChoices = entry.optJSONArray("choices")
        if (rawChoices != null) {
            for (j in 0 until rawChoices.length()) {
                val choice = rawChoices.optString(j).trim()
                if (choice.isNotEmpty()) choices.add(choice)
            }
        }
        out.add(
            ClarifyQuestionUi(
                qid = entry.optString("qid"),
                question = text,
                choices = choices,
                multiSelect = entry.optBoolean("multi_select") && choices.isNotEmpty(),
            ),
        )
    }
    return out
}

/** True when a multi-select answer should travel as a JSON array (Desktop stagedAnswer). */
fun isMultiSelectAnswer(question: ClarifyQuestionUi, picks: List<String>): Boolean =
    question.multiSelect && picks.isNotEmpty()

/**
 * The batch clarify join rule. Confirmed against the gateway's own parser
 * (`tools/clarify_tool.py::_parse_multi_select_response`): a multi-select
 * reply is parsed as a JSON array first, then a comma-split fallback — so
 * the canonical wire form is the JSON array. Desktop encodes exactly that
 * (`stagedAnswer`: `JSON.stringify(selectedChoices)` for multiSelect).
 */
fun encodeClarifyAnswer(question: ClarifyQuestionUi, picks: List<String>, draft: String): String =
    when {
        question.multiSelect && picks.isNotEmpty() ->
            org.json.JSONArray(picks).toString()
        else -> draft.trim()
    }

/** Desktop's batch progress line (en.ts clarify.questionProgress). */
fun clarifyProgressLabel(answered: Int, total: Int): String = "$answered of $total answered"

/**
 * The skip affordance's copy: an expired card resolves server-side to a
 * settle whose `timed_out`/skip shape the tool result carries — the phone
 * surfaces Desktop's `skipped` copy for it.
 */
const val CLARIFY_SKIPPED = "Skipped"
