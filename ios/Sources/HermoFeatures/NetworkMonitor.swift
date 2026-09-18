import ComposableArchitecture
import Foundation
import Network

@DependencyClient
public struct NetworkMonitor: Sendable {
    public var events: @Sendable () -> AsyncStream<Void> = { AsyncStream { $0.finish() } }
}

extension NetworkMonitor: TestDependencyKey {
    public static let testValue = Self()
}

extension NetworkMonitor: DependencyKey {
    public static let liveValue = Self(
        events: {
            AsyncStream { continuation in
                let monitor = NWPathMonitor()
                monitor.pathUpdateHandler = { path in
                    if path.status == .satisfied {
                        continuation.yield()
                    }
                }
                continuation.onTermination = { _ in monitor.cancel() }
                monitor.start(queue: DispatchQueue(label: "sh.mo.hermo.network-monitor"))
            }
        }
    )
}

extension DependencyValues {
    public var networkMonitor: NetworkMonitor {
        get { self[NetworkMonitor.self] }
        set { self[NetworkMonitor.self] = newValue }
    }
}
