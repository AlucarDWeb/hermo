import HermoLogic
import SwiftUI

/// Wraps a top bar's buttons in one `GlassEffectContainer` so their individual `.glassEffect()` backdrops blend instead of stacking.
/// `GlassEffectContainer` overlays its children rather than arranging them, so `content` has to bring its own stack.
public struct GlassBar<Content: View>: View {
    private let content: Content

    public init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    public var body: some View {
        GlassEffectContainer {
            content
        }
        .frame(maxWidth: .infinity)
    }
}

/// A capsule pill styled with Liquid Glass, tinted for the active tab at `midground` 25 percent (Theme.kt's `midground` token, opacity called out directly rather than ported from a token).
public struct GlassPill<Content: View>: View {
    private let active: Bool
    private let content: Content
    @Environment(\.hermoTokens) private var tokens

    public init(active: Bool = false, @ViewBuilder content: () -> Content) {
        self.active = active
        self.content = content()
    }

    public var body: some View {
        content
            .glassEffect(active ? .regular.tint(tokens.midground.opacity(0.25)) : .regular, in: .capsule)
    }
}

/// The approval and clarify choice control. `primary` tints it with `primarySolid`, matching PromptCards.kt's primary/secondary `ChoiceButton` split.
public struct ChoiceButton: View {
    private let label: String
    private let primary: Bool
    private let identifier: String?
    private let action: () -> Void
    @Environment(\.hermoTokens) private var tokens

    public init(label: String, primary: Bool, identifier: String? = nil, action: @escaping () -> Void) {
        self.label = label
        self.primary = primary
        self.identifier = identifier
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            Text(label)
                .font(HermoFonts.labelSmall)
        }
        .buttonStyle(.glass)
        .tint(primary ? tokens.primarySolid : nil)
        .modifier(OptionalAccessibilityIdentifier(identifier: identifier))
    }
}

/// The tool card container. A plain surface with no glass effect: content rows never carry the glass chrome.
public struct WidgetShell<Content: View>: View {
    private let content: Content
    @Environment(\.hermoTokens) private var tokens

    public init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            content
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        // 12/10 pt padding is PromptCards.kt's WidgetShell literal, not a Theme.kt token.
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .background(tokens.widgetSurface)
        .clipShape(RoundedRectangle(cornerRadius: HermoMetrics.radius3xl))
    }
}

/// Disclosure chevron. Open renders "⌄" and collapsed renders "›", matching TranscriptRow.kt's `Chevron`, whose glyphs run the opposite way from what a `DisclosureCaret: › rotates to ⌄` name suggests.
public struct Chevron: View {
    private let open: Bool
    private let tint: Color

    public init(open: Bool, tint: Color) {
        self.open = open
        self.tint = tint
    }

    public var body: some View {
        Text(open ? "⌄" : "›")
            // 11 pt is TranscriptRow.kt:340's literal, not a Theme.kt token.
            .font(.system(size: 11, weight: .medium))
            .foregroundStyle(tint)
    }
}

/// A 14 pt cell holding a 9 pt marker, the tool/thinking row's leading marker column (TranscriptRow.kt's `ScaffoldGlyph`); no default, since some call sites pass a blank spacer while `ToolCard` supplies its own `"•"` fallback for an unmapped tool (TranscriptRow.kt:388).
public struct ScaffoldGlyph: View {
    private enum Marker {
        case text(String)
        case symbol(String)
    }

    private let marker: Marker
    private let tint: Color?
    @Environment(\.hermoTokens) private var tokens

    public init(glyph: String, tint: Color? = nil) {
        self.marker = .text(glyph)
        self.tint = tint
    }

    public init(symbol: String, tint: Color? = nil) {
        self.marker = .symbol(symbol)
        self.tint = tint
    }

    public var body: some View {
        Group {
            switch marker {
            case .text(let glyph):
                Text(glyph)
            case .symbol(let name):
                Image(systemName: name)
            }
        }
        // 14 pt cell / 9 pt marker are TranscriptRow.kt:207/214 literals, not Theme.kt tokens.
        .font(.system(size: 9, weight: .medium))
        .foregroundStyle(tint ?? tokens.scaffoldMeta)
        .frame(width: 14, alignment: .leading)
    }
}

