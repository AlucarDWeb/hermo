/// Port of the settled-label ladder and body-visibility rule pinned by
/// `ThinkingLabelTest.kt`'s `settledLabel`/`open` helpers and mirrored from
/// `TranscriptRow.kt`'s `ThinkingRow`.
public enum ThinkingLabel {

    /// Desktop's three-way settled label (`message-parts.tsx:165-175`): `Thought` with no
    /// measurement, `Thought briefly` when the whole seconds round to 0, `Thought for Xs`
    /// otherwise.
    public static func settledLabel(measuredS: Int64?) -> String {
        guard let measuredS else { return "Thought" }
        if measuredS < 1 { return "Thought briefly" }
        return "Thought for \(ToolCardModel.formatElapsed(measuredS))"
    }

    /// The body's visibility: it follows the live flag, and the user's toggle outranks it in
    /// both directions. A declared divergence from Desktop, which latches the body open past
    /// settle; here it collapses to the label unless the user has toggled it explicitly.
    public static func isOpen(live: Bool, userToggle: Bool?) -> Bool {
        userToggle ?? live
    }
}
