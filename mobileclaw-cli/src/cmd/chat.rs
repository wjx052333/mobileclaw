use anyhow::Result;
use std::path::Path;
use rustyline::{DefaultEditor, error::ReadlineError};
use mobileclaw_core::ffi::AgentEventDto;
use crate::session::{open_session, init_logging};

pub async fn cmd_chat(data_dir: &Path, system: Option<String>) -> Result<()> {
    init_logging();
    tracing::info!(data_dir = %data_dir.display(), "mclaw chat starting");

    println!("Opening agent session...");
    println!("(Logs → ./mclaw.log)");
    let mut session = open_session(data_dir).await?;
    let system = system.unwrap_or_else(|| {
        "You are a helpful assistant. You have access to tools for email, files, memory, and web requests. \
         Use tools whenever the user asks you to perform an action.".into()
    });

    tracing::info!(system_base = %system, "session ready");
    println!("Chat started. Type '/quit' or Ctrl-D to exit.\n");

    let mut rl = DefaultEditor::new()?;
    loop {
        let line = match rl.readline("you> ") {
            Ok(l) => l,
            Err(ReadlineError::Eof | ReadlineError::Interrupted) => break,
            Err(e) => return Err(e.into()),
        };
        let input = line.trim().to_string();
        if input.is_empty() { continue; }
        if input == "/quit" || input == "/exit" { break; }
        let _ = rl.add_history_entry(&input);

        tracing::info!(input = %input, "user message");
        let events = match session.chat(input, system.clone()).await {
            Ok(e) => e,
            Err(e) => {
                tracing::error!(error = %e, "chat error");
                eprintln!("Error: {e}");
                continue;
            }
        };

        print!("agent> ");
        for event in events {
            match event {
                AgentEventDto::TextDelta { text } => print!("{text}"),
                AgentEventDto::ToolCall { name } => {
                    println!("\n[tool call: {name}]");
                }
                AgentEventDto::ToolResult { name, success } => {
                    let status = if success { "ok" } else { "error" };
                    println!("  [{name}: {status}]");
                }
                AgentEventDto::Done => {}
            }
        }
        println!();
    }

    println!("Bye.");
    Ok(())
}
