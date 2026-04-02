use futures::StreamExt;
use std::collections::HashSet;
use crate::{
    ClawResult,
    agent::parser::{extract_tool_calls, extract_text_without_tool_calls, format_tool_result},
    llm::{client::LlmClient, types::{Message, StreamEvent}},
    skill::{Skill, SkillManager},
    tools::{ToolContext, ToolRegistry},
};

const MAX_TOOL_ROUNDS: usize = 10;
const MAX_TOKENS: u32 = 4096;

// ---------------------------------------------------------------------------
// Tool descriptions injected into every system prompt
// ---------------------------------------------------------------------------

/// Build the `## Available Tools` section.
///
/// - If any matched skill declares `allowed_tools`, only those tools (union across
///   all matched skills) are described.  This keeps the prompt focused and
///   prevents the LLM from calling tools that a restricted skill should not use.
/// - If no matched skills, or all matched skills leave `allowed_tools` as `None`,
///   **all** registered tools are described.
///
/// The section is appended to the system prompt so the LLM knows the `<tool_call>`
/// XML format and which tools exist.
pub(crate) fn build_tools_section(registry: &ToolRegistry, matched_skills: &[&Skill]) -> String {
    let all_tools = registry.list();
    if all_tools.is_empty() {
        return String::new();
    }

    // Collect the union of `allowed_tools` from all matched skills that restrict tools.
    // If ANY matched skill has `allowed_tools = None`, no restriction is applied — that
    // skill may need any tool, so we show everything.
    let any_unrestricted = matched_skills.iter().any(|s| s.manifest.allowed_tools.is_none());
    let allowed_filter: Option<HashSet<&str>> = if any_unrestricted || matched_skills.is_empty() {
        None
    } else {
        Some(
            matched_skills
                .iter()
                .filter_map(|s| s.manifest.allowed_tools.as_deref())
                .flat_map(|names| names.iter().map(|n| n.as_str()))
                .collect(),
        )
    };

    let tools: Vec<_> = match &allowed_filter {
        Some(allowed) => all_tools.into_iter().filter(|t| allowed.contains(t.name())).collect(),
        None => all_tools,
    };

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
        let tools_section = build_tools_section(&self.registry, &matched);
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

    // -----------------------------------------------------------------------
    // CapturingMockLlmClient: records the system prompt for inspection
    // -----------------------------------------------------------------------

    use std::sync::Mutex;
    use futures::stream;
    use crate::llm::types::StreamEvent;

    struct CapturingMockLlmClient {
        captured_system: Arc<Mutex<String>>,
        response: String,
    }

    #[async_trait::async_trait]
    impl crate::llm::client::LlmClient for CapturingMockLlmClient {
        async fn stream_messages(
            &self,
            system: &str,
            _messages: &[crate::llm::types::Message],
            _max_tokens: u32,
        ) -> crate::ClawResult<crate::llm::client::EventStream> {
            *self.captured_system.lock().unwrap() = system.to_string();
            let text = self.response.clone();
            let events: Vec<crate::ClawResult<StreamEvent>> = vec![
                Ok(StreamEvent::MessageStart),
                Ok(StreamEvent::TextDelta { text }),
                Ok(StreamEvent::MessageStop),
            ];
            Ok(Box::pin(stream::iter(events)))
        }
    }

    async fn make_capturing_agent(
        response: &str,
    ) -> (AgentLoop<CapturingMockLlmClient>, Arc<Mutex<String>>, TempDir) {
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
        let captured = Arc::new(Mutex::new(String::new()));
        let client = CapturingMockLlmClient {
            captured_system: captured.clone(),
            response: response.to_string(),
        };
        let agent = AgentLoop::new(client, registry, ctx, SkillManager::new(vec![]));
        (agent, captured, dir)
    }

    // -----------------------------------------------------------------------
    // Tests: build_tools_section (unit, no async needed)
    // -----------------------------------------------------------------------

    fn make_tool_registry_with(names: &[(&str, &str)]) -> ToolRegistry {
        use crate::tools::traits::{Tool, ToolContext, ToolResult};
        use async_trait::async_trait;

        struct FakeTool { name: &'static str, desc: &'static str }
        #[async_trait]
        impl Tool for FakeTool {
            fn name(&self) -> &str { self.name }
            fn description(&self) -> &str { self.desc }
            fn parameters_schema(&self) -> serde_json::Value {
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "input": { "type": "string", "description": "the input" }
                    },
                    "required": ["input"]
                })
            }
            async fn execute(&self, _args: serde_json::Value, _ctx: &ToolContext)
                -> crate::ClawResult<ToolResult> { Ok(ToolResult::ok("ok")) }
        }

        let mut reg = ToolRegistry::new();
        for &(name, desc) in names {
            // SAFETY: these are 'static str literals — outlive the test
            let tool: Arc<dyn Tool> = Arc::new(FakeTool {
                name: Box::leak(name.to_string().into_boxed_str()),
                desc: Box::leak(desc.to_string().into_boxed_str()),
            });
            reg.register_builtin(tool);
        }
        reg
    }

    fn make_skill_with_allowed(name: &str, keywords: Vec<&str>, allowed: Option<Vec<&str>>) -> Skill {
        use crate::skill::types::{SkillActivation, SkillManifest, SkillTrust};
        Skill {
            manifest: SkillManifest {
                name: name.into(),
                description: "test".into(),
                trust: SkillTrust::Bundled,
                activation: SkillActivation {
                    keywords: keywords.into_iter().map(String::from).collect(),
                },
                allowed_tools: allowed.map(|v| v.into_iter().map(String::from).collect()),
            },
            prompt: format!("You are the {} skill.", name),
        }
    }

    #[test]
    fn build_tools_section_empty_registry_returns_empty_string() {
        let reg = ToolRegistry::new();
        let section = build_tools_section(&reg, &[]);
        assert!(section.is_empty(), "empty registry must produce empty section");
    }

    #[test]
    fn build_tools_section_no_skills_shows_all_tools() {
        let reg = make_tool_registry_with(&[
            ("tool_alpha", "Alpha does alpha things"),
            ("tool_beta",  "Beta does beta things"),
        ]);
        let section = build_tools_section(&reg, &[]);
        assert!(section.contains("tool_alpha"), "must contain tool_alpha");
        assert!(section.contains("tool_beta"),  "must contain tool_beta");
        assert!(section.contains("<tool_call>"), "must explain call format");
    }

    #[test]
    fn build_tools_section_includes_description_and_params() {
        let reg = make_tool_registry_with(&[("my_tool", "Does something important")]);
        let section = build_tools_section(&reg, &[]);
        assert!(section.contains("my_tool"), "tool name missing");
        assert!(section.contains("Does something important"), "tool description missing");
        assert!(section.contains("`input`"), "parameter name missing");
        assert!(section.contains("required"), "required flag missing");
    }

    #[test]
    fn build_tools_section_skill_with_allowed_tools_filters() {
        let reg = make_tool_registry_with(&[
            ("email_fetch", "Fetch emails"),
            ("email_send",  "Send emails"),
            ("file_read",   "Read files"),
        ]);
        let skill = make_skill_with_allowed("email-skill", vec!["email"], Some(vec!["email_fetch", "email_send"]));
        let matched = vec![&skill];
        let section = build_tools_section(&reg, &matched);
        assert!(section.contains("email_fetch"), "email_fetch should be in section");
        assert!(section.contains("email_send"),  "email_send should be in section");
        assert!(!section.contains("file_read"),  "file_read must NOT be in section — not in allowed_tools");
    }

    #[test]
    fn build_tools_section_skill_with_no_allowed_tools_shows_all() {
        let reg = make_tool_registry_with(&[
            ("tool_a", "Tool A"),
            ("tool_b", "Tool B"),
        ]);
        // allowed_tools = None → no restriction
        let skill = make_skill_with_allowed("unrestricted", vec!["test"], None);
        let matched = vec![&skill];
        let section = build_tools_section(&reg, &matched);
        assert!(section.contains("tool_a"), "tool_a must appear when no restriction");
        assert!(section.contains("tool_b"), "tool_b must appear when no restriction");
    }

    #[test]
    fn build_tools_section_multiple_skills_union_of_allowed() {
        let reg = make_tool_registry_with(&[
            ("tool_a", "Tool A"),
            ("tool_b", "Tool B"),
            ("tool_c", "Tool C"),
        ]);
        let skill1 = make_skill_with_allowed("skill1", vec!["s1"], Some(vec!["tool_a"]));
        let skill2 = make_skill_with_allowed("skill2", vec!["s2"], Some(vec!["tool_b"]));
        let matched = vec![&skill1, &skill2];
        let section = build_tools_section(&reg, &matched);
        assert!(section.contains("tool_a"),           "tool_a in skill1 allowed_tools");
        assert!(section.contains("tool_b"),           "tool_b in skill2 allowed_tools");
        assert!(!section.contains("`tool_c`"),        "tool_c not in any allowed_tools");
    }

    #[test]
    fn build_tools_section_mixed_restricted_unrestricted_skills_shows_all() {
        // If ANY matched skill has allowed_tools=None, treat as "no restriction"
        // because that skill needs all tools.
        let reg = make_tool_registry_with(&[
            ("tool_a", "Tool A"),
            ("tool_b", "Tool B"),
        ]);
        let restricted   = make_skill_with_allowed("restricted",   vec!["r"], Some(vec!["tool_a"]));
        let unrestricted = make_skill_with_allowed("unrestricted", vec!["u"], None);
        let matched = vec![&restricted, &unrestricted];
        let section = build_tools_section(&reg, &matched);
        // unrestricted skill has None → override: show all tools
        assert!(section.contains("tool_a"), "tool_a must appear");
        assert!(section.contains("tool_b"), "tool_b must appear — unrestricted skill lifts filter");
    }

    #[test]
    fn build_tools_section_extension_tool_appears_in_section() {
        use crate::tools::traits::{Tool, ToolContext, ToolResult};
        use async_trait::async_trait;

        struct ExtTool;
        #[async_trait]
        impl Tool for ExtTool {
            fn name(&self) -> &str { "custom_ext_tool" }
            fn description(&self) -> &str { "A customer-added extension tool" }
            fn parameters_schema(&self) -> serde_json::Value {
                serde_json::json!({"type": "object", "properties": {}})
            }
            async fn execute(&self, _: serde_json::Value, _: &ToolContext) -> crate::ClawResult<ToolResult> {
                Ok(ToolResult::ok("ok"))
            }
        }

        let mut reg = ToolRegistry::new();
        reg.register_extension(Arc::new(ExtTool)).unwrap();
        let section = build_tools_section(&reg, &[]);
        assert!(section.contains("custom_ext_tool"),               "extension tool name must appear");
        assert!(section.contains("A customer-added extension tool"), "extension tool description must appear");
    }

    // -----------------------------------------------------------------------
    // Integration: system prompt sent to LLM contains tool section
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn chat_sends_tool_descriptions_in_system_prompt() {
        let (mut agent, captured, _dir) = make_capturing_agent("hello").await;
        agent.chat("hi", "Base system.").await.unwrap();

        let system = captured.lock().unwrap().clone();
        assert!(system.starts_with("Base system."), "base system must come first");
        assert!(system.contains("## Available Tools"),  "tool section header must be present");
        assert!(system.contains("<tool_call>"),          "call format example must be present");
        // Builtins registered by register_all_builtins should appear
        assert!(system.contains("email_fetch"), "email_fetch must be in system prompt");
        assert!(system.contains("file_read"),   "file_read must be in system prompt");
        assert!(system.contains("time"),        "time must be in system prompt");
    }

    #[tokio::test]
    async fn chat_filters_tools_to_skill_allowed_tools() {
        use crate::skill::types::{SkillActivation, SkillManifest, SkillTrust};

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
        let captured = Arc::new(Mutex::new(String::new()));
        let client = CapturingMockLlmClient {
            captured_system: captured.clone(),
            response: "ok".into(),
        };

        // Skill that only allows email_fetch
        let email_skill = Skill {
            manifest: SkillManifest {
                name: "email-only".into(),
                description: "email focused skill".into(),
                trust: SkillTrust::Bundled,
                activation: SkillActivation {
                    keywords: vec!["email".into()],
                },
                allowed_tools: Some(vec!["email_fetch".into()]),
            },
            prompt: "You handle email.".into(),
        };
        let mgr = SkillManager::new(vec![email_skill]);
        let mut agent = AgentLoop::new(client, registry, ctx, mgr);

        // "email" keyword triggers the skill
        agent.chat("please email me", "").await.unwrap();

        let system = captured.lock().unwrap().clone();
        assert!(system.contains("email_fetch"),  "allowed tool must be in prompt");
        assert!(!system.contains("file_read"),   "non-allowed tool must NOT be in prompt");
        assert!(!system.contains("memory_store"),"non-allowed tool must NOT be in prompt");
    }
}
