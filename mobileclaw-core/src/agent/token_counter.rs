//! Token estimation for agent loop messages.
//!
//! Uses the 4-bytes-per-token rule from claude-code's `tokenEstimation.ts`:
//! content bytes / 4, rounded up, with a small per-message overhead for role tags.
//!
//! No API calls, no allocations — O(N) scan over message content.

use crate::llm::types::{ContentBlock, Message};

/// Estimate tokens for a single message using 4-bytes-per-token rule.
///
/// Overhead: +3 tokens for role tag, +1 token per content block.
/// Content is measured as raw UTF-8 bytes, divided by 4, rounded up.
///
/// This is consistent with claude-code's `roughTokenCountEstimation()` default
/// (4 bytes/token) — JSON files get 2 bytes/token but our messages are plain
/// text, so 4:1 is the right ratio.
pub fn estimate_message_tokens(msg: &Message) -> usize {
    let text_bytes: usize = msg.content.iter().map(|b| match b {
        ContentBlock::Text { text } => text.len(),
    }).sum();

    let overhead = 3 /* role tag */ + msg.content.len() /* per-block overhead */;

    if text_bytes == 0 {
        return overhead;
    }

    // ceil(text_bytes / 4) = (text_bytes + 3) / 4
    overhead + text_bytes.div_ceil(4)
}

/// Sum of `estimate_message_tokens` over all messages.
/// O(N) scan, zero allocations.
pub fn estimate_tokens(messages: &[Message]) -> usize {
    messages.iter().map(estimate_message_tokens).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // estimate_message_tokens — unit tests
    // -----------------------------------------------------------------------

    #[test]
    fn empty_message_returns_overhead() {
        let msg = Message::user("");
        let tokens = estimate_message_tokens(&msg);
        // 3 (role) + 1 (1 block) = 4
        assert_eq!(tokens, 4, "empty text message: 3 role overhead + 1 block");
    }

    #[test]
    fn ascii_text_token_count() {
        // "hello world" = 11 bytes → ceil(11/4) = 3 tokens + 4 overhead = 7
        let msg = Message::user("hello world");
        let tokens = estimate_message_tokens(&msg);
        assert_eq!(tokens, 7);
    }

    #[test]
    fn exactly_four_bytes() {
        // 4 bytes → ceil(4/4) = 1 token + 4 overhead = 5
        let msg = Message::user("abcd");
        assert_eq!(estimate_message_tokens(&msg), 5);
    }

    #[test]
    fn exactly_five_bytes() {
        // 5 bytes → ceil(5/4) = 2 tokens + 4 overhead = 6
        let msg = Message::user("abcde");
        assert_eq!(estimate_message_tokens(&msg), 6);
    }

    #[test]
    fn assistant_message_same_formula() {
        let msg = Message::assistant("short");
        // 5 bytes → ceil(5/4) = 2 + 4 overhead = 6
        assert_eq!(estimate_message_tokens(&msg), 6);
    }

    #[test]
    fn system_message_same_formula() {
        let msg = Message::system("system");
        // 6 bytes → ceil(6/4) = 2 + 4 overhead = 6
        assert_eq!(estimate_message_tokens(&msg), 6);
    }

    #[test]
    fn tool_result_xml_overhead() {
        // Assistant messages containing tool result XML should count all text
        let xml = "<tool_result name=\"x\" status=\"ok\">value</tool_result>";
        let msg = Message::assistant(xml);
        let tokens = estimate_message_tokens(&msg);
        // xml.len() = 55 bytes → ceil(55/4) = 14 + 4 overhead = 18
        assert!(tokens >= 18, "XML overhead must be counted, got {tokens}");
    }

    #[test]
    fn long_text_scales_linearly() {
        let text: String = "a".repeat(4000);
        let msg = Message::user(&text);
        let tokens = estimate_message_tokens(&msg);
        // 4000 bytes → ceil(4000/4) = 1000 + 4 overhead = 1004
        assert_eq!(tokens, 1004);
    }

    #[test]
    fn multi_block_message() {
        let mut msg = Message::user("first");
        msg.content.push(ContentBlock::Text { text: "second".into() });
        // 5+6 = 11 bytes → ceil(11/4) = 3 + overhead (3 + 2 blocks) = 8
        assert_eq!(estimate_message_tokens(&msg), 8);
    }

    // -----------------------------------------------------------------------
    // estimate_tokens — collection tests
    // -----------------------------------------------------------------------

    #[test]
    fn estimate_tokens_empty_slice() {
        assert_eq!(estimate_tokens(&[]), 0);
    }

    #[test]
    fn estimate_tokens_single_message() {
        let msgs = vec![Message::user("hello")];
        assert_eq!(estimate_tokens(&msgs), estimate_message_tokens(&msgs[0]));
    }

    #[test]
    fn estimate_tokens_multiple_messages() {
        let msgs = vec![
            Message::user("hi"),       // 2 bytes → 1 + 4 = 5
            Message::assistant("ok"),  // 2 bytes → 1 + 4 = 5
        ];
        assert_eq!(estimate_tokens(&msgs), 10);
    }

    #[test]
    fn estimate_tokens_system_prompt() {
        // Typical system prompt: repeats of 29-char string, total 290 bytes
        let system = Message::system(&"You are a helpful assistant. ".repeat(10));
        let tokens = estimate_message_tokens(&system);
        // 290 bytes → ceil(290/4) = 73 + 4 overhead = 77
        assert_eq!(tokens, 77);
    }
}
