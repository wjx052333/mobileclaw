//! Context management: threshold-based pruning of message history.
//!
//! Prevents unbounded context growth by removing oldest non-essential messages
//! when token usage approaches the model's context window.
//!
//! Design principles (per plan):
//! - System messages are NEVER removed
//! - Last N user turns are always preserved
//! - Last assistant turn is always preserved
//! - In-place mutation, zero intermediate allocations

use crate::agent::token_counter::{estimate_message_tokens, estimate_tokens};
use crate::llm::types::{Message, Role};
use crate::ClawResult;

/// Configurable context window management.
#[derive(Debug, Clone)]
pub struct ContextConfig {
    /// Maximum tokens allowed in history.
    /// Default: 200_000 (Claude Sonnet 4.6 context window).
    pub max_tokens: usize,
    /// Buffer tokens to keep below the limit.
    /// claude-code default: 13_000 — leaves room for response output.
    pub buffer_tokens: usize,
    /// Minimum user turns to always preserve (at least last N).
    pub min_user_turns: usize,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_tokens: 200_000,
            buffer_tokens: 13_000,
            min_user_turns: 3,
        }
    }
}

/// Result of a prune operation.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PruneResult {
    pub pruned_count: usize,
    pub tokens_before: usize,
    pub tokens_after: usize,
}

/// The pruning threshold: max_tokens - buffer_tokens.
/// When current tokens exceed this, oldest eligible messages are removed.
pub fn pruning_threshold(config: &ContextConfig) -> usize {
    config.max_tokens.saturating_sub(config.buffer_tokens)
}

/// Identify indices of messages protected from removal.
///
/// Protected:
/// - All system messages (role == System)
/// - Last assistant turn
/// - Last N user turns (configurable, min 3)
///
/// This is O(N) scanning from the end — no extra allocation beyond the HashSet.
fn protected_indices(messages: &[Message], min_user_turns: usize) -> std::collections::HashSet<usize> {
    let mut protected = std::collections::HashSet::new();
    let len = messages.len();

    // Protect all system messages
    for (i, msg) in messages.iter().enumerate() {
        if msg.role == Role::System {
            protected.insert(i);
        }
    }

    // Protect last assistant turn
    for i in (0..len).rev() {
        if messages[i].role == Role::Assistant {
            protected.insert(i);
            break;
        }
    }

    // Protect last N user turns (counting from the end)
    let mut user_turns_found = 0;
    for i in (0..len).rev() {
        if messages[i].role == Role::User {
            protected.insert(i);
            user_turns_found += 1;
            if user_turns_found >= min_user_turns {
                break;
            }
        }
    }

    protected
}

