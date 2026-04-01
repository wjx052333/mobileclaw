use futures::StreamExt;
use crate::{
    ClawResult,
    agent::parser::{extract_tool_calls, extract_text_without_tool_calls, format_tool_result},
    llm::{client::LlmClient, types::{Message, StreamEvent}},
    skill::SkillManager,
    tools::{ToolContext, ToolRegistry},
};

const MAX_TOOL_ROUNDS: usize = 10;
const MAX_TOKENS: u32 = 4096;

#[derive(Debug, Clone)]
pub enum AgentEvent {
    TextDelta { text: String },
    ToolCall { name: String },
    ToolResult { name: String, success: bool },
    Done,
}

pub struct AgentLoop<L: LlmClient> {
    llm: L,
    registry: ToolRegistry,
    ctx: ToolContext,
    skill_mgr: SkillManager,
    history: Vec<Message>,
}

impl<L: LlmClient> AgentLoop<L> {
    pub fn new(llm: L, registry: ToolRegistry, ctx: ToolContext, skill_mgr: SkillManager) -> Self {
        Self { llm, registry, ctx, skill_mgr, history: Vec::new() }
    }

    pub fn history(&self) -> &[Message] { &self.history }

    pub async fn chat(&mut self, user_input: &str, base_system: &str) -> ClawResult<Vec<AgentEvent>> {
        let matched = self.skill_mgr.match_skills(user_input);
        let system = self.skill_mgr.build_system_prompt(base_system, &matched);

        self.history.push(Message::user(user_input));
        let mut all_events = Vec::new();

        for _round in 0..MAX_TOOL_ROUNDS {
            let mut stream = self.llm.stream_messages(&system, &self.history, MAX_TOKENS).await?;

            let mut full_text = String::new();
            while let Some(event) = stream.next().await {
                match event? {
                    StreamEvent::TextDelta { text } => {
                        all_events.push(AgentEvent::TextDelta { text: text.clone() });
                        full_text.push_str(&text);
                    }
                    StreamEvent::MessageStop | StreamEvent::MessageStart => {}
                    StreamEvent::Error { message } => {
                        return Err(crate::ClawError::Llm(message));
                    }
                }
            }

            let tool_calls = extract_tool_calls(&full_text);
            if tool_calls.is_empty() {
                self.history.push(Message::assistant(&full_text));
                all_events.push(AgentEvent::Done);
                break;
            }

            let mut tool_results_xml = String::new();
            for call in &tool_calls {
                all_events.push(AgentEvent::ToolCall { name: call.name.clone() });
                let result = match self.registry.get(&call.name) {
                    Some(tool) => tool.execute(call.args.clone(), &self.ctx).await,
                    None => Err(crate::ClawError::Tool {
                        tool: call.name.clone(),
                        message: "tool not found".into(),
                    }),
                };
                match result {
                    Ok(r) => {
                        all_events.push(AgentEvent::ToolResult { name: call.name.clone(), success: r.success });
                        tool_results_xml.push_str(&format_tool_result(&call.name, r.success, &r.output));
                    }
                    Err(e) => {
                        let err_val = serde_json::json!({"error": e.to_string()});
                        all_events.push(AgentEvent::ToolResult { name: call.name.clone(), success: false });
                        tool_results_xml.push_str(&format_tool_result(&call.name, false, &err_val));
                    }
                }
            }

            let clean_text = extract_text_without_tool_calls(&full_text);
            let assistant_msg = format!("{}\n{}", clean_text, tool_results_xml);
            self.history.push(Message::assistant(&assistant_msg));
            self.history.push(Message::user("[tool results provided above, please continue]"));
        }

        Ok(all_events)
    }
}
