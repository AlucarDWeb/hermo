import SwiftUI

/// Unpaired: the QR scan action plus the manual payload fallback field (`MainActivity.kt`'s `PairingScreen`).
public struct PairingScreen: View {
    private let pairingPayload: String
    private let errorText: String
    private let onPairingPayloadChanged: (String) -> Void
    private let onScanRequested: () -> Void
    private let onPair: () -> Void

    @Environment(\.hermoTokens) private var tokens

    public init(
        pairingPayload: String,
        errorText: String,
        onPairingPayloadChanged: @escaping (String) -> Void,
        onScanRequested: @escaping () -> Void,
        onPair: @escaping () -> Void
    ) {
        self.pairingPayload = pairingPayload
        self.errorText = errorText
        self.onPairingPayloadChanged = onPairingPayloadChanged
        self.onScanRequested = onScanRequested
        self.onPair = onPair
    }

    public var body: some View {
        VStack(spacing: 0) {
            Text("hermo")
                .font(HermoFonts.headlineLarge)
                .foregroundStyle(tokens.text)
            Text("Not paired yet. Scan the gateway's QR, or paste the payload / URL below.")
                .font(HermoFonts.bodyMedium)
                .foregroundStyle(tokens.text)
                .multilineTextAlignment(.center)
                .padding(.top, 12)
                .padding(.bottom, 16)
            Button(action: onScanRequested) {
                Text("Scan QR").frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .tint(tokens.primary)
            .accessibilityIdentifier("hermo.pairing.scanQr")
            TextField(
                "Pairing payload or gateway URL",
                text: Binding(get: { pairingPayload }, set: onPairingPayloadChanged)
            )
            .textFieldStyle(.roundedBorder)
            .padding(.top, 16)
            .accessibilityIdentifier("hermo.pairing.payloadField")
            Button(action: onPair) {
                Text("Pair").frame(maxWidth: .infinity)
            }
            .buttonStyle(.borderedProminent)
            .tint(tokens.primary)
            .padding(.top, 8)
            .accessibilityIdentifier("hermo.pairing.pair")
            if !errorText.isEmpty {
                Text(errorText)
                    .font(HermoFonts.bodyMedium)
                    .foregroundStyle(tokens.destructive)
                    .padding(.top, 8)
            }
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(tokens.background.ignoresSafeArea())
    }
}

#Preview("Light") {
    PairingScreen(
        pairingPayload: "",
        errorText: "",
        onPairingPayloadChanged: { _ in },
        onScanRequested: {},
        onPair: {}
    )
    .hermoTheme(.light)
}

#Preview("Dark") {
    PairingScreen(
        pairingPayload: "hermes://connect?v=1&url=http%3A%2F%2F127.0.0.1%3A9123&user=hermo&name=fake",
        errorText: "error: could not reach the gateway",
        onPairingPayloadChanged: { _ in },
        onScanRequested: {},
        onPair: {}
    )
    .hermoTheme(.dark)
}
