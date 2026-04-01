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

// ---------------------------------------------------------------------------
// Tool descriptions injected into every system prompt
// ---------------------------------------------------------------------------

/// Build the `## Available Tools` section from the registered tool list.
/// This is appended to every system prompt so the LLM knows what tools exist
/// and how to call them using the `<tool_call>` XML format.
fn build_tools_section(registry: &ToolRegistry) -> String {
    let tools = registry.list();
    if tools.is_empty() {
        return String::new();
    }

    let mut s = String::from(r#"

## Available Tools

When you need to perform an action, output a tool call using **exactly** this XML format (on its own line):

<tool_call>{"name": "tool_name", "args": {"param": "value"}}</tool_call>

The system will execute the tool and return results as:

<tool_result name="tool_name" status="ok">{"field": "value"}</tool_result>

Rules:
- Only call tools when needed; prefer direct answers for conversational messages.
- You may call multiple tools sequentially across turns.
- Do NOT fabricate tool results; wait for the actual result before continuing.

### Tools

"#);

    for tool in &tools {
        s.push_str(&format!("#### `{}`\n{}\n\n", tool.name(), tool.description()));

        let schema = tool.parameters_schema();
        if let Some(props) = schema["properties"].as_object() {
            let required: Vec<&str> = schema["required"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();

            s.push_str("Parameters:\n");
            for (name, prop) in props {
                let type_str = prop["type"].as_str().unwrap_or("any");
                let desc = prop["description"].as_str().unwrap_or("");
                let req = if required.contains(&name.as_str()) { "required" } else { "optional" };
                if desc.is_empty() {
                    s.push_str(&format!("- `{}` ({}, {})\n", name, type_str, req));
                } else {
                    s.push_str(&format!("- `{}` ({}, {}): {}\n", name, type_str, req, desc));
                }
            }
            s.push('\n');
        }
    }

    s
}

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

    /// Returns a reference to the loaded skills.
    pub fn skills(&self) -> &[crate::skill::Skill] {
        self.skill_mgr.skills()
    }

    /// Replace the skill manager (used by FFI layer after loading new skills).
    pub fn replace_skills(&mut self, mgr: crate::skill::SkillManager) {
        self.skill_mgr = mgr;
    }

    pub async fn chat(&mut self, user_input: &str, base_system: &str) -> ClawResult<Vec<AgentEvent>> {
        let matched = self.skill_mgr.match_skills(user_input);
        let skill_prompt = self.skill_mgr.build_system_prompt(base_system, &matched);
        // Append tool descriptions so the LLM knows the call format and what tools exist.
        let tools_section = build_tools_section(&self.registry);
        let system = format!("{}{}", skill_prompt, tools_section);

        tracing::info!(
            user_input = %user_input,
            skills_matched = %matched.len(),
            tools_available = %self.registry.list().len(),
            "chat turn started"
        );
        tracing::debug!(system_prompt = %system, "full system prompt");

        self.history.push(Message::user(user_input));
        let mut all_events = Vec::new();

        for round in 0..MAX_TOOL_ROUNDS {
            tracing::debug!(round = %round, history_len = %self.history.len(), "starting LLM round");

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
                        tracing::error!(round = %round, error = %message, "LLM stream error");
                        return Err(crate::ClawError::Llm(message));
                    }
                }
            }

            tracing::debug!(round = %round, response_len = %full_text.len(), response = %full_text, "LLM response received");

            let tool_calls = extract_tool_calls(&full_text);
            tracing::info!(round = %round, tool_calls_found = %tool_calls.len(), "tool call extraction");

            if tool_calls.is_empty() {
                tracing::info!(round = %round, "no tool calls, turn complete");
                self.history.push(Message::assistant(&full_text));
                all_events.push(AgentEvent::Done);
                break;
            }

            let mut tool_results_xml = String::new();
            for call in &tool_calls {
                tracing::info!(tool = %call.name, args = %call.args, "executing tool");
                all_events.push(AgentEvent::ToolCall { name: call.name.clone() });
                let result = match self.registry.get(&call.name) {
                    Some(tool) => tool.execute(call.args.clone(), &self.ctx).await,
                    None => {
                        tracing::warn!(tool = %call.name, "tool not found in registry");
                        Err(crate::ClawError::Tool {
                            tool: call.name.clone(),
                            message: "tool not found".into(),
                        })
                    }
                };
                match result {
                    Ok(r) => {
                        tracing::info!(tool = %call.name, success = %r.success, output = %r.output, "tool result");
                        all_events.push(AgentEvent::ToolResult { name: call.name.clone(), success: r.success });
                        tool_results_xml.push_str(&format_tool_result(&call.name, r.success, &r.output));
                    }
                    Err(e) => {
                        tracing::error!(tool = %call.name, error = %e, "tool execution error");
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

        // Ensure Done is always the last event, even when tool rounds are exhausted.
        if !matches!(all_events.last(), Some(AgentEvent::Done)) {
            tracing::warn!("tool rounds exhausted without clean completion");
            all_events.push(AgentEvent::Done);
        }

        Ok(all_events)
    }
}

#[cfg(feature = "test-utils")]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        llm::client::test_helpers::MockLlmClient,
        memory::sqlite::SqliteMemory,
        secrets::store::test_helpers::NullSecretStore,
        skill::SkillManager,
        tools::{ToolContext, ToolRegistry, PermissionChecker, builtin::register_all_builtins},
    };
    use std::sync::Arc;
    use tempfile::TempDir;

    async fn make_agent(response: &str) -> (AgentLoop<MockLlmClient>, TempDir) {
        let dir = TempDir::new().unwrap();
        let mem = Arc::new(SqliteMemory::open(dir.path().join("mem.db")).await.unwrap());
        let mut registry = ToolRegistry::new();
        register_all_builtins(&mut registry);
        let ctx = ToolContext {
            memory: mem,
            sandbox_dir: dir.path().to_path_buf(),
            http_allowlist: vec![],
            permissions: Arc::new(PermissionChecker::allow_all()),
            secrets: Arc::new(NullSecretStore),
        };
        let agent = AgentLoop::new(
            MockLlmClient { response: response.to_string() },
            registry, ctx,
            SkillManager::new(vec![]),
        );
        (agent, dir)
    }

    #[tokio::test]
    async fn unknown_tool_produces_tool_result_error_event() {
        let (mut agent, _dir) = make_agent(
            r#"<tool_call>{"name": "nonexistent_tool", "args": {}}</tool_call>"#
        ).await;
        let events = agent.chat("test", "").await.unwrap();
        let result_events: Vec<_> = events.iter()
            .filter(|e| matches!(e, AgentEvent::ToolResult { success: false, .. }))
            .collect();
        assert!(!result_events.is_empty(), "unknown tool should produce a failed ToolResult event");
    }

    #[tokio::test]
    async fn skill_keyword_activates_skill() {
        use crate::skill::{SkillManager, types::{Skill, SkillManifest, SkillActivation, SkillTrust}};
        let skill = Skill {
            manifest: SkillManifest {
                name: "test-skill".into(),
                description: "test".into(),
                trust: SkillTrust::Bundled,
                activation: SkillActivation { keywords: vec!["activate_me".into()] },
                allowed_tools: None,
            },
            prompt: "You are a test skill.".into(),
        };
        let dir = TempDir::new().unwrap();
        let mem = Arc::new(SqliteMemory::open(dir.path().join("mem.db")).await.unwrap());
        let registry = ToolRegistry::new();
        let ctx = ToolContext {
            memory: mem,
            sandbox_dir: dir.path().to_path_buf(),
            http_allowlist: vec![],
            permissions: Arc::new(PermissionChecker::allow_all()),
            secrets: Arc::new(NullSecretStore),
        };
        let mgr = SkillManager::new(vec![skill]);
        let mut agent = AgentLoop::new(
            MockLlmClient { response: "skill activated".into() },
            registry, ctx, mgr,
        );
        let events = agent.chat("please activate_me", "Base system.").await.unwrap();
        // Just verify it completes without error and produces events
        assert!(!events.is_empty());
    }

    #[tokio::test]
    async fn empty_history_before_first_chat() {
        let (agent, _dir) = make_agent("hello").await;
        assert!(agent.history().is_empty());
    }

    #[tokio::test]
    async fn skills_getter_returns_loaded_skills() {
        use crate::skill::{SkillManager, types::{Skill, SkillManifest, SkillActivation, SkillTrust}};
        let skill = Skill {
            manifest: SkillManifest {
                name: "test-skill".into(),
                description: "test".into(),
                trust: SkillTrust::Bundled,
                activation: SkillActivation { keywords: vec!["test".into()] },
                allowed_tools: None,
            },
            prompt: "You are a test skill.".into(),
        };
        let dir = TempDir::new().unwrap();
        let mem = Arc::new(SqliteMemory::open(dir.path().join("mem.db")).await.unwrap());
        let registry = ToolRegistry::new();
        let ctx = ToolContext {
            memory: mem,
            sandbox_dir: dir.path().to_path_buf(),
            http_allowlist: vec![],
            permissions: Arc::new(PermissionChecker::allow_all()),
            secrets: Arc::new(NullSecretStore),
        };
        let mgr = SkillManager::new(vec![skill]);
        let mut agent = AgentLoop::new(
            MockLlmClient { response: "ok".into() },
            registry, ctx, mgr,
        );

        // skills() should return 1 skill named "test-skill"
        assert_eq!(agent.skills().len(), 1);
        assert_eq!(agent.skills()[0].manifest.name, "test-skill");

        // replace_skills() should replace the manager with an empty one
        agent.replace_skills(SkillManager::new(vec![]));
        assert_eq!(agent.skills().len(), 0);
    }

    #[tokio::test]
    async fn chat_tool_round_exhaustion_still_emits_done() {
        // MockLlmClient always returns a fixed text response.
        // By embedding a tool_call XML in the response, the loop will never
        // see a clean (tool-free) response, exhausting all MAX_TOOL_ROUNDS.
        // After exhaustion the Done guard must still be appended.
        let (mut agent, _dir) = make_agent(
            r#"<tool_call>{"name": "nonexistent_tool", "args": {}}</tool_call>"#
        ).await;
        let events = agent.chat("go", "").await.unwrap();
        assert!(
            matches!(events.last(), Some(AgentEvent::Done)),
            "last event must be Done even when tool rounds are exhausted, got: {:?}",
            events.last()
        );
    }
}

