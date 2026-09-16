import Foundation

/// The gateway's canonical approval choices (ui-tui prompts.tsx), with Desktop's i18n
/// labels (en.ts approval block). A wire string with no matching case renders nothing
/// here; the caller falls back to the server's own text rather than inventing a label.
public enum ApprovalChoice: Sendable, Equatable, CaseIterable {
    case run
    case allowSession
    case always
    case reject

    public var wire: String {
        switch self {
        case .run: return "once"
        case .allowSession: return "session"
        case .always: return "always"
        case .reject: return "deny"
        }
    }

    public var label: String {
        switch self {
        case .run: return "Run"
        case .allowSession: return "Allow this session"
        case .always: return "Always allow"
        case .reject: return "Reject"
        }
    }

    public static func fromWire(_ wire: String) -> ApprovalChoice? {
        allCases.first { $0.wire == wire }
    }
}

/// The single choice the row highlights: primary is Run, else Reject.
public func primaryApprovalChoice(_ choices: [String]) -> ApprovalChoice? {
    guard let wire = choices.first(where: { $0 == "once" }) ?? choices.first(where: { $0 == "deny" }) else {
        return nil
    }
    return ApprovalChoice.fromWire(wire)
}

/// The secondary choices in the order the server sent them, minus the primary.
public func secondaryApprovalChoices(_ choices: [String]) -> [ApprovalChoice] {
    let primary = primaryApprovalChoice(choices)
    return choices.compactMap(ApprovalChoice.fromWire).filter { $0 != primary }
}

/// `always` renders only when the server's choices carry it; a missing choice is never synthesised.
public func shouldRenderAlways(_ choices: [String]) -> Bool {
    choices.contains("always")
}

public enum ApprovalCopy {
    public static let alwaysTitle = "Always allow this command?"
    public static let alwaysBody = "Hermes won't ask again for commands like this — in this session or any future one."
    public static let alwaysConfirm = "Always allow"
    public static let alwaysCancel = "Cancel"
    public static let jumpToApproval = "Approval needed"
    public static let resolved = "Answered elsewhere"
}

/// Gateway codes 4009 (no pending request) and 4018 (stale target) mean the approval
/// was answered elsewhere, not an error dialog; anything else has no mapped copy.
public func approvalRespondErrorCopy(_ message: String) -> String? {
    (message.contains("rpc error 4009") || message.contains("rpc error 4018")) ? ApprovalCopy.resolved : nil
}

/// One question of a clarify card, decoded from the core's `questions[]`.
public struct ClarifyQuestionUi: Sendable, Equatable {
    public let qid: String
    public let question: String
    public let choices: [String]
    public let multiSelect: Bool

    public init(qid: String, question: String, choices: [String], multiSelect: Bool) {
        self.qid = qid
        self.question = question
        self.choices = choices
        self.multiSelect = multiSelect
    }
}

/// Decodes the raw compact JSON of `questions[]`. Garbage or an empty string yields
/// an empty list, never a crash; a non-object element or a blank question is skipped.
public func parseClarifyQuestions(_ questionsJson: String) -> [ClarifyQuestionUi] {
    guard !questionsJson.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
          let data = questionsJson.data(using: .utf8),
          let parsed = try? JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed]),
          let array = parsed as? [Any]
    else {
        return []
    }

    var out: [ClarifyQuestionUi] = []
    out.reserveCapacity(array.count)
    for element in array {
        guard let dict = element as? [String: Any] else { continue }
        let entry = LooseJSON(dict)
        let text = entry.optString("question").trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { continue }
        let choices = entry.optArray("choices")
            .map { $0.stringValue().trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
        out.append(
            ClarifyQuestionUi(
                qid: entry.optString("qid"),
                question: text,
                choices: choices,
                multiSelect: entry.optBool("multi_select") && !choices.isEmpty
            )
        )
    }
    return out
}

/// True when a multi-select answer should travel as a JSON array.
public func isMultiSelectAnswer(_ question: ClarifyQuestionUi, picks: [String]) -> Bool {
    question.multiSelect && !picks.isEmpty
}

/// A multi-select reply is the JSON array the gateway's parser reads first; a
/// single-select reply is the pick itself. Draft text only applies when nothing was picked.
public func encodeClarifyAnswer(_ question: ClarifyQuestionUi, picks: [String], draft: String) -> String {
    if !picks.isEmpty && question.multiSelect,
       let data = try? JSONSerialization.data(withJSONObject: picks),
       let encoded = String(data: data, encoding: .utf8) {
        return encoded
    }
    if let first = picks.first {
        return first
    }
    return draft.trimmingCharacters(in: .whitespacesAndNewlines)
}

/// Desktop's batch progress line (en.ts clarify.questionProgress).
public func clarifyProgressLabel(_ answered: Int, _ total: Int) -> String {
    "\(answered) of \(total) answered"
}
