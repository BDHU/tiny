//! Auto-compaction: when history grows past a char threshold, summarize the
//! older prefix into a single synthetic user message via the same provider.
//!
//! The cutoff sits at a `Message::User` boundary, which guarantees we never
//! split an assistant's tool_calls from their tool_results — those always
//! live entirely between two user messages.

use crate::agent::{Message, Provider};
use anyhow::{anyhow, Result};

pub const DEFAULT_THRESHOLD: usize = 200_000;

const KEEP_RECENT_TURNS: usize = 3;

const SUMMARY_SYSTEM: &str = "You write concise, structured summaries of \
partial conversations so the work can continue from the summary alone.";

const SUMMARY_INSTRUCTION: &str = r#"Summarize the conversation above using exactly this Markdown structure. Keep every section, even when empty (use "(none)").

## Goal
- [single-sentence task summary]

## Progress
### Done
- [completed work or "(none)"]
### In Progress
- [current work or "(none)"]

## Key Decisions
- [decision and why, or "(none)"]

## Next Steps
- [ordered next actions or "(none)"]

## Critical Context
- [errors, open questions, important facts, or "(none)"]

## Relevant Files
- [path: why it matters, or "(none)"]

Rules:
- Use terse bullets, not prose.
- Preserve exact file paths, commands, error strings, and identifiers verbatim.
- Do not mention the summary process or that context was compacted."#;

// Auto path: gated by char threshold; keeps the last KEEP_RECENT_TURNS user
// turns so context stays predictable across many small turns.
pub async fn compact_if_needed(
    history: &mut Vec<Message>,
    provider: &dyn Provider,
    max_chars: usize,
) -> Result<bool> {
    if total_chars(history) <= max_chars {
        return Ok(false);
    }
    let Some(cutoff) = recent_user_boundary(history, KEEP_RECENT_TURNS) else {
        return Ok(false);
    };
    summarize_and_replace(history, cutoff, provider).await
}

// Manual path: unconditional. Summarize the full history into one message.
// Caller decides when to invoke this — we just do the work.
pub async fn compact_now(
    history: &mut Vec<Message>,
    provider: &dyn Provider,
) -> Result<bool> {
    if history.is_empty() {
        return Ok(false);
    }
    let cutoff = history.len();
    summarize_and_replace(history, cutoff, provider).await
}

async fn summarize_and_replace(
    history: &mut Vec<Message>,
    cutoff: usize,
    provider: &dyn Provider,
) -> Result<bool> {
    // Send the conversation first, then the summarize instruction as the final
    // user turn — the canonical "context, then ask" shape, more reliable than
    // putting the instruction in the system prompt.
    let mut messages: Vec<Message> = history[..cutoff].to_vec();
    messages.push(Message::User(SUMMARY_INSTRUCTION.into()));
    let response = provider
        .complete(SUMMARY_SYSTEM, &messages, &[])
        .await?;
    let Message::Assistant { text, .. } = response else {
        return Err(anyhow!("summarizer returned non-assistant message"));
    };
    let text = text.trim();
    if text.is_empty() {
        return Err(anyhow!("summarizer returned empty text"));
    }
    let summary = Message::User(format!(
        "<previous-conversation-summary>\n{text}\n</previous-conversation-summary>"
    ));
    history.splice(..cutoff, std::iter::once(summary));
    Ok(true)
}

fn total_chars(history: &[Message]) -> usize {
    history.iter().map(message_chars).sum()
}

fn message_chars(msg: &Message) -> usize {
    match msg {
        Message::User(text) => text.len(),
        Message::Assistant { text, tool_calls } => {
            text.len()
                + tool_calls
                    .iter()
                    .map(|c| c.name.len() + c.input.to_string().len())
                    .sum::<usize>()
        }
        Message::Tool(result) => result.content.len(),
    }
}

