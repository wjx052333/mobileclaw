mod cmd;
mod env_parser;
mod session;

use std::path::PathBuf;
use clap::{Parser, Subcommand};
use session::default_data_dir;

#[derive(Parser)]
#[command(name = "mclaw", about = "mobileclaw-core CLI — test all Rust APIs interactively")]
struct Cli {
    /// Data directory (default: ~/.mobileclaw/)
    #[arg(long, global = true, env = "MCLAW_DATA_DIR")]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Manage LLM provider configurations
    Provider {
        #[command(subcommand)]
        action: ProviderCmd,
    },
    /// Manage email accounts
    Email {
        #[command(subcommand)]
        action: EmailCmd,
    },
    /// Start interactive agent chat
    Chat {
        /// Override system prompt
        #[arg(long)]
        system: Option<String>,
    },
    /// Run context-window stress benchmark from a prompts JSON file
    Bench {
        /// Path to bench_prompts.json (default: docs/bench_prompts.json relative to cwd)
        #[arg(long, default_value = "docs/bench_prompts.json")]
        prompts: std::path::PathBuf,
        /// Override system prompt
        #[arg(long)]
        system: Option<String>,
        /// Only run the first N turns
        #[arg(long)]
        max_turns: Option<usize>,
        /// Print prompts without calling LLM (for inspection)
        #[arg(long)]
        dry_run: bool,
        /// Write full interaction records (history, response, stats) to this JSONL file
        #[arg(long)]
        interaction_log: Option<std::path::PathBuf>,
    },
}

#[derive(Subcommand)]
enum ProviderCmd {
    /// Add a new LLM provider
    Add {
        #[arg(long)] name: String,
        /// Protocol: anthropic | openai_compat | ollama
        #[arg(long)] protocol: String,
        #[arg(long)] url: String,
        #[arg(long)] model: String,
        #[arg(long)] key: Option<String>,
        #[arg(long, default_value = "true")] active: bool,
    },
    List,
    SetActive { id: String },
    Delete { id: String },
    Probe {
        #[arg(long)] id: Option<String>,
        #[arg(long)] protocol: Option<String>,
        #[arg(long)] url: Option<String>,
        #[arg(long)] model: Option<String>,
        #[arg(long)] key: Option<String>,
    },
}

#[derive(Subcommand)]
enum EmailCmd {
    /// Import email account from test_env.sh
    AddFromEnv {
        #[arg(long, default_value = "default")] id: String,
        #[arg(long, default_value = "test_env.sh")] env_file: PathBuf,
    },
    Add {
        #[arg(long)] id: String,
        #[arg(long)] smtp_host: String,
        #[arg(long, default_value = "465")] smtp_port: u16,
        #[arg(long)] imap_host: String,
        #[arg(long, default_value = "993")] imap_port: u16,
        #[arg(long)] username: String,
        #[arg(long)] password: String,
    },
    Delete { id: String },
    List,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // rustls 0.23 requires an explicit CryptoProvider even when compiled with the `ring` feature.
    // install_default() is idempotent — safe to call multiple times.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cli = Cli::parse();
    let data_dir = cli.data_dir.unwrap_or_else(default_data_dir);

    match cli.command {
        Command::Provider { action } => match action {
            ProviderCmd::Add { name, protocol, url, model, key, active } => {
                cmd::provider::cmd_provider_add(&data_dir, name, protocol, url, model, key, active).await?;
            }
            ProviderCmd::List => { cmd::provider::cmd_provider_list(&data_dir).await?; }
            ProviderCmd::SetActive { id } => { cmd::provider::cmd_provider_set_active(&data_dir, id).await?; }
            ProviderCmd::Delete { id } => { cmd::provider::cmd_provider_delete(&data_dir, id).await?; }
            ProviderCmd::Probe { id, protocol, url, model, key } => {
                cmd::provider::cmd_provider_probe(&data_dir, id, protocol, url, model, key).await?;
            }
        },
        Command::Email { action } => match action {
            EmailCmd::AddFromEnv { id, env_file } => {
                cmd::email::cmd_email_add_from_env(&data_dir, &env_file, &id).await?;
            }
            EmailCmd::Add { id, smtp_host, smtp_port, imap_host, imap_port, username, password } => {
                cmd::email::cmd_email_add(&data_dir, id, smtp_host, smtp_port, imap_host, imap_port, username, password).await?;
            }
            EmailCmd::Delete { id } => { cmd::email::cmd_email_delete(&data_dir, id).await?; }
            EmailCmd::List => { cmd::email::cmd_email_list(&data_dir).await?; }
        },
        Command::Chat { system } => {
            cmd::chat::cmd_chat(&data_dir, system).await?;
        }
        Command::Bench { prompts, system, max_turns, dry_run, interaction_log } => {
            cmd::bench::cmd_bench(&data_dir, &prompts, system, max_turns, dry_run, interaction_log.as_deref()).await?;
        }
    }

    Ok(())
}
