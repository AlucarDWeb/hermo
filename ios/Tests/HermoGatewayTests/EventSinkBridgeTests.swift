import XCTest
import HermesCore
import HermoGateway

private struct TimeoutError: Error {}

final class EventSinkBridgeTests: XCTestCase {
    func testEventsArriveInYieldOrder() async throws {
        let bridge = EventSinkBridge()
        let transcript = TranscriptChangeDto(
            key: "session-1",
            kind: .rowAppended,
            index: 0,
            rowJson: "{}",
            title: "Session"
        )

        Task.detached {
            bridge.onTranscript(change: transcript)
            bridge.onConnection(status: .open)
        }

        let received = try await withThrowingTaskGroup(of: [CoreEvent].self) { group -> [CoreEvent] in
            group.addTask {
                var collected: [CoreEvent] = []
                for await event in bridge.events {
                    collected.append(event)
                    if collected.count == 2 { break }
                }
                return collected
            }
            group.addTask {
                try await Task.sleep(nanoseconds: 5_000_000_000)
                throw TimeoutError()
            }
            guard let result = try await group.next() else {
                throw TimeoutError()
            }
            group.cancelAll()
            return result
        }

        XCTAssertEqual(received, [.transcript(transcript), .connection(.open)])
    }

    func testStreamSurvivesACancelledConsumer() async throws {
        let bridge = EventSinkBridge()

        let first = Task {
            for await _ in bridge.events { break }
        }
        bridge.onConnection(status: .connecting)
        _ = await first.value
        first.cancel()

        let second = Task { () -> CoreEvent? in
            for await event in bridge.events { return event }
            return nil
        }
        try await Task.sleep(nanoseconds: 100_000_000)
        bridge.onConnection(status: .open)

        let received = await second.value
        XCTAssertEqual(received, .connection(.open))
    }

    func testEveryConsumerSeesEveryEvent() async throws {
        let bridge = EventSinkBridge()
        let firstStream = bridge.events
        let secondStream = bridge.events

        let first = Task { () -> CoreEvent? in
            for await event in firstStream { return event }
            return nil
        }
        let second = Task { () -> CoreEvent? in
            for await event in secondStream { return event }
            return nil
        }
        try await Task.sleep(nanoseconds: 100_000_000)
        bridge.onConnection(status: .open)

        let a = await first.value
        let b = await second.value
        XCTAssertEqual(a, .connection(.open))
        XCTAssertEqual(b, .connection(.open))
    }

    func testParkedEventsReplayBeforeLiveOnes() async throws {
        let bridge = EventSinkBridge()
        func change(_ key: String) -> TranscriptChangeDto {
            TranscriptChangeDto(key: key, kind: .rowAppended, index: 0, rowJson: "{}", title: "t")
        }

        // Nobody is listening yet: these park.
        bridge.onTranscript(change: change("a"))
        bridge.onTranscript(change: change("b"))

        let stream = bridge.events
        bridge.onConnection(status: .open)

        let received = try await withThrowingTaskGroup(of: [CoreEvent].self) { group -> [CoreEvent] in
            group.addTask {
                var collected: [CoreEvent] = []
                for await event in stream {
                    collected.append(event)
                    if collected.count == 3 { break }
                }
                return collected
            }
            group.addTask {
                try await Task.sleep(nanoseconds: 5_000_000_000)
                throw TimeoutError()
            }
            guard let result = try await group.next() else { throw TimeoutError() }
            group.cancelAll()
            return result
        }

        XCTAssertEqual(
            received,
            [.transcript(change("a")), .transcript(change("b")), .connection(.open)]
        )
    }
}