fn recent_user_boundary(history: &[Message], keep_turns: usize) -> Option<usize> {
    let user_indices: Vec<usize> = history
        .iter()
        .enumerate()
        .filter_map(|(i, m)| matches!(m, Message::User(_)).then_some(i))
        .collect();
    if user_indices.len() <= keep_turns {
        return None;
    }
    Some(user_indices[user_indices.len() - keep_turns])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{ToolCall, ToolResult};
    use crate::tool::ErasedTool;
    use async_trait::async_trait;
    use serde_json::json;

    struct FakeProvider {
        reply: String,
    }

    #[async_trait]
    impl Provider for FakeProvider {
        async fn complete(
            &self,
            _system: &str,
            _messages: &[Message],
            _tools: &[Box<dyn ErasedTool>],
        ) -> Result<Message> {
            Ok(Message::Assistant {
                text: self.reply.clone(),
                tool_calls: Vec::new(),
            })
        }
    }

    fn make_history() -> Vec<Message> {
        vec![
            Message::User("u1".into()),
            Message::Assistant {
                text: "a1".into(),
                tool_calls: vec![ToolCall {
                    id: "t1".into(),
                    name: "echo".into(),
                    input: json!({}),
                }],
            },
            Message::Tool(ToolResult {
                id: "t1".into(),
                content: "out1".into(),
                is_error: false,
            }),
            Message::User("u2".into()),
            Message::Assistant {
                text: "a2".into(),
                tool_calls: vec![],
            },
            Message::User("u3".into()),
            Message::Assistant {
                text: "a3".into(),
                tool_calls: vec![],
            },
            Message::User("u4".into()),
            Message::Assistant {
                text: "a4".into(),
                tool_calls: vec![],
            },
            Message::User("u5".into()),
            Message::Assistant {
                text: "a5".into(),
                tool_calls: vec![],
            },
        ]
    }

    #[tokio::test]
    async fn no_op_below_threshold() {
        let provider = FakeProvider {
            reply: "summary".into(),
        };
        let mut history = make_history();
        let original_len = history.len();
        let did = compact_if_needed(&mut history, &provider, 10_000)
            .await
            .unwrap();
        assert!(!did);
        assert_eq!(history.len(), original_len);
    }

    #[tokio::test]
    async fn compacts_at_user_boundary() {
        let provider = FakeProvider {
            reply: "SUMMARIZED".into(),
        };
        let mut history = make_history();
        let did = compact_if_needed(&mut history, &provider, 1).await.unwrap();
        assert!(did);
        // Keep last 3 user turns: u3, a3, u4, a4, u5, a5 — plus synthetic at index 0.
        assert_eq!(history.len(), 7);
        assert!(matches!(&history[0], Message::User(s) if s.contains("SUMMARIZED")));
        assert!(matches!(&history[0], Message::User(s) if s.contains("<previous-conversation-summary>")));
        assert!(matches!(&history[1], Message::User(s) if s == "u3"));
        assert!(matches!(&history[6], Message::Assistant { text, .. } if text == "a5"));
    }

    #[tokio::test]
    async fn cutoff_does_not_split_tool_pair() {
        // Tool pair lives between u1 and u2; cutoff should land at u2 so the
        // pair stays entirely inside the summarized prefix, not orphaned.
        let mut history = vec![
            Message::User("u1".into()),
            Message::Assistant {
                text: "".into(),
                tool_calls: vec![ToolCall {
                    id: "t1".into(),
                    name: "x".into(),
                    input: json!({}),
                }],
            },
            Message::Tool(ToolResult {
                id: "t1".into(),
                content: "r1".into(),
                is_error: false,
            }),
            Message::User("u2".into()),
            Message::Assistant {
                text: "a2".into(),
                tool_calls: vec![],
            },
            Message::User("u3".into()),
            Message::Assistant {
                text: "a3".into(),
                tool_calls: vec![],
            },
            Message::User("u4".into()),
            Message::Assistant {
                text: "a4".into(),
                tool_calls: vec![],
            },
        ];
        let provider = FakeProvider { reply: "S".into() };
        let did = compact_if_needed(&mut history, &provider, 1).await.unwrap();
        assert!(did);
        // After: [summary, u2, a2, u3, a3, u4, a4]
        assert_eq!(history.len(), 7);
        // First message after summary must be a User, never an orphan Tool.
        assert!(matches!(&history[1], Message::User(s) if s == "u2"));
    }

    #[tokio::test]
    async fn no_op_with_one_user_turn() {
        let provider = FakeProvider { reply: "S".into() };
        let mut history = vec![
            Message::User("u1".into()),
            Message::Assistant {
                text: "a1".into(),
                tool_calls: vec![],
            },
        ];
        let did = compact_if_needed(&mut history, &provider, 1).await.unwrap();
        assert!(!did);
        assert_eq!(history.len(), 2);
    }

    #[tokio::test]
    async fn compact_now_replaces_entire_history() {
        let provider = FakeProvider {
            reply: "ALL".into(),
        };
        let mut history = vec![
            Message::User("u1".into()),
            Message::Assistant {
                text: "a1".into(),
                tool_calls: vec![],
            },
            Message::User("u2".into()),
            Message::Assistant {
                text: "a2".into(),
                tool_calls: vec![],
            },
        ];
        let did = compact_now(&mut history, &provider).await.unwrap();
        assert!(did);
        assert_eq!(history.len(), 1);
        assert!(matches!(&history[0], Message::User(s) if s.contains("ALL")));
    }

    #[tokio::test]
    async fn compact_now_no_op_on_empty() {
        let provider = FakeProvider { reply: "X".into() };
        let mut history: Vec<Message> = Vec::new();
        let did = compact_now(&mut history, &provider).await.unwrap();
        assert!(!did);
        assert!(history.is_empty());
    }

    #[tokio::test]
    async fn compact_now_skips_threshold_gate() {
        // Tiny history that would not trigger compact_if_needed even at low
        // thresholds — but compact_now still runs as long as there are enough
        // user turns.
        let provider = FakeProvider {
            reply: "FORCED".into(),
        };
        let mut history = make_history();
        let did = compact_now(&mut history, &provider).await.unwrap();
        assert!(did);
        assert!(matches!(&history[0], Message::User(s) if s.contains("FORCED")));
    }
}
