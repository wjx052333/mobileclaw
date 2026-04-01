use anyhow::Result;
use std::path::Path;
use rustyline::{DefaultEditor, error::ReadlineError};
use mobileclaw_core::ffi::AgentEventDto;
use crate::session::open_session;

pub async fn cmd_chat(data_dir: &Path, system: Option<String>) -> Result<()> {
    println!("Opening agent session...");
    let mut session = open_session(data_dir).await?;
    let system = system.unwrap_or_else(|| {
        "You are a helpful assistant. You have access to email tools. \
         When the user asks to fetch or send email, use the email tools directly.".into()
    });

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

        let events = match session.chat(input, system.clone()).await {
            Ok(e) => e,
            Err(e) => {
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
