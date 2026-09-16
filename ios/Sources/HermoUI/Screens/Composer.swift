import SwiftUI

/// The message input at the foot of the chat screen (`ChatScreen.kt`'s `Composer`): a glass
/// capsule shell holding the growing text field, the model pill, and the Send/Stop control.
public struct Composer: View {
    private let model: String
    private let running: Bool
    private let draft: String
    private let prefill: String?
    private let onDraftChanged: (String) -> Void
    private let onSend: (String) -> Void
    private let onStop: () -> Void

    @FocusState private var focused: Bool
    @Environment(\.hermoTokens) private var tokens

    /// `prefill` replaces Android's `composerEvents` stream: the caller (owning the slash
    /// outcome that wants to fill the draft) passes the text it wants shown, and any change to
    /// that value rewrites `draft` the same way a `Prefill` event does on Android.
    public init(
        model: String,
        running: Bool,
        draft: String,
        prefill: String? = nil,
        onDraftChanged: @escaping (String) -> Void,
        onSend: @escaping (String) -> Void,
        onStop: @escaping () -> Void
    ) {
        self.model = model
        self.running = running
        self.draft = draft
        self.prefill = prefill
        self.onDraftChanged = onDraftChanged
        self.onSend = onSend
        self.onStop = onStop
    }

    private var sendEnabled: Bool {
        running || !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    public var body: some View {
        HStack(alignment: .bottom, spacing: HermoMetrics.composerControlGap) {
            TextField(
                "",
                text: Binding(get: { draft }, set: onDraftChanged),
                axis: .vertical
            )
            .lineLimit(1...7)
            // The draft goes to an agent verbatim: iOS would capitalise the first
            // word and autocorrect a slash command's name.
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .font(HermoFonts.bodyMedium)
            .foregroundStyle(tokens.text)
            .tint(tokens.midground)
            .focused($focused)
            .frame(minHeight: HermoMetrics.composerControlRowHeight)
            .accessibilityIdentifier("hermo.chat.composer.field")
            if !model.isEmpty {
                ModelPill(model: model)
            }
            Button {
                if running {
                    onStop()
                } else if !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    onSend(draft)
                    // FIX8 item 6 on Android: the send path hides the keyboard before clearing the draft.
                    focused = false
                    onDraftChanged("")
                }
            } label: {
                Text(running ? "Stop" : "Send")
            }
            .buttonStyle(.glassProminent)
            .tint(tokens.primary)
            .disabled(!sendEnabled)
            .accessibilityIdentifier(running ? "hermo.chat.composer.stop" : "hermo.chat.composer.send")
        }
        .padding(.horizontal, HermoMetrics.composerSurfacePadX)
        .padding(.vertical, HermoMetrics.composerSurfacePadY)
        .glassEffect(.regular.interactive(), in: .capsule)
        .overlay(
            Capsule().strokeBorder(focused ? tokens.midground : tokens.strokeTertiary, lineWidth: 1)
        )
        .contentShape(.capsule)
        .onTapGesture { focused = true }
        .padding(12)
        .onChange(of: prefill) { _, newValue in
            if let newValue {
                onDraftChanged(newValue)
            }
        }
    }
}

/// The model name, relocated from Desktop's dropdown trigger (`model-pill.tsx`) to a static
/// label: the phone has no model picker yet, so there is no chevron and no press affordance.
private struct ModelPill: View {
    let model: String
    @Environment(\.hermoTokens) private var tokens

    var body: some View {
        Text(model)
            // Kotlin's ModelPill is `FontWeight.Normal`, not `HermoFonts.labelSmall`'s medium.
            .font(.system(size: HermoMetrics.convToolFontSize))
            .foregroundStyle(tokens.textTertiary)
            .lineLimit(1)
            .frame(maxWidth: HermoMetrics.composerPillMaxWidth, alignment: .leading)
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
    }
}

private struct ComposerPreview: View {
    let model: String
    let running: Bool
    let draft: String

    @State private var text: String

    init(model: String, running: Bool, draft: String) {
        self.model = model
        self.running = running
        self.draft = draft
        self._text = State(initialValue: draft)
    }

    var body: some View {
        Composer(
            model: model,
            running: running,
            draft: text,
            onDraftChanged: { text = $0 },
            onSend: { _ in },
            onStop: {}
        )
    }
}

#Preview("Light") {
    VStack(spacing: 8) {
        ComposerPreview(model: "claude-sonnet-4.5", running: false, draft: "")
        ComposerPreview(model: "claude-sonnet-4.5", running: false, draft: "hi there")
        ComposerPreview(model: "claude-sonnet-4.5", running: true, draft: "")
        ComposerPreview(model: "", running: false, draft: "")
    }
    .hermoTheme(.light)
}

#Preview("Dark") {
    VStack(spacing: 8) {
        ComposerPreview(model: "claude-sonnet-4.5", running: false, draft: "")
        ComposerPreview(model: "claude-sonnet-4.5", running: false, draft: "hi there")
        ComposerPreview(model: "claude-sonnet-4.5", running: true, draft: "")
        ComposerPreview(model: "", running: false, draft: "")
    }
    .hermoTheme(.dark)
}
