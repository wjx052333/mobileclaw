use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role { User, Assistant, System }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self { role: Role::User, content: vec![ContentBlock::Text { text: text.into() }] }
    }
    pub fn assistant(text: impl Into<String>) -> Self {
        Self { role: Role::Assistant, content: vec![ContentBlock::Text { text: text.into() }] }
    }
    pub fn system(text: impl Into<String>) -> Self {
        Self { role: Role::System, content: vec![ContentBlock::Text { text: text.into() }] }
    }
    /// 返回文本内容（多 block 拼接）
    pub fn text_content(&self) -> String {
        self.content.iter().map(|b| match b {
            ContentBlock::Text { text } => text.as_str(),
        }).collect::<Vec<_>>().join("")
    }
}

/// Agent 循环中消费的流式事件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamEvent {
    TextDelta { text: String },
    MessageStart,
    MessageStop,
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_serializes_correctly() {
        let msg = Message {
            role: Role::User,
            content: vec![ContentBlock::Text { text: "hello".into() }],
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["role"], "user");
        assert_eq!(json["content"][0]["type"], "text");
        assert_eq!(json["content"][0]["text"], "hello");
    }

    #[test]
    fn stream_event_text_delta() {
        let event = StreamEvent::TextDelta { text: "hi".into() };
        assert!(matches!(event, StreamEvent::TextDelta { .. }));
    }
}
