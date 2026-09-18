import HermoLogic
import SwiftUI

/// The theme picker sheet (Android's `AppearanceSheet.kt`): renders the mode it is handed and
/// reports a choice up, `AppearanceFeature` owns loading and persisting it.
public struct AppearanceSheet: View {
    private let mode: ThemeMode
    private let onModeChange: (ThemeMode) -> Void

    @Environment(\.hermoTokens) private var tokens

    public init(mode: ThemeMode, onModeChange: @escaping (ThemeMode) -> Void) {
        self.mode = mode
        self.onModeChange = onModeChange
    }

    public var body: some View {
        VStack(spacing: 0) {
            Text("Appearance")
                .font(HermoFonts.bodyMedium.weight(.medium))
                .foregroundStyle(tokens.textSecondary)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
            Rectangle()
                .fill(tokens.strokeTertiary)
                .frame(height: 0.5)
            ForEach(ThemeMode.allCases, id: \.self) { candidate in
                AppearanceRow(mode: candidate, selected: candidate == mode) {
                    onModeChange(candidate)
                }
            }
        }
        .padding(.vertical, 8)
        .background(tokens.elevated)
        .presentationDetents([.medium])
    }
}

private struct AppearanceRow: View {
    let mode: ThemeMode
    let selected: Bool
    let onSelect: () -> Void

    @Environment(\.hermoTokens) private var tokens

    var body: some View {
        Button(action: onSelect) {
            HStack(spacing: 8) {
                Text(mode.rawValue)
                    .font(HermoFonts.bodyMedium)
                    .foregroundStyle(tokens.text)
                Spacer()
                if selected {
                    Circle()
                        .fill(tokens.midground)
                        .frame(width: 8, height: 8)
                        .padding(4)
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("hermo.appearance.\(mode.rawValue.lowercased())")
    }
}

#Preview("Light") {
    Color.clear
        .sheet(isPresented: .constant(true)) {
            AppearanceSheet(mode: .system, onModeChange: { _ in })
                .hermoTheme(.light)
        }
}

#Preview("Dark") {
    Color.clear
        .sheet(isPresented: .constant(true)) {
            AppearanceSheet(mode: .dark, onModeChange: { _ in })
                .hermoTheme(.dark)
        }
}
