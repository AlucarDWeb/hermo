import XCTest
@testable import HermoLogic

final class MarkdownBlocksTests: XCTestCase {

    func testFenceWithLanguageAndSurroundingProse() {
        let blocks = MarkdownBlocks.split("before\n```rust\nfn main() {}\n```\nafter")
        XCTAssertEqual(blocks.count, 3)
        XCTAssertEqual(blocks[0].text, "before")
        XCTAssertEqual(blocks[1].language, "rust")
        XCTAssertEqual(blocks[1].text, "fn main() {}")
        XCTAssertTrue(blocks[1].isFence)
        XCTAssertEqual(blocks[2].text, "after")
    }

    func testUnterminatedFinalFenceStaysOpen() {
        let blocks = MarkdownBlocks.split("prose\n```python\nprint(1)")
        XCTAssertEqual(blocks.count, 2)
        XCTAssertTrue(blocks[1].open)
        XCTAssertEqual(blocks[1].language, "python")
        XCTAssertEqual(blocks[1].text, "print(1)")
    }

    func testInnerShorterBacktickRunDoesNotCloseTheFence() {
        let blocks = MarkdownBlocks.split("```\ncode with ``` inside\nmore\n```")
        XCTAssertEqual(blocks.count, 1)
        XCTAssertTrue(blocks[0].text.contains("``` inside"))
    }

    func testTildeFencesWork() {
        let blocks = MarkdownBlocks.split("~~~\nx\n~~~")
        XCTAssertEqual(blocks.count, 1)
        XCTAssertTrue(blocks[0].isFence)
        XCTAssertEqual(blocks[0].text, "x")
    }

    func testEmptyAndPlainText() {
        XCTAssertEqual(MarkdownBlocks.split("").count, 0)
        let one = MarkdownBlocks.split("just text")
        XCTAssertEqual(one.count, 1)
        XCTAssertFalse(one[0].isFence)
    }
}