private struct OptionalAccessibilityIdentifier: ViewModifier {
    let identifier: String?

    func body(content: Content) -> some View {
        if let identifier {
            content.accessibilityIdentifier(identifier)
        } else {
            content
        }
    }
}

private struct ThemedSwatch<Content: View>: View {
    let mode: ThemeMode
    let content: Content

    init(_ mode: ThemeMode, @ViewBuilder content: () -> Content) {
        self.mode = mode
        self.content = content()
    }

    var body: some View {
        ThemedSwatchBody(content: content)
            .hermoTheme(mode)
    }
}

private struct ThemedSwatchBody<Content: View>: View {
    @Environment(\.hermoTokens) private var tokens
    let content: Content

    var body: some View {
        content
            .padding(20)
            .background(tokens.background)
    }
}

#Preview("GlassBar") {
    HStack(spacing: 16) {
        ThemedSwatch(.light) {
            GlassBar {
                HStack {
                    Image(systemName: "line.3.horizontal")
                    Spacer()
                    Text("hermo").font(HermoFonts.titleMedium)
                    Spacer()
                    Image(systemName: "sun.max")
                }
            }
        }
        ThemedSwatch(.dark) {
            GlassBar {
                HStack {
                    Image(systemName: "line.3.horizontal")
                    Spacer()
                    Text("hermo").font(HermoFonts.titleMedium)
                    Spacer()
                    Image(systemName: "sun.max")
                }
            }
        }
    }
}

#Preview("GlassPill") {
    HStack(spacing: 16) {
        ThemedSwatch(.light) {
            HStack(spacing: 8) {
                GlassPill { Text("Session 1").font(HermoFonts.labelSmall).padding(.horizontal, 12).padding(.vertical, 6) }
                GlassPill(active: true) { Text("Session 2").font(HermoFonts.labelSmall).padding(.horizontal, 12).padding(.vertical, 6) }
            }
        }
        ThemedSwatch(.dark) {
            HStack(spacing: 8) {
                GlassPill { Text("Session 1").font(HermoFonts.labelSmall).padding(.horizontal, 12).padding(.vertical, 6) }
                GlassPill(active: true) { Text("Session 2").font(HermoFonts.labelSmall).padding(.horizontal, 12).padding(.vertical, 6) }
            }
        }
    }
}

#Preview("ChoiceButton") {
    HStack(spacing: 16) {
        ThemedSwatch(.light) {
            HStack(spacing: 8) {
                ChoiceButton(label: "Run", primary: true) {}
                ChoiceButton(label: "Cancel", primary: false) {}
            }
        }
        ThemedSwatch(.dark) {
            HStack(spacing: 8) {
                ChoiceButton(label: "Run", primary: true) {}
                ChoiceButton(label: "Cancel", primary: false) {}
            }
        }
    }
}

#Preview("WidgetShell") {
    HStack(spacing: 16) {
        ThemedSwatch(.light) {
            WidgetShell {
                Text("git diff --stat").font(HermoFonts.mono)
            }
        }
        ThemedSwatch(.dark) {
            WidgetShell {
                Text("git diff --stat").font(HermoFonts.mono)
            }
        }
    }
}

#Preview("Chevron") {
    HStack(spacing: 16) {
        ThemedSwatch(.light) {
            ThemedChevronRow()
        }
        ThemedSwatch(.dark) {
            ThemedChevronRow()
        }
    }
}

private struct ThemedChevronRow: View {
    @Environment(\.hermoTokens) private var tokens

    var body: some View {
        HStack(spacing: 12) {
            VStack { Chevron(open: false, tint: tokens.scaffoldMeta); Text("collapsed").font(.caption2) }
            VStack { Chevron(open: true, tint: tokens.scaffoldMeta); Text("open").font(.caption2) }
        }
    }
}

#Preview("ScaffoldGlyph") {
    HStack(spacing: 16) {
        ThemedSwatch(.light) {
            HStack(spacing: 8) {
                ScaffoldGlyph(glyph: "T")
                ScaffoldGlyph(symbol: "terminal")
            }
        }
        ThemedSwatch(.dark) {
            HStack(spacing: 8) {
                ScaffoldGlyph(glyph: "T")
                ScaffoldGlyph(symbol: "terminal")
            }
        }
    }
}
