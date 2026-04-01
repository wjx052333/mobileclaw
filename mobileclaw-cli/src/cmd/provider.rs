use anyhow::Result;
use std::path::Path;
use mobileclaw_core::ffi::{ProviderConfigDto, provider_probe};
use mobileclaw_core::llm::provider::{ProviderConfig, ProviderProtocol};
use crate::session::open_secrets;

fn protocol_str(p: &ProviderProtocol) -> &'static str {
    match p {
        ProviderProtocol::Anthropic    => "anthropic",
        ProviderProtocol::OpenAiCompat => "openai_compat",
        ProviderProtocol::Ollama       => "ollama",
    }
}

pub async fn cmd_provider_add(
    data_dir: &Path,
    name: String,
    protocol: String,
    url: String,
    model: String,
    key: Option<String>,
    set_active: bool,
) -> Result<()> {
    let secrets = open_secrets(data_dir).await?;
    let proto = match protocol.as_str() {
        "anthropic"     => ProviderProtocol::Anthropic,
        "openai_compat" => ProviderProtocol::OpenAiCompat,
        "ollama"        => ProviderProtocol::Ollama,
        other           => anyhow::bail!("unknown protocol: {other}  (use: anthropic | openai_compat | ollama)"),
    };
    let cfg = ProviderConfig::new(name.clone(), proto, url, model);
    secrets.provider_save(&cfg, key.as_deref()).await?;
    println!("Saved provider '{}' (id: {})", name, cfg.id);
    if set_active {
        secrets.set_active_provider_id(&cfg.id).await?;
        println!("Set as active provider.");
    }
    Ok(())
}

pub async fn cmd_provider_list(data_dir: &Path) -> Result<()> {
    let secrets = open_secrets(data_dir).await?;
    let active_id = secrets.active_provider_id().await?;
    let list = secrets.provider_list().await?;
    if list.is_empty() {
        println!("No providers configured. Use `mclaw provider add` to add one.");
        return Ok(());
    }
    println!("{:<38} {:<14} {:<20} MODEL", "ID", "PROTOCOL", "NAME");
    println!("{}", "-".repeat(90));
    for p in &list {
        let proto = protocol_str(&p.protocol);
        let active = if active_id.as_deref() == Some(&p.id) { " ✓ active" } else { "" };
        println!("{:<38} {:<14} {:<20} {}{}", p.id, proto, p.name, p.model, active);
    }
    Ok(())
}

pub async fn cmd_provider_set_active(data_dir: &Path, id: String) -> Result<()> {
    let secrets = open_secrets(data_dir).await?;
    secrets.provider_load(&id).await?;
    secrets.set_active_provider_id(&id).await?;
    println!("Active provider set to: {id}");
    Ok(())
}

pub async fn cmd_provider_delete(data_dir: &Path, id: String) -> Result<()> {
    let secrets = open_secrets(data_dir).await?;
    let cfg = secrets.provider_load(&id).await?;
    secrets.provider_delete(&id).await?;
    println!("Deleted provider '{}' ({})", cfg.name, id);
    Ok(())
}

pub async fn cmd_provider_probe(
    data_dir: &Path,
    id: Option<String>,
    protocol: Option<String>,
    url: Option<String>,
    model: Option<String>,
    key: Option<String>,
) -> Result<()> {
    let (dto, api_key): (ProviderConfigDto, Option<String>) = if let Some(ref pid) = id {
        let secrets = open_secrets(data_dir).await?;
        let cfg = secrets.provider_load(pid).await?;
        let k = secrets.provider_api_key(pid).await?;
        let proto = protocol_str(&cfg.protocol);
        (ProviderConfigDto {
            id: cfg.id, name: cfg.name, protocol: proto.into(),
            base_url: cfg.base_url, model: cfg.model, created_at: cfg.created_at,
        }, k)
    } else {
        let p = protocol.as_deref().unwrap_or("openai_compat");
        let u = url.unwrap_or_default();
        let m = model.unwrap_or_default();
        (ProviderConfigDto {
            id: "probe-tmp".into(), name: "probe".into(), protocol: p.into(),
            base_url: u, model: m, created_at: 0,
        }, key)
    };

    println!("Probing {} ({})...", dto.name, dto.protocol);
    let result = provider_probe(dto, api_key).await;
    if result.ok {
        if result.degraded {
            println!("⚠  Reachable ({}ms) — completions unverified, only /models endpoint responded", result.latency_ms);
        } else {
            println!("✓  OK ({}ms) — completion request succeeded", result.latency_ms);
        }
    } else {
        println!("✗  Failed ({}ms): {}", result.latency_ms, result.error.unwrap_or_default());
    }
    Ok(())
}
