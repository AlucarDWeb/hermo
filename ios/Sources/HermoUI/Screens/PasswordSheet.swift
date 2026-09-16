import SwiftUI

/// The credential sheet, shown standalone on first pair or as an overlay on session expiry (`Screens.kt`'s `PasswordSheet`).
public struct PasswordSheet: View {
    private let endpoint: String
    private let errorText: String
    private let overlay: Bool
    private let onSubmit: (String) -> Void
    private let onLogout: () -> Void

    @State private var password = ""
    @Environment(\.hermoTokens) private var tokens

    /// `overlay` drops the sheet's own background so the caller's ready screen stays visible
    /// underneath, which is the whole point of the mid-session variant.
    public init(
        endpoint: String,
        errorText: String,
        overlay: Bool = false,
        onSubmit: @escaping (String) -> Void,
        onLogout: @escaping () -> Void
    ) {
        self.endpoint = endpoint
        self.errorText = errorText
        self.overlay = overlay
        self.onSubmit = onSubmit
        self.onLogout = onLogout
    }

    public var body: some View {
        VStack(spacing: 0) {
            Text("Password for \(endpoint)")
                .font(HermoFonts.titleMedium)
                .foregroundStyle(tokens.text)
            // A declared divergence from Android's unmasked field.
            SecureField("Password", text: $password)
                .textFieldStyle(.roundedBorder)
                .padding(.top, 12)
                .accessibilityIdentifier("hermo.password.field")
            Button(action: { onSubmit(password) }) {
                Text("Sign in").frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .tint(tokens.primary)
            .padding(.top, 8)
            .disabled(password.isEmpty)
            .accessibilityIdentifier("hermo.password.signIn")
            Button("Wrong gateway? Log out", action: onLogout)
                .buttonStyle(.plain)
                .foregroundStyle(tokens.destructive)
                .padding(.top, 4)
                .accessibilityIdentifier("hermo.password.logOut")
            if !errorText.isEmpty {
                Text(errorText)
                    .font(HermoFonts.bodyMedium)
                    .foregroundStyle(tokens.destructive)
                    .padding(.top, 8)
            }
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background {
            if !overlay {
                tokens.background.ignoresSafeArea()
            }
        }
    }
}

#Preview("Light") {
    PasswordSheet(
        endpoint: "gateway (hermo) — http://127.0.0.1:9123/",
        errorText: "",
        onSubmit: { _ in },
        onLogout: {}
    )
    .hermoTheme(.light)
}

#Preview("Dark") {
    PasswordSheet(
        endpoint: "gateway (hermo) — http://127.0.0.1:9123/",
        errorText: "error: invalid credentials",
        onSubmit: { _ in },
        onLogout: {}
    )
    .hermoTheme(.dark)
}
