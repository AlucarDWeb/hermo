import ComposableArchitecture
import HermoLogic

#if canImport(UIKit)
import UIKit
#endif

@DependencyClient
public struct ScreenCols: Sendable {
    public var columns: @Sendable () async -> Int = { HermoLogic.Cols.max }
}

extension ScreenCols: TestDependencyKey {
    public static let testValue = Self(columns: { 80 })
}

extension ScreenCols: DependencyKey {
    public static let liveValue = Self(
        columns: {
            #if canImport(UIKit)
            return await MainActor.run {
                // The app's own window, not the device screen: in Split View and Slide Over
                // the screen is wider than the terminal and the PTY would be sized to wrap.
                let widthPoints = UIApplication.shared.connectedScenes
                    .compactMap { $0 as? UIWindowScene }
                    .flatMap(\.windows)
                    .first(where: \.isKeyWindow)?
                    .bounds.width
                guard let widthPoints else { return HermoLogic.Cols.max }
                // `Cols.from` divides by density before the glyph width, so density 1 keeps widthPx in points.
                return HermoLogic.Cols.from(widthPx: Int(widthPoints), density: 1)
            }
            #else
            return HermoLogic.Cols.max
            #endif
        }
    )
}

extension DependencyValues {
    public var screenCols: ScreenCols {
        get { self[ScreenCols.self] }
        set { self[ScreenCols.self] = newValue }
    }
}
