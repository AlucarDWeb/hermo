import Foundation

/// One transcript row, decoded from the core's row JSON. Unknown kinds and
/// garbage JSON both degrade to a `.status` row rather than throwing.
public enum ChatRow: Equatable, Sendable {
    case user(id: Int, text: String)
    case assistant(id: Int, text: String, streaming: Bool, warning: String, usageJson: String)
    case thinking(id: Int, text: String)
    case tool(
        id: Int,
        name: String,
        complete: Bool,
        context: String,
        argsJson: String,
        resultJson: String,
        inlineDiff: String,
        durationS: Double,
        exitCode: Int?
    )
    case approval(
        id: Int,
        requestId: String,
        command: String,
        description: String,
        choices: [String],
        resolved: Bool
    )
    case clarify(id: Int, requestId: String, questions: String, resolved: Bool)
    case status(id: Int, kind: String, text: String)
    case error(id: Int, message: String)

    public var id: Int {
        switch self {
        case .user(let id, _): return id
        case .assistant(let id, _, _, _, _): return id
        case .thinking(let id, _): return id
        case .tool(let id, _, _, _, _, _, _, _, _): return id
        case .approval(let id, _, _, _, _, _): return id
        case .clarify(let id, _, _, _): return id
        case .status(let id, _, _): return id
        case .error(let id, _): return id
        }
    }
}

/// Defensive: a JSON parse failure yields a `.status` row with kind `"unknown"`,
/// never an exception.
public func parseChatRow(_ id: Int, _ rowJson: String) -> ChatRow {
    guard let obj = LooseJSON(rowJson), obj.isObject else {
        return .status(id: id, kind: "unknown", text: "")
    }
    switch obj.optString("kind") {
    case "user":
        return .user(id: id, text: obj.optString("text"))
    case "assistant":
        return .assistant(
            id: id,
            text: obj.optString("text"),
            streaming: obj.optBool("streaming"),
            warning: obj.optString("warning"),
            usageJson: obj.optString("usage")
        )
    case "thinking":
        return .thinking(id: id, text: obj.optString("text"))
    case "tool":
        return .tool(
            id: id,
            name: obj.optString("name"),
            complete: obj.optBool("complete"),
            context: obj.optString("context"),
            argsJson: obj.optString("args"),
            resultJson: obj.optString("result"),
            inlineDiff: inlineDiffOf(obj),
            durationS: obj.optDouble("duration_s"),
            exitCode: exitCodeOf(obj)
        )
    case "approval":
        // choices is the server's own list, rendered verbatim: parsed as strings and never synthesised.
        return .approval(
            id: id,
            requestId: obj.optString("request_id"),
            command: obj.optString("command"),
            description: obj.optString("description"),
            choices: obj.optArray("choices").map { $0.stringValue() }.filter { !isBlank($0) },
            resolved: obj.optBool("resolved")
        )
    case "clarify":
        return .clarify(
            id: id,
            requestId: obj.optString("request_id"),
            questions: questionsJson(obj),
            resolved: obj.optBool("resolved")
        )
    case "status":
        return .status(id: id, kind: obj.optString("status"), text: obj.optString("text"))
    case "error":
        return .error(id: id, message: obj.optString("message"))
    default:
        return .status(id: id, kind: obj.optString("kind"), text: "")
    }
}

private func questionsJson(_ obj: LooseJSON) -> String {
    let items = obj.optArray("questions")
    if items.isEmpty { return "[]" }
    return "[" + items.map { $0.compact() }.joined(separator: ",") + "]"
}

/// The diff hides under `inline_diff` or `diff`, either at the top level or
/// inside `result` (itself a JSON string, so it has to be reparsed): the first
/// non-blank match wins.
private func inlineDiffOf(_ obj: LooseJSON) -> String {
    let result = LooseJSON(obj.optString("result"))
    let sources = [
        obj.optString("inline_diff"),
        obj.optString("diff"),
        result?.optString("inline_diff") ?? "",
        result?.optString("diff") ?? "",
    ]
    return sources.first { !isBlank($0) } ?? ""
}

/// `0` is a real exit code and must survive, hence the presence check rather
/// than a default that cannot tell zero from absence.
private func exitCodeOf(_ obj: LooseJSON) -> Int? {
    if obj.has("exit_code") { return obj.optInt("exit_code") }
    guard let result = LooseJSON(obj.optString("result")), result.has("exit_code") else {
        return nil
    }
    return result.optInt("exit_code")
}

private func isBlank(_ s: String) -> Bool {
    s.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
}
