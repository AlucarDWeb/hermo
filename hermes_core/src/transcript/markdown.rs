//! Markdown block splitting (entity layer, PLAN §4 T4).
//!
//! `split_blocks` splits streamed assistant text into renderable blocks:
//! fenced code blocks carry their language, everything else is plain text.
//! While streaming a fence may be unterminated — the last block then stays
//! `open` (never dropped), so the UI keeps appending as deltas arrive.
//! Nested backticks (a fence with more backticks than an inner run) are
//! handled: the block only closes at a fence line of at least the opening
//! length.

use crate::transcript::model::MarkdownBlock;

/// CommonMark fences allow at least three backticks (or tildes) to open.
const MIN_FENCE: usize = 3;

/// Split `text` into blocks: fenced code (language + text) and plain text.
///
/// Rules (pinned by tests):
/// - A fence line is a line of only backticks (length >= [`MIN_FENCE`]) or
///   only tildes (length >= [`MIN_FENCE`]), optionally with an info string.
/// - An unterminated final fence yields one last block with `open: true` —
///   the normal streaming state; dropping it would lose the code being
///   streamed.
/// - Inner backtick runs shorter than the opening fence do not close it
///   (nested backticks).
/// - Empty input yields no blocks.
pub fn split_blocks(text: &str) -> Vec<MarkdownBlock> {
    let mut blocks: Vec<MarkdownBlock> = Vec::new();
    let mut current: Option<MarkdownBlock> = None;
    // Opening fence: (backtick_length, tilde_length) — exactly one nonzero.
    let mut fence_ticks = 0usize;
    let mut fence_tildes = 0usize;

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some((kind_len, is_tick)) = fence_marker(trimmed) {
            if fence_ticks == 0 && fence_tildes == 0 {
                // Opening a fence.
                if is_tick {
                    fence_ticks = kind_len;
                } else {
                    fence_tildes = kind_len;
                }
                let language = if is_tick {
                    trimmed[kind_len..].trim().to_string()
                } else {
                    String::new()
                };
                if let Some(block) = current.take() {
                    blocks.push(block);
                }
                current = Some(MarkdownBlock { language, text: String::new(), open: true });
                continue;
            }
            // Inside a fence: a marker of the SAME kind and at least the
            // opening length closes it; shorter runs stay in the text.
            let closes = (is_tick && fence_ticks > 0 && kind_len >= fence_ticks)
                || (!is_tick && fence_tildes > 0 && kind_len >= fence_tildes);
            if closes {
                if let Some(mut block) = current.take() {
                    block.open = false;
                    blocks.push(block);
                }
                fence_ticks = 0;
                fence_tildes = 0;
                continue;
            }
        }
        match &mut current {
            Some(block) => {
                if !block.text.is_empty() {
                    block.text.push('\n');
                }
                block.text.push_str(line);
            }
            None => {
                current = Some(MarkdownBlock {
                    language: String::new(),
                    text: line.to_string(),
                    open: true,
                });
            }
        }
    }
    if let Some(block) = current.take() {
        blocks.push(block);
    }
    blocks
}

/// `Some((marker_length, is_backtick))` when `line` is a pure fence marker
/// line: only backticks (length >= [`MIN_FENCE`]), backticks plus an info
/// string (no further backticks in it), or only tildes
/// (length >= [`MIN_FENCE`], CommonMark: tilde fences carry no info string).
fn fence_marker(line: &str) -> Option<(usize, bool)> {
    let ticks = line.chars().take_while(|&c| c == '`').count();
    if ticks >= MIN_FENCE {
        let info = &line[ticks..];
        if !info.contains('`') {
            return Some((ticks, true));
        }
        return None;
    }
    let tildes = line.chars().take_while(|&c| c == '~').count();
    if tildes >= MIN_FENCE && tildes == line.chars().count() {
        return Some((tildes, false));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rule pinned: a closed fenced block carries its language and text;
    /// surrounding plain text becomes its own blocks.
    #[test]
    fn fenced_code_with_language_and_plain_text() {
        let blocks = split_blocks("before\n```rust\nfn main() {}\n```\nafter");
        assert_eq!(
            blocks,
            vec![
                MarkdownBlock { language: String::new(), text: "before".into(), open: true },
                MarkdownBlock { language: "rust".into(), text: "fn main() {}".into(), open: false },
                MarkdownBlock { language: String::new(), text: "after".into(), open: true },
            ]
        );
    }

    /// Rule pinned: an unterminated fence during streaming stays the last
    /// OPEN block — the pre-fix "drop it" behaviour fails this test.
    #[test]
    fn unterminated_fence_stays_open_not_dropped() {
        let streaming = "text first\n```json\n{\"key\": \"val";
        let blocks = split_blocks(streaming);
        assert_eq!(blocks.len(), 2, "the open code block must survive");
        assert_eq!(blocks[0].text, "text first");
        assert!(blocks[0].open);
        assert_eq!(blocks[1].language, "json");
        assert_eq!(blocks[1].text, "{\"key\": \"val");
        assert!(blocks[1].open, "the fence is not closed yet");
    }

    /// Rule pinned: the fence CLOSES once the closing marker arrives — the
    /// same text fully streamed must end with `open: false`.
    #[test]
    fn terminated_fence_closes() {
        let full = "```json\n{\"k\": 1}\n```";
        let blocks = split_blocks(full);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].language, "json");
        assert_eq!(blocks[0].text, "{\"k\": 1}");
        assert!(!blocks[0].open, "closed fence is not open");
    }

    /// Rule pinned: nested backticks — an inner run shorter than the opening
    /// fence does NOT close the block.
    #[test]
    fn nested_backticks_do_not_close_the_block() {
        let text = "````md\n# title with `inline` code\n````";
        let blocks = split_blocks(text);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].language, "md");
        assert_eq!(blocks[0].text, "# title with `inline` code");
        assert!(!blocks[0].open, "a 4-tick fence closes at 4 ticks");
    }

    /// Rule pinned: adjacent nested-backtick content inside a 3-tick fence.
    #[test]
    fn inline_backticks_inside_fence_stay_in_text() {
        let text = "```\nuse `x` here\n```";
        let blocks = split_blocks(text);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "use `x` here");
    }

    /// Rule pinned: multi-line code keeps its internal newlines.
    #[test]
    fn multiline_code_keeps_newlines() {
        let text = "```py\na = 1\nb = 2\n```";
        let blocks = split_blocks(text);
        assert_eq!(blocks[0].text, "a = 1\nb = 2");
        assert_eq!(blocks[0].language, "py");
    }

    /// Rule pinned: empty and fence-less input yield simple text blocks.
    #[test]
    fn plain_text_and_empty_input() {
        assert!(split_blocks("").is_empty(), "empty input yields no blocks");
        let blocks = split_blocks("just text");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].language, "");
        assert_eq!(blocks[0].text, "just text");
    }
}
