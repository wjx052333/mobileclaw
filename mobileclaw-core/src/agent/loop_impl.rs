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

const SUMMARY_SYSTEM: &str =
    "Summarize the following AI assistant interaction in exactly one sentence. \
     Output only the summary sentence, nothing else.";
const SUMMARY_MAX_TOKENS: u32 = 150;

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

**CRITICAL JSON formatting rules — violations will silently drop your tool call:**
- The JSON object must have exactly one comma between `"name"` and `"args"`: `{"name": "...", "args": {...}}`
- Every key must be double-quoted: `"name"`, `"args"`, not `name` or `args`
- Do NOT write `{"name": "foo" "args": ...}` (missing comma) — this is invalid JSON
- Do NOT write `{"name": "foo" args": ...}` (missing comma AND missing opening quote) — also invalid

Correct multi-parameter example:
<tool_call>{"name": "memory_search", "args": {"query": "rust async", "limit": 10}}</tool_call>

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

/// Snapshot of context-window state emitted once per `chat()` call,
/// just before returning events. Useful for bench/monitoring tooling.
#[derive(Debug, Clone)]
pub struct ContextStats {
    /// Estimated tokens in history *before* this turn's user message was pushed.
    pub tokens_before_turn: usize,
    /// Estimated tokens after pruning (or same as before if no pruning triggered).
    pub tokens_after_prune: usize,
    /// Number of messages removed by the pruner this turn (0 if no pruning).
    pub messages_pruned: usize,
    /// Current message count in history after this full chat() call.
    pub history_len: usize,
    /// Configured pruning threshold (max_tokens - buffer_tokens).
    pub pruning_threshold: usize,
}

#[derive(Debug, Clone)]
pub enum AgentEvent {
    TextDelta { text: String },
    ToolCall { name: String },
    ToolResult { name: String, success: bool },
    /// Context-window observability snapshot — emitted once per chat() turn,
    /// as the second-to-last event (before Done).
    ContextStats(ContextStats),
    Done,
}

pub struct AgentLoop<L: LlmClient> {
    llm: L,
    registry: ToolRegistry,
    ctx: ToolContext,
    skill_mgr: SkillManager,
    history: Vec<Message>,
    // Context management: threshold-based pruning of message history.
    ctx_config: crate::agent::context_manager::ContextConfig,
    // Optional directory for session transcript persistence.
    session_dir: Option<std::path::PathBuf>,
}

impl<L: LlmClient> AgentLoop<L> {
    pub fn new(llm: L, registry: ToolRegistry, ctx: ToolContext, skill_mgr: SkillManager) -> Self {
        Self {
            llm,
            registry,
            ctx,
            skill_mgr,
            history: Vec::new(),
            ctx_config: crate::agent::context_manager::ContextConfig::default(),
            session_dir: None,
        }
    }

    /// Override the context pruning configuration.
    pub fn with_context_config(mut self, config: crate::agent::context_manager::ContextConfig) -> Self {
        self.ctx_config = config;
        self
    }

    /// Set the directory for session transcript persistence.
    /// When set, the full message history is saved to a JSONL file after every chat turn.
    pub fn with_session_dir(mut self, dir: std::path::PathBuf) -> Self {
        self.session_dir = Some(dir);
        self
    }

    pub fn history(&self) -> &[Message] { &self.history }

    /// Replace the message history (used by session_load).
    pub fn set_history(&mut self, messages: Vec<Message>) {
        self.history = messages;
    }

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

        // Snapshot token count *before* pruning — used in ContextStats event.
        let tokens_before_turn = crate::agent::token_counter::estimate_tokens(&self.history);

        // Context pruning: remove oldest messages when approaching context window
        let current_tokens = tokens_before_turn;
        let threshold = crate::agent::context_manager::pruning_threshold(&self.ctx_config);
        let mut messages_pruned: usize = 0;
        let mut tokens_after_prune = current_tokens;
        if current_tokens > threshold {
            match crate::agent::context_manager::prune_oldest_messages(
                &mut self.history,
                threshold,
                current_tokens,
                self.ctx_config.min_user_turns,
            ) {
                Ok(result) if result.pruned_count > 0 => {
                    messages_pruned = result.pruned_count;
                    tokens_after_prune = result.tokens_after;
                    tracing::info!(
                        pruned = result.pruned_count,
                        tokens_before = result.tokens_before,
                        tokens_after = result.tokens_after,
                        history_len = self.history.len(),
                        "context pruned"
                    );
                }
                Ok(_) => {} // nothing pruned
                Err(e) => {
                    // Non-fatal: log and continue with unpruned history
                    tracing::warn!(error = %e, "context pruning failed");
                }
            }
        }

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