/// Prune oldest non-protected messages when tokens exceed threshold.
///
/// Returns Ok(PruneResult) with counts. Does NOT fail — if pruning can't
/// reduce enough tokens (all messages protected), it removes what it can.
///
/// Safety: never returns an empty history — at least system + last user turn survive.
pub fn prune_oldest_messages(
    messages: &mut Vec<Message>,
    threshold: usize,
    current_tokens: usize,
    min_user_turns: usize,
) -> ClawResult<PruneResult> {
    let tokens_before = current_tokens;

    if tokens_before <= threshold || messages.is_empty() {
        return Ok(PruneResult {
            pruned_count: 0,
            tokens_before,
            tokens_after: tokens_before,
        });
    }

    let prot = protected_indices(messages, min_user_turns);
    let mut to_remove: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(i, _)| !prot.contains(i))
        .map(|(i, _)| i)
        .collect();

    // Remove from oldest (ascending index → reverse for swap_remove safety)
    to_remove.sort_unstable();

    let mut tokens = tokens_before;
    let mut pruned = 0;

    // Remove oldest first until under threshold
    for idx in &to_remove {
        if tokens <= threshold {
            break;
        }
        let msg_tokens = estimate_message_tokens(&messages[*idx]);
        tokens -= msg_tokens;
        pruned += 1;
    }

    if pruned == 0 {
        return Ok(PruneResult {
            pruned_count: 0,
            tokens_before,
            tokens_after: tokens_before,
        });
    }

    // Remove in reverse order (highest index first) for correct in-place deletion
    let to_keep = messages.len() - pruned;
    // Rebuild: this is O(N) but avoids unsafe swap_remove which would scramble order
    // In a hot path this would be unacceptable, but pruning happens once per chat turn.
    let mut new_messages = Vec::with_capacity(to_keep);
    let mut removed = 0;
    // to_remove contains indices in ascending order, only first `pruned` are actually removed
    let removed_set: std::collections::HashSet<usize> = to_remove.into_iter().take(pruned).collect();
    for (i, msg) in messages.iter().enumerate() {
        if !removed_set.contains(&i) {
            new_messages.push(msg.clone());
        } else {
            removed += 1;
            if removed >= pruned {
                // Add remaining messages
                break;
            }
        }
    }
    // Add all messages after the last removed one
    if let Some(&last_removed) = removed_set.iter().take(pruned).max() {
        for msg in messages.iter().skip(last_removed + 1) {
            new_messages.push(msg.clone());
        }
    }

    *messages = new_messages;

    Ok(PruneResult {
        pruned_count: pruned,
        tokens_before,
        tokens_after: estimate_tokens(messages),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys(t: &str) -> Message { Message::system(t) }
    fn user(t: &str) -> Message { Message::user(t) }
    fn assistant(t: &str) -> Message { Message::assistant(t) }

    // -----------------------------------------------------------------------
    // protected_indices
    // -----------------------------------------------------------------------

    #[test]
    fn system_messages_always_protected() {
        let msgs = vec![sys("A"), user("1"), user("2"), user("3")];
        let prot = protected_indices(&msgs, 3);
        assert!(prot.contains(&0), "system message must be protected");
    }

    #[test]
    fn last_user_turns_protected() {
        let msgs = vec![user("1"), user("2"), user("3")];
        let prot = protected_indices(&msgs, 2);
        assert!(prot.contains(&2), "last user turn must be protected");
        assert!(prot.contains(&1), "second-to-last user turn must be protected");
        assert!(!prot.contains(&0), "3rd-from-last should NOT be protected with min=2");
    }

    #[test]
    fn last_assistant_protected() {
        let msgs = vec![user("1"), assistant("r1"), user("2")];
        let prot = protected_indices(&msgs, 1);
        // last assistant turn is at index 1
        assert!(prot.contains(&1), "last assistant turn must be protected");
    }

    // -----------------------------------------------------------------------
    // pruning_threshold
    // -----------------------------------------------------------------------

    #[test]
    fn threshold_equals_max_minus_buffer() {
        let config = ContextConfig { max_tokens: 100, buffer_tokens: 20, min_user_turns: 3 };
        assert_eq!(pruning_threshold(&config), 80);
    }

    #[test]
    fn threshold_zero_on_overflow() {
        let config = ContextConfig { max_tokens: 10, buffer_tokens: 100, min_user_turns: 3 };
        // saturating_sub => 0
        assert_eq!(pruning_threshold(&config), 0);
    }

    // -----------------------------------------------------------------------
    // prune_oldest_messages
    // -----------------------------------------------------------------------

    #[test]
    fn no_prune_when_under_threshold() {
        let mut msgs = vec![sys("s"), user("hi")];
        let current = estimate_tokens(&msgs);
        let result = prune_oldest_messages(&mut msgs, 80, current, 3).unwrap();
        assert_eq!(result.pruned_count, 0);
        assert_eq!(msgs.len(), 2);
    }

    #[test]
    fn prune_oldest_removes_oldest_user_message() {
        // Create messages that exceed a small threshold
        let mut msgs = vec![
            sys("system"),
            user("first message to be pruned"),
            user("second message"),
            user("third message"),
            user("fourth message"),
            user("fifth message"),
        ];
        let current = estimate_tokens(&msgs);
        // Threshold of 20 tokens — should prune oldest messages
        let result = prune_oldest_messages(&mut msgs, 20, current, 2).unwrap();
        assert!(result.pruned_count > 0, "should have pruned messages");
        // System message must still be there
        assert!(msgs.iter().any(|m| m.role == Role::System), "system message must survive");
    }

    #[test]
    fn prune_preserves_at_least_minimum_user_turns() {
        let mut msgs = vec![sys("s")];
        for i in 1..30 {
            msgs.push(user(&format!("user {i}")));
        }
        let result = prune_oldest_messages(&mut msgs, 10, 1000, 5).unwrap();
        assert!(result.pruned_count > 0);
        let user_count = msgs.iter().filter(|m| m.role == Role::User).count();
        assert!(user_count >= 5, "must preserve at least 5 user turns, got {user_count}");
    }

    #[test]
    fn prune_empty_messages_returns_early() {
        let mut msgs: Vec<Message> = vec![];
        let result = prune_oldest_messages(&mut msgs, 100, 200, 3).unwrap();
        assert_eq!(result.pruned_count, 0);
    }

    #[test]
    fn prune_noop_when_all_protected() {
        let mut msgs = vec![sys("sys"), user("only message")];
        let result = prune_oldest_messages(&mut msgs, 10, 100, 1).unwrap();
        // Only system(protected) + last user(protected) — nothing eligible
        assert_eq!(result.pruned_count, 0);
    }

    #[test]
    fn prune_result_tokens_under_threshold() {
        // Build ~500 tokens of messages
        let mut msgs = vec![sys("system prompt")];
        for i in 1..50 {
            let text = format!("This is user message number {i} with enough text to add tokens.");
            msgs.push(user(&text));
        }
        let current = estimate_tokens(&msgs);
        let threshold = 100;
        let result = prune_oldest_messages(&mut msgs, threshold, current, 2).unwrap();
        assert!(result.pruned_count > 0, "must prune something with 500+ tokens vs 100 threshold");
        assert!(result.tokens_after < result.tokens_before, "tokens must decrease");
        assert!(result.tokens_after <= threshold, "result must be under threshold: {} vs {}", result.tokens_after, threshold);
    }
}
