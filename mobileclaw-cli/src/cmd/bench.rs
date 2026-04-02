use anyhow::{Context, Result};
use mobileclaw_core::ffi::AgentEventDto;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use crate::session::{init_logging, open_session};

// ─── JSON schema ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct BenchFile {
    meta: BenchMeta,
    turns: Vec<BenchTurn>,
}

#[derive(Debug, Deserialize)]
struct BenchMeta {
    description: String,
    pruning_threshold_approx: usize,
}

#[derive(Debug, Deserialize)]
struct BenchTurn {
    id: u32,
    label: String,
    prompt: String,
}

// ─── Interaction log record (one per turn, written as JSONL) ─────────────────

#[derive(Debug, Serialize)]
struct InteractionRecord {
    turn_id: u32,
    label: String,
    timestamp_ms: u64,
    system: String,
    prompt: String,
    history_before: Vec<HistoryEntry>,
    response_text: String,
    history_after: Vec<HistoryEntry>,
    context_stats: Option<ContextStatsRecord>,
    events_seen: Vec<String>,
    elapsed_ms: u128,
}

#[derive(Debug, Serialize)]
struct HistoryEntry {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ContextStatsRecord {
    tokens_before_turn: usize,
    tokens_after_prune: usize,
    messages_pruned: usize,
    history_len: usize,
    pruning_threshold: usize,
}

// ─── Per-turn metrics ────────────────────────────────────────────────────────

/// All fields are read in the summary loop at the end of cmd_bench(); the dead_code
/// lint fires because rustc doesn't track struct-field reads across match arms.
#[allow(dead_code)]
#[derive(Debug)]
struct TurnMetrics {
    id: u32,
    label: String,
    elapsed_ms: u128,
    tokens_before_turn: usize,
    tokens_after_prune: usize,
    messages_pruned: usize,
    history_len: usize,
    pruning_threshold: usize,
    response_chars: usize,
    pruning_fired: bool,
}

// ─── RSS helper ──────────────────────────────────────────────────────────────

/// Read current RSS (Resident Set Size) in KiB from /proc/self/status.
/// Returns 0 on any read/parse failure (non-Linux or permission denied).
fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<u64>().ok())
        })
        .unwrap_or(0)
}

// ─── Main command ─────────────────────────────────────────────────────────────

