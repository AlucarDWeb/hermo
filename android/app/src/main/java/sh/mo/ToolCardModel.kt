package sh.mo

import java.util.Locale

/**
 * Desktop's tool-card view model, mirrored from
 * `apps/desktop/src/components/assistant-ui/tool/fallback-model/` (T8
 * scope 2) — pure Kotlin, no Android imports, JVM-testable. Every formatter
 * here is the Desktop formatter restated, not an invention:
 *
 * - `formatDurationSeconds` (fallback-model/format.ts): <1s → ms, <60s →
 *   `0.5s`/`12s` (one decimal only below 10), then `Xm Ys`/`Xm`, then the
 *   hours branch `Xh Ym`/`Xh` (format.ts:141-152 — was missing, reviewed on
 *   PR #9).
 * - `prettyTechnicalValue` + `technicalTrace` (fallback.tsx): a JSON-looking
 *   string is re-printed pretty (`JSON.stringify(…, null, 2)`), everything
 *   else passes through; the trace is `Arguments:\n…\n\nResult:\n…`.
 * - `toolIcon` (TOOL_META + PREFIX_META, fallback-model/index.ts): exact
 *   name table first, then the `browser_`/`web_` prefix mapping, else null.
 *   The phone renders the mapping as its leading glyph text/box, not SVG
 *   paths (a phone cannot load the desktop icon font — stated equivalence).
 * - `usageLabel` (lib/statusbar.tsx usageContextLabel): `~12.3k/200k` with
 *   the `~` only when `context_estimated`, or `N tok` when no context max.
 * - `formatElapsed` (activity-timer.ts): `42s`, then `m:ss`.
 * - `clampForDisplay` (fallback-model/format.ts): the expanded payload is
 *   clamped to `MAX_TOOL_RENDER_CHARS` (20 000) with a trailing line saying
 *   how many characters were omitted — stacked unclamped tool rows froze
 *   the Desktop renderer, so the phone clamps for the same reason (PR #9).
 * - `stripInlineDiffChrome` (fallback-model/index.ts): strips ANSI escape
 *   sequences and the leading `┊ review diff` header line from the raw
 *   inline diff — the Desktop never shows that chrome (PR #9).
 * - `compactNumber` (lib/format.ts): k/M promotion just under the boundary.
 */

/** Desktop's `formatDurationSeconds`. Empty for a negative/absent duration. */
fun formatToolDuration(seconds: Double): String {
    if (seconds.isNaN() || seconds < 0) return ""
    if (seconds < 1) {
        val ms = maxOf(1, Math.round(seconds * 1000).toInt())
        return "${ms}ms"
    }
    if (seconds < 60) {
        return if (seconds >= 10) "${seconds.toInt()}s" else String.format(Locale.US, "%.1f", seconds) + "s"
    }
    val whole = Math.round(seconds).toInt()
    val minutes = whole / 60
    val rem = whole % 60
    if (minutes < 60) {
        return if (rem != 0) "${minutes}m ${rem}s" else "${minutes}m"
    }
    // format.ts:141-152's hours branch, restated: `1h`, `1h 2m`.
    val hours = minutes / 60
    val remMinutes = minutes % 60
    return if (remMinutes != 0) "${hours}h ${remMinutes}m" else "${hours}h"
}

/** Desktop's `formatElapsed` — the live turn timer (`42s`, then `1:05`). */
fun formatElapsed(seconds: Long): String {
    if (seconds < 60) return "${seconds}s"
    return "${seconds / 60}:${(seconds % 60).toString().padStart(2, '0')}"
}

/** Desktop's `compactNumber` (lib/format.ts). */
fun compactNumber(value: Long): String {
    val num = value.toDouble()
    if (num <= 0) return "0"
    fun scaled(v: Double, suffix: String) =
        String.format(Locale.US, "%.1f", v).trimEnd('0').trimEnd('.') + suffix
    return when {
        num >= 999_950 -> scaled(num / 1_000_000, "M")
        num >= 999.5 -> scaled(num / 1_000, "k")
        else -> Math.round(num).toString()
    }
}

/** The status strip's usage chip, Desktop's `usageContextLabel`. */
fun usageLabel(usageJson: String): String {
    if (usageJson.isBlank()) return ""
    return try {
        val u = org.json.JSONObject(usageJson)
        val contextMax = u.optLong("context_max")
        if (contextMax > 0) {
            val tilde = if (u.optBoolean("context_estimated")) "~" else ""
            "$tilde${compactNumber(u.optLong("context_used"))}/${compactNumber(contextMax)}"
        } else {
            val total = u.optLong("total")
            if (total > 0) "${compactNumber(total)} tok" else ""
        }
    } catch (_: Exception) {
        "" // untyped payload: parse defensively, never throw (PI_ROLE).
    }
}

