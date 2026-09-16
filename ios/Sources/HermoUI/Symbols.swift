import HermoLogic
import SwiftUI

/// SF Symbol for a tool's glyph key. Unmapped glyphs and unknown tool names both land here, matching the Kotlin `?: "•"` default at TranscriptRow.kt:389.
private let defaultToolSymbol = "circle.fill"

public func toolSymbol(_ name: String) -> String {
    switch ToolCardModel.toolGlyph(name) {
    case "globe": "globe"
    case "image": "photo"
    case "question": "questionmark.circle"
    case "watch": "clock"
    case "edit": "pencil"
    case "terminal": "terminal"
    case "files": "folder"
    case "brain": "brain"
    case "file": "doc.text"
    case "search": "magnifyingglass"
    case "tools": "checklist"
    case "eye": "eye"
    default: defaultToolSymbol
    }
}

public func appearanceSymbol(_ mode: ThemeMode) -> String {
    switch mode {
    case .light: "sun.max"
    case .dark: "moon"
    case .system: "circle.lefthalf.filled"
    }
}

#Preview("Tool and appearance symbols") {
    let toolSamples: [(glyphKey: String, sampleTool: String)] = [
        ("globe", "browser_navigate"),
        ("image", "browser_take_screenshot"),
        ("question", "clarify"),
        ("watch", "cronjob"),
        ("edit", "edit_file"),
        ("terminal", "execute_code"),
        ("files", "list_files"),
        ("brain", "memory"),
        ("file", "read_file"),
        ("search", "web_search"),
        ("tools", "todo"),
        ("eye", "vision_analyze"),
        ("(default)", "unmapped_tool"),
    ]
    let appearanceSamples: [(label: String, mode: ThemeMode)] = [
        ("light", .light),
        ("dark", .dark),
        ("system", .system),
    ]
    let columns = [GridItem(.adaptive(minimum: 110), spacing: 16)]

    return ScrollView {
        VStack(alignment: .leading, spacing: 20) {
            Text("Tool glyphs").font(.headline)
            LazyVGrid(columns: columns, spacing: 16) {
                ForEach(toolSamples, id: \.sampleTool) { sample in
                    VStack(spacing: 4) {
                        Image(systemName: toolSymbol(sample.sampleTool))
                            .font(.title2)
                        Text(sample.glyphKey)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }
            }

            Text("Appearance").font(.headline)
            LazyVGrid(columns: columns, spacing: 16) {
                ForEach(appearanceSamples, id: \.label) { sample in
                    VStack(spacing: 4) {
                        Image(systemName: appearanceSymbol(sample.mode))
                            .font(.title2)
                        Text(sample.label)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }
            }
        }
        .padding()
    }
}
