import Foundation

public enum ThemeMode: String, Equatable, Sendable, CaseIterable {
    case light = "Light"
    case dark = "Dark"
    case system = "System"

    public static func fromStored(_ raw: String?) -> ThemeMode {
        let trimmed = raw?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        return allCases.first { $0.rawValue.caseInsensitiveCompare(trimmed) == .orderedSame } ?? .system
    }
}

public func resolve(_ mode: ThemeMode, systemDark: Bool) -> Bool {
    switch mode {
    case .light: return false
    case .dark: return true
    case .system: return systemDark
    }
}
