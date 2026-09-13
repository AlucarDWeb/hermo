package sh.mo

/**
 * One transcript row, decoded from the core's row JSON (`core.rs::row_json`).
 * Pure Kotlin — no Android imports — so a plain `kt_jvm_test` can link it.
 *
 * Row JSON shapes (PI_TASK_T7A "Facts you must not re-derive"):
 * user/assistant/thinking carry `text`; tool carries `name`/`tool_id`/
 * `complete`/`context`/`args`/`result`/`duration_s`; approval carries
 * `request_id`/`command`/`description`/`choices`/`resolved`; clarify carries
 * `request_id`/`questions[]`/`resolved`; status carries `status`+`text`;
 * error carries `message`. Unknown kinds and garbage JSON degrade to a
 * status row carrying nothing — parse defensively, never throw.
 */
sealed interface ChatRow {
    val id: Int

    data class User(override val id: Int, val text: String) : ChatRow
    data class Assistant(
        override val id: Int,
        val text: String,
        val streaming: Boolean = false,
        val warning: String = "",
        /** Compact JSON `usage` of `message.complete` (untyped, PLAN §3). */
        val usageJson: String = "",
    ) : ChatRow
    data class Thinking(override val id: Int, val text: String) : ChatRow

    /**
     * Tool card (T8): `args`/`result` cross as compact JSON strings (PLAN §3),
     * so the pretty-printing and the inline-diff extraction stay APP-side —
     * the UI is the formatter, never the core.
     */
    data class Tool(
        override val id: Int,
        val name: String,
        val complete: Boolean,
        val context: String,
        val argsJson: String = "",
        val resultJson: String = "",
        val inlineDiff: String = "",
        val durationS: Double = 0.0,
        val exitCode: Int? = null,
    ) : ChatRow
    data class Approval(
        override val id: Int,
        val requestId: String,
        val command: String,
        val description: String,
        val resolved: Boolean,
    ) : ChatRow
    data class Clarify(
        override val id: Int,
        val requestId: String,
        val questions: String, // raw `questions[]` JSON — rendered by T8.
        val resolved: Boolean,
    ) : ChatRow
    data class Status(override val id: Int, val kind: String, val text: String) : ChatRow
    data class Error(override val id: Int, val message: String) : ChatRow
}

/**
 * The parse: one row-JSON string → one [ChatRow]. Defensive per PI_ROLE: an
 * empty/garbage string yields a Status row with empty text, never an
 * exception.
 */
fun parseChatRow(id: Int, rowJson: String): ChatRow {
    val obj = try {
        org.json.JSONObject(rowJson)
    } catch (_: Exception) {
        return ChatRow.Status(id, "unknown", "")
    }
    return when (obj.optString("kind")) {
        "user" -> ChatRow.User(id, obj.optString("text"))
        "assistant" -> ChatRow.Assistant(
            id,
            obj.optString("text"),
            streaming = obj.optBoolean("streaming"),
            warning = obj.optString("warning"),
            usageJson = obj.optStringOrNull("usage") ?: "",
        )
        "thinking" -> ChatRow.Thinking(id, obj.optString("text"))
        "tool" -> ChatRow.Tool(
            id,
            name = obj.optString("name"),
            complete = obj.optBoolean("complete"),
            context = obj.optString("context"),
            argsJson = obj.optString("args"),
            resultJson = obj.optString("result"),
            inlineDiff = inlineDiffOf(obj),
            durationS = obj.optDouble("duration_s", 0.0),
            exitCode = obj.optInt("exit_code", 0).takeIf { obj.has("exit_code") && it != 0 },
        )
        "approval" -> ChatRow.Approval(
            id,
            requestId = obj.optString("request_id"),
            command = obj.optString("command"),
            description = obj.optString("description"),
            resolved = obj.optBoolean("resolved"),
        )
        "clarify" -> ChatRow.Clarify(
            id,
            requestId = obj.optString("request_id"),
            questions = obj.optJSONArray("questions")?.toString() ?: "[]",
            resolved = obj.optBoolean("resolved"),
        )
        "status" -> ChatRow.Status(id, obj.optString("status"), obj.optString("text"))
        "error" -> ChatRow.Error(id, obj.optString("message"))
        // Unknown kind: tolerate, do not crash (PI_ROLE wire rule).
        else -> ChatRow.Status(id, obj.optString("kind"), "")
    }
}

/**
 * Desktop's `inlineDiffFromResult` (tool/fallback-model/index.ts): the diff
 * hides under `inline_diff` or `diff`, either at the top level or inside the
 * result object — take the first non-empty string. Never throws.
 */
private fun inlineDiffOf(obj: org.json.JSONObject): String {
    val result = obj.optJSONObject("result")
    val sources = listOfNotNull(
        obj.optStringOrNull("inline_diff"),
        obj.optStringOrNull("diff"),
        result?.optStringOrNull("inline_diff"),
        result?.optStringOrNull("diff"),
    )
    return sources.firstOrNull { it.isNotBlank() } ?: ""
}

private fun org.json.JSONObject.optStringOrNull(key: String): String? =
    if (has(key) && !isNull(key)) optString(key) else null