pub async fn cmd_bench(
    data_dir: &Path,
    prompts_file: &Path,
    system: Option<String>,
    max_turns: Option<usize>,
    dry_run: bool,
    interaction_log: Option<&Path>,
    turn_delay_ms: u64,
) -> Result<()> {
    init_logging();

    // ── Load prompts ──────────────────────────────────────────────────────────
    let json_src = std::fs::read_to_string(prompts_file)
        .with_context(|| format!("reading {}", prompts_file.display()))?;
    let bench: BenchFile =
        serde_json::from_str(&json_src).context("parsing bench_prompts.json")?;

    let turns: Vec<&BenchTurn> = bench
        .turns
        .iter()
        .take(max_turns.unwrap_or(usize::MAX))
        .collect();

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║            mobileclaw context-window stress bench            ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║ {:<62}║", bench.meta.description.chars().take(62).collect::<String>());
    println!("║ turns: {:>3}   prune threshold ≈ {:>8} tokens{:>13}║",
        turns.len(), bench.meta.pruning_threshold_approx, "");
    if dry_run {
        println!("║ *** DRY RUN — prompts printed, no LLM calls made ***{:>11}║", "");
    }
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    if dry_run {
        for t in &turns {
            let preview: String = t.prompt.chars().take(120).collect();
            println!("  Turn {:>2} │ {} │ {} chars", t.id, t.label, t.prompt.len());
            println!("           │ {}…", preview);
            println!();
        }
        return Ok(());
    }

    // ── Open session ──────────────────────────────────────────────────────────
    println!("Opening agent session...");
    let mut session = open_session(data_dir).await?;
    let system = system.unwrap_or_else(|| {
        "You are a senior Rust systems engineer. Answer questions thoroughly with code examples. \
         Be detailed — this is a technical deep-dive session."
            .into()
    });

    // ── Open interaction log (optional) ──────────────────────────────────────
    let mut ilog: Option<std::io::BufWriter<std::fs::File>> = if let Some(p) = interaction_log {
        let f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .with_context(|| format!("opening interaction log {}", p.display()))?;
        println!("Interaction log: {}", p.display());
        Some(std::io::BufWriter::new(f))
    } else {
        None
    };

    // ── Print table header ───────────────────────────────────────────────────
    println!(
        "{:>4}  {:<28}  {:>8}  {:>7}  {:>7}  {:>6}  {:>5}  {:>7}  {:>7}",
        "turn", "label", "elapsed", "tok_bef", "tok_aft", "pruned", "h_len", "resp_ch", "rss_MiB"
    );
    println!("{}", "─".repeat(100));

    let mut all_metrics: Vec<TurnMetrics> = Vec::with_capacity(turns.len());
    let bench_start = Instant::now();
    let bench_start_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    const RETRY_429_WAIT_SECS: u64 = 30;

    for (turn_idx, turn) in turns.iter().enumerate() {
        // Inter-turn delay (skip before first turn)
        if turn_delay_ms > 0 && turn_idx > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(turn_delay_ms)).await;
        }

        let rss_before = rss_kib();
        let _ = rss_before; // used only for potential delta reporting; suppress dead_code warning
        let t_start = Instant::now();

        let label_short: String = turn.label.chars().take(28).collect();

        // Snapshot history before this turn (for interaction log)
        let history_before: Vec<HistoryEntry> = if ilog.is_some() {
            session.history().into_iter().map(|m| HistoryEntry { role: m.role, content: m.content }).collect()
        } else {
            vec![]
        };

        // Attempt chat; retry once on 429 rate-limit after RETRY_429_WAIT_SECS.
        let chat_result = session.chat(turn.prompt.clone(), system.clone()).await;
        let chat_result = match chat_result {
            Err(ref e) if e.to_string().contains("429") => {
                println!(
                    "       ⏳ turn {} rate-limited (429), retrying in {}s…",
                    turn.id, RETRY_429_WAIT_SECS
                );
                tokio::time::sleep(std::time::Duration::from_secs(RETRY_429_WAIT_SECS)).await;
                session.chat(turn.prompt.clone(), system.clone()).await
            }
            other => other,
        };

        let events = match chat_result {
            Ok(evts) => evts,
            Err(e) => {
                let elapsed = t_start.elapsed();
                println!(
                    "{:>4}  {:<28}  {:>7}ms  ERROR: {}",
                    turn.id, label_short, elapsed.as_millis(), e
                );
                all_metrics.push(TurnMetrics {
                    id: turn.id,
                    label: turn.label.clone(),
                    elapsed_ms: elapsed.as_millis(),
                    tokens_before_turn: 0,
                    tokens_after_prune: 0,
                    messages_pruned: 0,
                    history_len: 0,
                    pruning_threshold: 0,
                    response_chars: 0,
                    pruning_fired: false,
                });
                continue;
            }
        };

        let elapsed = t_start.elapsed();

        // Extract ContextStats, response text, and event type names
        let mut ctx_stats: Option<(usize, usize, usize, usize, usize)> = None;
        let mut response_chars: usize = 0;
        let mut response_text = String::new();
        let mut events_seen: Vec<String> = Vec::new();
        for event in &events {
            match event {
                AgentEventDto::TextDelta { text } => {
                    response_chars += text.len();
                    if ilog.is_some() {
                        response_text.push_str(text);
                    }
                    events_seen.push("TextDelta".to_string());
                }
                AgentEventDto::ContextStats {
                    tokens_before_turn,
                    tokens_after_prune,
                    messages_pruned,
                    history_len,
                    pruning_threshold,
                } => {
                    ctx_stats = Some((
                        *tokens_before_turn,
                        *tokens_after_prune,
                        *messages_pruned,
                        *history_len,
                        *pruning_threshold,
                    ));
                    events_seen.push("ContextStats".to_string());
                }
                AgentEventDto::ToolCall { .. } => events_seen.push("ToolCall".to_string()),
                AgentEventDto::ToolResult { .. } => events_seen.push("ToolResult".to_string()),
                AgentEventDto::Done => events_seen.push("Done".to_string()),
            }
        }

        let (tok_before, tok_after, pruned, h_len, threshold) =
            ctx_stats.unwrap_or((0, 0, 0, 0, 0));
        let pruning_fired = pruned > 0;
        let rss_after = rss_kib();
        let rss_mib = rss_after / 1024;

        // Write interaction log record
        if let Some(ref mut log_writer) = ilog {
            let history_after: Vec<HistoryEntry> = session.history().into_iter()
                .map(|m| HistoryEntry { role: m.role, content: m.content })
                .collect();

            let record = InteractionRecord {
                turn_id: turn.id,
                label: turn.label.clone(),
                timestamp_ms: bench_start_unix + bench_start.elapsed().as_millis() as u64,
                system: system.clone(),
                prompt: turn.prompt.clone(),
                history_before,
                response_text,
                history_after,
                context_stats: ctx_stats.map(|(tb, ta, mp, hl, pt)| ContextStatsRecord {
                    tokens_before_turn: tb,
                    tokens_after_prune: ta,
                    messages_pruned: mp,
                    history_len: hl,
                    pruning_threshold: pt,
                }),
                events_seen,
                elapsed_ms: elapsed.as_millis(),
            };
            let line = serde_json::to_string(&record).context("serializing interaction record")?;
            writeln!(log_writer, "{}", line).context("writing interaction log")?;
            log_writer.flush().context("flushing interaction log")?;
        }

        let prune_marker = if pruning_fired { "✂" } else { " " };

        println!(
            "{:>4}  {:<28}  {:>7}ms{} {:>7}  {:>7}  {:>6}  {:>5}  {:>7}  {:>6}MiB",
            turn.id,
            label_short,
            elapsed.as_millis(),
            prune_marker,
            tok_before,
            tok_after,
            pruned,
            h_len,
            response_chars,
            rss_mib,
        );

        if pruning_fired {
            println!(
                "       ✂ PRUNING FIRED: {} msgs removed, tokens {} → {} (threshold {})",
                pruned, tok_before, tok_after, threshold
            );
        }

        all_metrics.push(TurnMetrics {
            id: turn.id,
            label: turn.label.clone(),
            elapsed_ms: elapsed.as_millis(),
            tokens_before_turn: tok_before,
            tokens_after_prune: tok_after,
            messages_pruned: pruned,
            history_len: h_len,
            pruning_threshold: threshold,
            response_chars,
            pruning_fired,
        });
    }

    // ── Summary ───────────────────────────────────────────────────────────────
    let total_elapsed = bench_start.elapsed();
    let prune_events: Vec<&TurnMetrics> = all_metrics.iter().filter(|m| m.pruning_fired).collect();
    let total_pruned_msgs: usize = all_metrics.iter().map(|m| m.messages_pruned).sum();
    let max_tokens = all_metrics.iter().map(|m| m.tokens_before_turn).max().unwrap_or(0);
    let avg_elapsed_ms = if all_metrics.is_empty() {
        0
    } else {
        all_metrics.iter().map(|m| m.elapsed_ms).sum::<u128>() / all_metrics.len() as u128
    };

    println!();
    println!("{}", "═".repeat(100));
    println!("  BENCH SUMMARY");
    println!("{}", "─".repeat(100));
    println!("  Total turns         : {}", all_metrics.len());
    println!("  Total wall time     : {:.1}s", total_elapsed.as_secs_f64());
    println!("  Avg turn latency    : {}ms", avg_elapsed_ms);
    println!("  Peak token estimate : {} tokens", max_tokens);
    println!(
        "  Pruning events      : {} (turns: {})",
        prune_events.len(),
        prune_events
            .iter()
            .map(|m| m.id.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("  Total msgs pruned   : {}", total_pruned_msgs);
    println!("  Final RSS           : {} MiB", rss_kib() / 1024);

    if prune_events.is_empty() && !all_metrics.is_empty() {
        println!();
        println!("  ⚠  No pruning events observed.");
        println!(
            "     Peak tokens ({}) did not exceed threshold (~{}).",
            max_tokens,
            bench.meta.pruning_threshold_approx
        );
        println!("     Add more turns or longer prompts to stress the context window.");
    } else if !prune_events.is_empty() {
        println!();
        println!("  ✓ Context-window pruning is working correctly.");
    }

    println!("{}", "═".repeat(100));

    Ok(())
}