        // Inject ContextStats before Done so bench/monitoring tools can observe it.
        // Insert second-to-last (before Done).
        let done_pos = all_events.len() - 1;
        all_events.insert(done_pos, AgentEvent::ContextStats(ContextStats {
            tokens_before_turn,
            tokens_after_prune,
            messages_pruned,
            history_len: self.history.len(),
            pruning_threshold: threshold,
        }));

        // Session persistence: save history to disk if configured (non-fatal)
        if let Some(ref dir) = self.session_dir {
            if let Err(e) = crate::agent::session::save_session(dir, &self.history).await {
                tracing::warn!(error = %e, "failed to save session transcript");
            }
        }

        Ok(all_events)
    }

    /// Return the ascending indices of messages in history that would be
    /// dropped by count-based pruning.  Returns an empty vec if
    /// `ctx_config.max_messages` is `None` or history is already within limit.
    pub fn count_prune_candidates(&self) -> Vec<usize> {
        let Some(max) = self.ctx_config.max_messages else { return vec![] };
        crate::agent::context_manager::count_prune_candidates(
            &self.history,
            max,
            self.ctx_config.min_user_turns,
        )
    }

    /// Remove `candidates` from history and optionally prepend `prefix_msg`
    /// at index 0 of the surviving history.
    ///
    /// `candidates` must be ascending indices produced by `count_prune_candidates`.
    /// This is a no-op if `candidates` is empty.
    pub fn apply_count_prune(&mut self, candidates: &[usize], prefix_msg: Option<crate::llm::types::Message>) {
        if candidates.is_empty() {
            return;
        }
        let removed_set: std::collections::HashSet<usize> = candidates.iter().copied().collect();
        let old = std::mem::take(&mut self.history);
        self.history = old
            .into_iter()
            .enumerate()
            .filter_map(|(i, m)| if removed_set.contains(&i) { None } else { Some(m) })
            .collect();
        if let Some(msg) = prefix_msg {
            self.history.insert(0, msg);
        }
    }

    /// Make a lightweight LLM call to summarize an interaction.
    /// Does NOT modify `self.history`.
    /// Returns the trimmed summary string.
    pub async fn summarize_interaction(&self, interaction_text: &str) -> crate::ClawResult<String> {
        let msgs = vec![crate::llm::types::Message::user(interaction_text)];
        let mut stream = self.llm.stream_messages(SUMMARY_SYSTEM, &msgs, SUMMARY_MAX_TOKENS).await?;
        let mut summary = String::new();
        while let Some(event) = stream.next().await {
            match event? {
                crate::llm::types::StreamEvent::TextDelta { text } => summary.push_str(&text),
                crate::llm::types::StreamEvent::MessageStop => break,
                _ => {}
            }
        }
        Ok(summary.trim().to_string())
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

    // -----------------------------------------------------------------------
    // New: context pruning + session persistence integration tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn context_pruning_fires_when_threshold_exceeded() {
        use crate::agent::context_manager::ContextConfig;
        // Use a very small context window to force pruning
        let config = ContextConfig { max_tokens: 50, buffer_tokens: 10, min_user_turns: 2, max_messages: None };
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
        let mut agent = AgentLoop::new(
            MockLlmClient { response: "ok".to_string() },
            registry, ctx, SkillManager::new(vec![]),
        ).with_context_config(config);

        // Pump enough turns to exceed the tiny context window
        for i in 0..10 {
            agent.chat(&format!("message {i} with extra padding to exceed token budget"), "").await.unwrap();
        }

        // History should be bounded (pruning fired), not 20+ messages
        let history_len = agent.history().len();
        assert!(history_len < 20, "history must be pruned, got {} messages", history_len);
    }

    #[tokio::test]
    async fn session_save_creates_file_when_dir_configured() {
        let dir = TempDir::new().unwrap();
        let session_dir = dir.path().join("sessions");

        let mem = Arc::new(SqliteMemory::open(dir.path().join("mem.db")).await.unwrap());
        let registry = ToolRegistry::new();
        let ctx = ToolContext {
            memory: mem,
            sandbox_dir: dir.path().to_path_buf(),
            http_allowlist: vec![],
            permissions: Arc::new(PermissionChecker::allow_all()),
            secrets: Arc::new(NullSecretStore),
        };
        let mut agent = AgentLoop::new(
            MockLlmClient { response: "hello".to_string() },
            registry, ctx, SkillManager::new(vec![]),
        ).with_session_dir(session_dir.clone());

        agent.chat("hi", "").await.unwrap();

        // Session file should exist
        let files: Vec<_> = std::fs::read_dir(&session_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".jsonl"))
            .collect();
        assert_eq!(files.len(), 1, "exactly one session file should be saved");
    }

    // -----------------------------------------------------------------------
    // Tests: count_prune_candidates and apply_count_prune
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn count_prune_candidates_returns_empty_when_max_messages_none() {
        let (mut agent, _dir) = make_agent("ok").await;
        // Pump a few messages into history
        agent.chat("msg1", "").await.unwrap();
        agent.chat("msg2", "").await.unwrap();
        // Default config has max_messages = None → should return empty
        let candidates = agent.count_prune_candidates();
        assert!(candidates.is_empty(), "must return empty when max_messages is None");
    }

    #[tokio::test]
    async fn count_prune_candidates_returns_candidates_when_over_limit() {
        use crate::agent::context_manager::ContextConfig;
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
        // max_messages = 3, min_user_turns = 1 so we can actually get candidates with a small history
        let config = ContextConfig { max_tokens: 200_000, buffer_tokens: 13_000, min_user_turns: 1, max_messages: Some(3) };
        let mut agent = AgentLoop::new(
            MockLlmClient { response: "ok".to_string() },
            registry, ctx, SkillManager::new(vec![]),
        ).with_context_config(config);

        // Push 5 messages manually (2 user + 1 assistant per chat call would be too many rounds)
        // Use set_history to precisely control message count
        let msgs = vec![
            crate::llm::types::Message::user("a"),
            crate::llm::types::Message::assistant("ra"),
            crate::llm::types::Message::user("b"),
            crate::llm::types::Message::assistant("rb"),
            crate::llm::types::Message::user("c"),
        ];
        agent.set_history(msgs);

        let candidates = agent.count_prune_candidates();
        // 5 messages, limit 3 → 2 candidates
        assert_eq!(candidates.len(), 2, "must return 2 candidates for 5 msgs with max=3, got {:?}", candidates);
        // Candidates must be in ascending order
        assert!(candidates.windows(2).all(|w| w[0] < w[1]), "candidates must be ascending");
    }

    #[tokio::test]
    async fn apply_count_prune_removes_candidates() {
        use crate::agent::context_manager::ContextConfig;
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
        let config = ContextConfig { max_tokens: 200_000, buffer_tokens: 13_000, min_user_turns: 1, max_messages: Some(3) };
        let mut agent = AgentLoop::new(
            MockLlmClient { response: "ok".to_string() },
            registry, ctx, SkillManager::new(vec![]),
        ).with_context_config(config);

        let msgs = vec![
            crate::llm::types::Message::user("a"),
            crate::llm::types::Message::assistant("ra"),
            crate::llm::types::Message::user("b"),
            crate::llm::types::Message::assistant("rb"),
            crate::llm::types::Message::user("c"),
        ];
        agent.set_history(msgs);

        let candidates = agent.count_prune_candidates();
        assert_eq!(candidates.len(), 2);

        agent.apply_count_prune(&candidates, None);

        assert_eq!(agent.history().len(), 3, "history must be trimmed to 3 after pruning");
    }

    #[tokio::test]
    async fn apply_count_prune_inserts_prefix_at_index_0() {
        use crate::agent::context_manager::ContextConfig;
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
        let config = ContextConfig { max_tokens: 200_000, buffer_tokens: 13_000, min_user_turns: 1, max_messages: Some(3) };
        let mut agent = AgentLoop::new(
            MockLlmClient { response: "ok".to_string() },
            registry, ctx, SkillManager::new(vec![]),
        ).with_context_config(config);

        let msgs = vec![
            crate::llm::types::Message::user("a"),
            crate::llm::types::Message::assistant("ra"),
            crate::llm::types::Message::user("b"),
            crate::llm::types::Message::assistant("rb"),
            crate::llm::types::Message::user("c"),
        ];
        agent.set_history(msgs);

        let candidates = agent.count_prune_candidates();
        let prefix = crate::llm::types::Message::user("[summary of pruned context]");
        agent.apply_count_prune(&candidates, Some(prefix));

        // History is 3 (after pruning) + 1 prefix = 4
        assert_eq!(agent.history().len(), 4, "must have 3 survivors + 1 prefix");
        assert_eq!(
            agent.history()[0].text_content(),
            "[summary of pruned context]",
            "prefix must be at index 0"
        );
    }
}
