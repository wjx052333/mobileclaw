use serde::{Deserialize, Serialize};

/// Describes a tool available to the LLM (request-side tools array).
#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: serde_json::Value },
    ToolResult { tool_use_id: String, content: String, is_error: bool },
    Image { mime_type: String, data: Vec<u8> },
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
            ContentBlock::ToolUse { .. } => "",
            ContentBlock::ToolResult { .. } => "",
            ContentBlock::Image { .. } => "",
        }).collect::<Vec<_>>().join("")
    }
}

/// Agent 循环中消费的流式事件
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StreamEvent {
    TextDelta { text: String },
    /// Emitted once per complete tool_use block.
    ToolUse { id: String, name: String, input: serde_json::Value },
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

    #[test]
    fn message_user_constructor() {
        let m = Message::user("hello");
        assert_eq!(m.role, Role::User);
        assert_eq!(m.text_content(), "hello");
    }

    #[test]
    fn message_assistant_constructor() {
        let m = Message::assistant("reply");
        assert_eq!(m.role, Role::Assistant);
        assert_eq!(m.text_content(), "reply");
    }

    #[test]
    fn message_system_constructor() {
        let m = Message::system("system prompt");
        assert_eq!(m.role, Role::System);
        assert_eq!(m.text_content(), "system prompt");
    }

    #[test]
    fn text_content_empty_for_no_content() {
        let m = Message { role: Role::User, content: vec![] };
        assert_eq!(m.text_content(), "");
    }

    #[test]
    fn tool_use_block_serializes_with_type_tag() {
        let block = ContentBlock::ToolUse {
            id: "tu_1".into(),
            name: "time".into(),
            input: serde_json::json!({}),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "tool_use");
        assert_eq!(json["id"], "tu_1");
        assert_eq!(json["name"], "time");
        assert_eq!(json["input"], serde_json::json!({}));
    }

    #[test]
    fn tool_result_block_serializes_with_type_tag() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "tu_1".into(),
            content: "ok".into(),
            is_error: false,
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "tool_result");
        assert_eq!(json["tool_use_id"], "tu_1");
        assert_eq!(json["content"], "ok");
        assert_eq!(json["is_error"], false);
    }

    #[test]
    fn role_tool_serializes_as_tool() {
        let json = serde_json::to_value(Role::Tool).unwrap();
        assert_eq!(json, "tool");
    }

    #[test]
    fn image_block_serializes_with_type_tag() {
        let block = ContentBlock::Image {
            mime_type: "image/jpeg".into(),
            data: vec![0xFF, 0xD8, 0xFF],
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "image");
        assert_eq!(json["mime_type"], "image/jpeg");
        assert_eq!(json["data"], serde_json::json!([255, 216, 255]));
    }

    #[test]
    fn text_content_skips_image_blocks() {
        let mut msg = Message::user("hello");
        msg.content.push(ContentBlock::Image {
            mime_type: "image/jpeg".into(),
            data: vec![1, 2, 3],
        });
        assert_eq!(msg.text_content(), "hello");
    }
}