/**
 * Desktop's `prettyTechnicalValue`: a JSON-looking string is pretty-printed,
 * any other string passes through untouched.
 */
fun prettyTechnicalValue(value: String): String {
    val trimmed = value.trim()
    if (!trimmed.startsWith("{") && !trimmed.startsWith("[")) return value
    return try {
        val parsed = org.json.JSONTokener(trimmed).nextValue()
        // org.json's toString(indentSpaces) — the JSON.stringify(…, null, 2) twin.
        when (parsed) {
            is org.json.JSONObject -> parsed.toString(2)
            is org.json.JSONArray -> parsed.toString(2)
            else -> value
        }
    } catch (_: Exception) {
        value
    }
}

/** Desktop's `technicalTrace` — the expanded row's payload body. */
fun technicalTrace(argsJson: String, resultJson: String): String {
    val parts = mutableListOf<String>()
    if (argsJson.isNotBlank()) parts.add("Arguments:\n${prettyTechnicalValue(argsJson)}")
    if (resultJson.isNotBlank()) parts.add("Result:\n${prettyTechnicalValue(resultJson)}")
    return parts.joinToString("\n\n")
}

/**
 * Desktop's tool icon mapping (fallback-model/index.ts TOOL_META + PREFIX_META).
 * Exact names first; unknown names fall back to the `browser_`/`web_` prefix
 * rule; anything else carries no glyph — the Desktop fallback (no icon).
 */
fun toolGlyph(name: String): String? = when (name) {
    "browser_click", "browser_fill", "browser_navigate", "browser_snapshot", "browser_type" -> "globe"
    "browser_take_screenshot" -> "image"
    "clarify" -> "question"
    "cronjob" -> "watch"
    "edit_file", "patch", "write_file" -> "edit"
    "execute_code", "terminal" -> "terminal"
    "image_generate" -> "image"
    "list_files" -> "files"
    "memory" -> "brain"
    "read_file" -> "file"
    "search_files", "session_search_recall", "web_search" -> "search"
    "todo" -> "tools"
    "vision_analyze" -> "eye"
    "web_extract" -> "globe"
    else -> when {
        name.startsWith("browser_") -> "globe"
        name.startsWith("web_") -> "globe"
        else -> null
    }
}

/** Desktop's `titleForTool`: `web_search` → "Search", `browser_navigate` → "Navigate". */
fun toolTitle(name: String): String =
    name.removePrefix("browser_").removePrefix("web_")
        .split("_").filter { it.isNotEmpty() }
        .joinToString(" ") { it.replaceFirstChar { c -> c.uppercase() } }
        .ifEmpty { name }

/**
 * Desktop's status ladder (`toolStatus` + `leadingStatus`): a card still
 * running is `running`; once complete, success is SILENT (no checkmark — the
 * row simply reads as done) and only error/warning get a glyph.
 */
fun toolStatus(complete: Boolean): String = if (complete) "done" else "running"

/** Desktop's `MAX_TOOL_RENDER_CHARS` (fallback-model/format.ts:70). */
const val MAX_TOOL_RENDER_CHARS = 20_000

/**
 * Desktop's `clampForDisplay` (fallback-model/format.ts:72-80): over the cap
 * the payload is cut to exactly `max` characters and a continuation line
 * states the omitted count (locale-independent digits — the Desktop uses
 * `toLocaleString()`, the phone's own display locale is English-only UI).
 */
fun clampForDisplay(value: String, max: Int = MAX_TOOL_RENDER_CHARS): String {
    if (value.length <= max) return value
    val omitted = value.length - max
    return "${value.take(max)}\n\n… $omitted more characters truncated — use Copy for the full output."
}

/**
 * Desktop's `stripInlineDiffChrome` (fallback-model/index.ts:771-781): the
 * core's inline diff crosses raw, so the phone strips what Desktop strips —
 * the ANSI SGR sequences (`ESC [ … m`) and the leading `┊ review diff`
 * header line (case-insensitive, leading blanks tolerated).
 */
fun stripInlineDiffChrome(value: String): String {
    if (value.isEmpty()) return ""
    val noAnsi = value.replace(Regex("\u001B\\[[0-9;]*m"), "")
    val noHeader = noAnsi.replace(Regex("^\\s*┊\\s*review diff\\s*\\n", RegexOption.IGNORE_CASE), "")
    return noHeader.trim()
}
