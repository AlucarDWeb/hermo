import XCTest
@testable import HermoLogic

final class ChatRowParseTests: XCTestCase {

    func testEveryCoreRowKindDecodes() {
        XCTAssertEqual(
            ChatRow.user(id: 0, text: "hello"),
            parseChatRow(0, #"{"kind":"user","text":"hello"}"#)
        )

        let a = parseChatRow(1, #"{"kind":"assistant","text":"hi","streaming":true}"#)
        if case .assistant(_, _, let streaming, _, _) = a {
            XCTAssertTrue(streaming)
        } else {
            XCTFail("expected .assistant")
        }

        if case .thinking = parseChatRow(2, #"{"kind":"thinking","text":"hm"}"#) {
        } else {
            XCTFail("expected .thinking")
        }

        let t = parseChatRow(3, #"{"kind":"tool","name":"ls","complete":true,"duration_s":0.5}"#)
        if case .tool(_, _, let complete, _, _, _, _, let durationS, _) = t {
            XCTAssertTrue(complete)
            XCTAssertEqual(durationS, 0.5)
        } else {
            XCTFail("expected .tool")
        }

        if case .status = parseChatRow(4, #"{"kind":"status","status":"Done","text":"x"}"#) {
        } else {
            XCTFail("expected .status")
        }

        let e = parseChatRow(5, #"{"kind":"error","message":"boom"}"#)
        if case .error(_, let message) = e {
            XCTAssertEqual(message, "boom")
        } else {
            XCTFail("expected .error")
        }
    }

    func testGarbageDegradesNeverThrows() {
        guard case .status = parseChatRow(0, "") else {
            return XCTFail("expected .status")
        }
        guard case .status = parseChatRow(0, "not json") else {
            return XCTFail("expected .status")
        }
        guard case .status = parseChatRow(0, #"{"kind":"mystery"}"#) else {
            return XCTFail("expected .status")
        }
        guard case .status(_, let arrayKind, _) = parseChatRow(0, "[1,2]") else {
            return XCTFail("expected .status")
        }
        XCTAssertEqual(arrayKind, "unknown")
    }

    func testRowParsingCarriesTheT8PayloadFieldsThrough() {
        let tool = parseChatRow(
            0,
            #"{"kind":"tool","tool_id":"t1","name":"terminal","complete":true,"args":"{\"command\":\"ls\"}","result":"{\"exit_code\":0,\"output\":\"x\"}","duration_s":12.0}"#
        )
        guard case .tool(_, _, let complete, _, let argsJson, let resultJson, let inlineDiff, let durationS, let exitCode) = tool else {
            return XCTFail("expected .tool")
        }
        XCTAssertTrue(complete)
        XCTAssertTrue(argsJson.contains("command"))
        XCTAssertTrue(resultJson.contains("exit_code"))
        XCTAssertEqual(inlineDiff, "")
        XCTAssertEqual(exitCode, 0, "exit code 0 is a real value, not absence")
        XCTAssertEqual(durationS, 12.0, accuracy: 0.001)

        let diff = parseChatRow(
            1,
            #"{"kind":"tool","name":"patch","complete":true,"result":"{\"inline_diff\":\"--- a\\n+++ b\\n+hi\"}","duration_s":1.0}"#
        )
        guard case .tool(_, _, _, _, _, _, let diffText, _, let diffExitCode) = diff else {
            return XCTFail("expected .tool")
        }
        XCTAssertTrue(diffText.hasPrefix("---"), "diff found inside the result string")
        XCTAssertNil(diffExitCode, "an absent exit code stays null")

        let assistant = parseChatRow(
            2,
            #"{"kind":"assistant","text":"ok","streaming":false,"usage":{"total":800,"context_used":700,"context_max":1000}}"#
        )
        guard case .assistant(_, _, _, _, let usageJson) = assistant else {
            return XCTFail("expected .assistant")
        }
        XCTAssertTrue(usageJson.contains("context_max"))
    }

    func testGarbageToolRowsDegradeInsteadOfThrowing() {
        let row = parseChatRow(0, #"{"kind":"tool","name":"x","args":12,"result":[1,2]}"#)
        guard case .tool(_, _, let complete, _, let argsJson, let resultJson, _, _, _) = row else {
            return XCTFail("expected .tool")
        }
        XCTAssertEqual(argsJson, "12")
        XCTAssertFalse(resultJson.isEmpty)
        XCTAssertFalse(complete)
    }
}
