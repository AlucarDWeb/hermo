import XCTest
import HermoLogic

/// Pins the transcript-fidelity fix: a `rowUpdated` past the end of the local
/// list used to be dropped, which desynchronised every later index, and a
/// `reset` used to leave the screen empty since the app's transcript is only
/// what the change stream delivers. These tests fail against the old
/// drop / blind-overwrite semantics and pass against `applyTranscriptChange`.
final class TranscriptStateTests: XCTestCase {

    private func appends(_ jsons: String...) -> [String] {
        var rows: [String] = []
        for (i, json) in jsons.enumerated() {
            rows = applyTranscriptChange(rows, kind: "rowAppended", index: Int64(i), rowJson: json)
        }
        return rows
    }

    private let toolJson =
        #"{"kind":"tool","tool_id":"t1","name":"terminal","complete":true,"args":{},"result":"ok","duration_s":0.4}"#
    private let assistantJson = #"{"kind":"assistant","text":"hi","streaming":false}"#

    func testRowUpdatedPastTheEndPadsAndLandsAtTheRightIndex() {
        let rows = appends(#"{"kind":"user","text":"q"}"#, assistantJson)
        // A tool card updates at index 2 while the list holds 2 rows (0,1):
        // the old code dropped it, the pinned fix pads and keeps the index.
        let out = applyTranscriptChange(rows, kind: "rowUpdated", index: 2, rowJson: toolJson)
        XCTAssertEqual(out.count, 3)
        XCTAssertEqual(out[2], toolJson)
        XCTAssertEqual(out[0], #"{"kind":"user","text":"q"}"#)
    }

    func testAMidTurnCollapseScenarioKeepsEveryRowAddressable() {
        // Stream shape of a real turn: user row, tool updates racing ahead of
        // an append, assistant streaming updates. Every index must land.
        var rows = appends(#"{"kind":"user","text":"do it"}"#)
        rows = applyTranscriptChange(rows, kind: "rowAppended", index: 1, rowJson: toolJson)
        rows = applyTranscriptChange(rows, kind: "rowUpdated", index: 2, rowJson: #"{"kind":"thinking","text":"hmm"}"#)
        rows = applyTranscriptChange(rows, kind: "rowUpdated", index: 3, rowJson: assistantJson)
        rows = applyTranscriptChange(rows, kind: "rowUpdated", index: 1, rowJson: toolJson) // re-complete
        XCTAssertEqual(rows.count, 4)
        XCTAssertTrue(rows[3].contains("\"assistant\""))
        XCTAssertTrue(rows[1].contains("\"tool\""))
    }

    func testResetClearsTheRowsAndTheCoresRowAppendedsRebuildThem() {
        let coreRows = [#"{"kind":"user","text":"q"}"#, assistantJson]
        // RESET honours the clear.
        var rows = applyTranscriptChange(coreRows, kind: "reset", index: Int64.max, rowJson: "")
        XCTAssertEqual(rows.count, 0)
        // The rebuilt rows arrive right after as one rowAppended each FROM
        // THE CORE. No app-side snapshot replay: rebuild from the core's row
        // JSON only, never from whatever the app last held.
        for (i, json) in coreRows.enumerated() {
            rows = applyTranscriptChange(rows, kind: "rowAppended", index: Int64(i), rowJson: json)
        }
        XCTAssertEqual(rows, coreRows)
    }

    func testAReplayedAppendDoesNotDuplicateItsRow() {
        let rows = appends(#"{"kind":"user","text":"q"}"#)
        let again = applyTranscriptChange(rows, kind: "rowAppended", index: 0, rowJson: #"{"kind":"user","text":"q"}"#)
        XCTAssertEqual(again.count, 1)
    }

    // The session map may only grow for a key the repository considers
    // open-or-restoring. The resume race itself is covered at the adapter,
    // which seeds pendingKeys before the core call; this pure function only
    // sees the resulting knownKeys set.

    func testApplySessionChangeKeepsResumeRowsForARestoringKey() {
        let user = #"{"kind":"user","text":"pong"}"#
        var sessions: [String: SessionUiState] = [:]
        sessions = applySessionChange(sessions: sessions, key: "k", kind: "reset", index: 0, rowJson: "", knownKeys: ["k"])
        sessions = applySessionChange(sessions: sessions, key: "k", kind: "rowAppended", index: 0, rowJson: user, knownKeys: ["k"])
        XCTAssertEqual(sessions["k"]?.rows, [user])
    }

    func testApplySessionChangeDropsEventsForAKeyOutsideKnownKeys() {
        // A late change for a key closed via closeTab must not resurrect a phantom entry.
        let user = #"{"kind":"user","text":"pong"}"#
        var sessions = applySessionChange(sessions: [:], key: "k", kind: "reset", index: 0, rowJson: "", knownKeys: ["open"])
        XCTAssertTrue(sessions.isEmpty)
        sessions = applySessionChange(sessions: sessions, key: "k", kind: "rowAppended", index: 0, rowJson: user, knownKeys: ["open"])
        XCTAssertTrue(sessions.isEmpty)
    }

    func testApplySessionChangeDropsHeaderUpdatedForAnUnknownKey() {
        // A header for an unknown key must not mint an empty entry either, same gate as above.
        let sessions = applySessionChange(sessions: [:], key: "k", kind: "headerUpdated", index: 0, rowJson: "", knownKeys: ["open"])
        XCTAssertTrue(sessions.isEmpty)
    }

    func testApplySessionChangeHeaderUpdatedWithATitleUpdatesSessionUiStateTitle() {
        var sessions = applySessionChange(sessions: [:], key: "k", kind: "rowAppended", index: 0, rowJson: #"{"kind":"user","text":"q"}"#, knownKeys: ["k"])
        sessions = applySessionChange(sessions: sessions, key: "k", kind: "headerUpdated", index: 0, rowJson: "", knownKeys: ["k"], title: "my chat")
        XCTAssertEqual(sessions["k"]?.title, "my chat")
    }

    func testApplySessionChangeHeaderUpdatedWithAnEmptyTitleKeepsTheOldOne() {
        // An empty title must never blank a known one.
        var sessions = applySessionChange(sessions: [:], key: "k", kind: "headerUpdated", index: 0, rowJson: "", knownKeys: ["k"], title: "my chat")
        sessions = applySessionChange(sessions: sessions, key: "k", kind: "headerUpdated", index: 0, rowJson: "", knownKeys: ["k"], title: "")
        XCTAssertEqual(sessions["k"]?.title, "my chat")
    }

    func testApplySessionChangeHeaderUpdatedLeavesTheRowListUntouched() {
        let user = #"{"kind":"user","text":"q"}"#
        var sessions = applySessionChange(sessions: [:], key: "k", kind: "rowAppended", index: 0, rowJson: user, knownKeys: ["k"])
        sessions = applySessionChange(sessions: sessions, key: "k", kind: "headerUpdated", index: 0, rowJson: "", knownKeys: ["k"], title: "my chat")
        XCTAssertEqual(sessions["k"]?.rows, [user])
    }

    func testEnsureSessionDoesNotWipeRowsAlreadyApplied() {
        let user = #"{"kind":"user","text":"pong"}"#
        var sessions = applySessionChange(sessions: [:], key: "k", kind: "rowAppended", index: 0, rowJson: user, knownKeys: ["k"])
        sessions = ensureSession(sessions, "k")
        XCTAssertEqual(sessions["k"]?.rows, [user])
        sessions = ensureSession(sessions, "other")
        XCTAssertEqual(sessions["k"]?.rows, [user])
        XCTAssertEqual(sessions["other"]?.rows, [])
    }
}

final class RemoteSessionRowTests: XCTestCase {

    func testBlankTitleFallsBackToNewSession() {
        let row = RemoteSessionRow(id: "s1", title: "   ", preview: "hi", messageCount: 1)

        XCTAssertEqual(row.displayTitle, "New session")
    }

    func testBlankPreviewTrimsToAnEmptyStringWithNoSubstituteCopy() {
        let row = RemoteSessionRow(id: "s1", title: "Standup", preview: "   ", messageCount: 1)

        XCTAssertEqual(row.displayPreview, "")
    }
}
