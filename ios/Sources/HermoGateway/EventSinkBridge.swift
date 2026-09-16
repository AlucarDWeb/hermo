import Foundation
import HermesCore

/// UniFFI invokes `onTranscript` and `onConnection` from a tokio worker thread, and each call
/// to `events` gets its own stream: one cancelled consumer must not terminate the shared
/// stream, and two consumers must not split the events between them. Events that arrive while
/// nobody is listening are parked (bounded) and replayed to the next subscriber, which mirrors
/// the unlimited channel the Android repository drains.
///
/// Every mutation and every yield runs on one serial queue, so a subscriber that arrives while
/// events are in flight still sees the parked ones before the live ones.
public final class EventSinkBridge: EventSink, @unchecked Sendable {
    private static let parkedMax = 512

    private let queue = DispatchQueue(label: "sh.mo.event-sink-bridge")
    private var continuations: [Int: AsyncStream<CoreEvent>.Continuation] = [:]
    private var nextId = 0
    private var parked: [CoreEvent] = []

    public init() {}

    public var events: AsyncStream<CoreEvent> {
        AsyncStream(bufferingPolicy: .unbounded) { continuation in
            let id = queue.sync { register(continuation) }
            continuation.onTermination = { [weak self] _ in
                self?.queue.async { self?.continuations.removeValue(forKey: id) }
            }
        }
    }

    public func onTranscript(change: TranscriptChangeDto) {
        deliver(.transcript(change))
    }

    public func onConnection(status: ConnectionStatus) {
        deliver(.connection(status))
    }

    /// Caller holds the queue.
    private func register(_ continuation: AsyncStream<CoreEvent>.Continuation) -> Int {
        nextId += 1
        continuations[nextId] = continuation
        for event in parked {
            continuation.yield(event)
        }
        parked.removeAll()
        return nextId
    }

    private func deliver(_ event: CoreEvent) {
        queue.async { [self] in
            if continuations.isEmpty {
                parked.append(event)
                if parked.count > Self.parkedMax {
                    parked.removeFirst(parked.count - Self.parkedMax)
                }
                return
            }
            for continuation in continuations.values {
                continuation.yield(event)
            }
        }
    }
}
