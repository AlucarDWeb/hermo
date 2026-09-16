import XCTest
import HermoLogic

final class LooseJSONTests: XCTestCase {

    func testNotJsonInputReturnsNil() {
        XCTAssertNil(LooseJSON(""))
        XCTAssertNil(LooseJSON("not json"))
        XCTAssertNil(LooseJSON("{oops"))
    }

    func testMissingKeyYieldsTheDefault() {
        let json = LooseJSON(#"{"name":"tool"}"#)!
        XCTAssertEqual("fallback", json.optString("missing", default: "fallback"))
        XCTAssertEqual(7, json.optInt("missing", default: 7))
        XCTAssertEqual(7, json.optLong("missing", default: 7))
        XCTAssertEqual(1.5, json.optDouble("missing", default: 1.5))
        XCTAssertTrue(json.optBool("missing", default: true))
        XCTAssertNil(json.optObject("missing"))
        XCTAssertTrue(json.optArray("missing").isEmpty)
    }

    func testNullValueYieldsTheDefaultJustLikeAMissingKey() {
        let json = LooseJSON(#"{"warning":null,"choices":null,"nested":null}"#)!
        // A JSON null takes the default. org.json would hand back the literal
        // "null" here, which no caller wants on screen and no Kotlin test pins.
        XCTAssertEqual("", json.optString("warning"))
        XCTAssertNil(json.optObject("nested"))
        XCTAssertTrue(json.optArray("choices").isEmpty)
    }

    func testWrongTypeYieldsTheDefaultNeverThrows() {
        let json = LooseJSON(#"{"greeting":{"nested":true},"exit_code":[1,2],"resolved":42,"payload":"just a string"}"#)!
        XCTAssertEqual(#"{"nested":true}"#, json.optString("greeting"))
        XCTAssertEqual(0, json.optInt("exit_code"))
        XCTAssertFalse(json.optBool("resolved"))
        XCTAssertNil(json.optObject("payload"))
        XCTAssertTrue(json.optArray("payload").isEmpty)
    }

    func testNumberReadAsStringCoercesLikeOrgJson() {
        let json = LooseJSON(#"{"duration_s":12,"streaming":true}"#)!
        XCTAssertEqual("12", json.optString("duration_s"))
        XCTAssertEqual("true", json.optString("streaming"))
    }

    func testStringReadAsNumberCoercesLikeOrgJson() {
        let json = LooseJSON(#"{"context_max":"200000","duration_s":"1.5"}"#)!
        XCTAssertEqual(200_000, json.optLong("context_max"))
        XCTAssertEqual(1.5, json.optDouble("duration_s"))
        XCTAssertEqual(1, json.optInt("duration_s"))
    }

    func testStringReadAsBoolCoercesCaseInsensitively() {
        let json = LooseJSON(#"{"a":"TRUE","b":"False","c":"maybe"}"#)!
        XCTAssertTrue(json.optBool("a"))
        XCTAssertFalse(json.optBool("b"))
        XCTAssertFalse(json.optBool("c", default: false))
        XCTAssertTrue(json.optBool("c", default: true))
    }

    func testOptObjectReturnsTheNestedNodeOnly() {
        let json = LooseJSON(#"{"result":{"exit_code":0},"args":"not an object"}"#)!
        let result = json.optObject("result")
        XCTAssertNotNil(result)
        XCTAssertEqual(0, result?.optInt("exit_code", default: -1))
        XCTAssertNil(json.optObject("args"))
    }

    func testOptArrayIsEmptyNotNilWhenAbsentOrWrongType() {
        let missing = LooseJSON(#"{}"#)!
        XCTAssertTrue(missing.optArray("choices").isEmpty)
        let wrongType = LooseJSON(#"{"choices":"once"}"#)!
        XCTAssertTrue(wrongType.optArray("choices").isEmpty)
    }

    func testOptArrayElementsReadBackAsStringsLikeAJsonArrayOfStrings() {
        let json = LooseJSON(#"{"choices":["once","deny",3]}"#)!
        let choices = json.optArray("choices").map { $0.stringValue() }
        XCTAssertEqual(["once", "deny", "3"], choices)
    }

    func testPrettyMatchesJsonObjectToString2ForASimpleObject() {
        let json = LooseJSON(#"{"a":1}"#)!
        XCTAssertEqual("{\n  \"a\": 1\n}", json.pretty())
    }

    func testPrettyIndentsNestedObjectsTwoSpacesPerLevel() {
        let json = LooseJSON(#"{"a":{"b":2}}"#)!
        XCTAssertEqual("{\n  \"a\": {\n    \"b\": 2\n  }\n}", json.pretty())
    }

    func testPrettyCollapsesEmptyObjectsAndArrays() {
        XCTAssertEqual("{}", LooseJSON(#"{}"#)!.pretty())
        let json = LooseJSON(#"{"items":[]}"#)!
        XCTAssertEqual("{\n  \"items\": []\n}", json.pretty())
    }

    func testPrettyRoundTripsBackToTheSameValues() {
        let original = LooseJSON(#"{"name":"tool","complete":true,"duration_s":1.5,"result":{"exit_code":0}}"#)!
        let reparsed = LooseJSON(original.pretty())
        XCTAssertNotNil(reparsed)
        XCTAssertEqual("tool", reparsed?.optString("name"))
        XCTAssertTrue(reparsed?.optBool("complete") ?? false)
        XCTAssertEqual(1.5, reparsed?.optDouble("duration_s"))
        XCTAssertEqual(0, reparsed?.optObject("result")?.optInt("exit_code", default: -1))
    }

    func testWrapAnAlreadyDecodedValue() {
        let json = LooseJSON(["name": "tool", "count": 3] as [String: Any])
        XCTAssertEqual("tool", json.optString("name"))
        XCTAssertEqual(3, json.optInt("count"))
    }
}
