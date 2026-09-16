import ComposableArchitecture
import HermesCore
import SwiftUI

@Reducer
struct AppShell {
    @ObservableState
    struct State: Equatable {}

    enum Action {}

    var body: some ReducerOf<Self> {
        Reduce { _, _ in
            .none
        }
    }
}

@main
struct HermoApp: App {
    static let store = Store(initialState: AppShell.State()) {
        AppShell()
    }

    var body: some Scene {
        WindowGroup {
            RootView(store: Self.store)
        }
    }
}

struct RootView: View {
    let store: StoreOf<AppShell>

    var body: some View {
        VStack(spacing: 8) {
            Text("hermo")
                .font(.largeTitle.weight(.semibold))
                .accessibilityIdentifier("hermo.shell.title")
            Text("Not paired yet")
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("hermo.shell.subtitle")
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
