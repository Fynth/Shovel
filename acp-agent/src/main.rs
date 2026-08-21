use acp_core::{
    EmbeddedDeepSeekAgentConfig, EmbeddedOllamaAgentConfig, run_embedded_deepseek_agent,
    run_embedded_ollama_agent,
};
use std::env;

fn main() {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        eprintln!("Usage: acp-agent <deepseek|ollama> [flags...]");
        std::process::exit(1);
    }

    let result = match args.first().map(String::as_str) {
        Some("deepseek") => {
            parse_deepseek_agent_args(&args[1..]).and_then(run_embedded_deepseek_agent)
        }
        Some("ollama") => parse_ollama_agent_args(&args[1..]).and_then(run_embedded_ollama_agent),
        Some(other) => Err(format!("Unsupported embedded ACP agent `{other}`")),
        None => Err("Missing embedded ACP agent name".to_string()),
    };

    if let Err(err) = result {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn parse_deepseek_agent_args(args: &[String]) -> Result<EmbeddedDeepSeekAgentConfig, String> {
    let mut base_url = None;
    let mut model = None;
    let mut api_key = env::var("DEEPSEEK_API_KEY").ok();
    let mut thinking_enabled = true;
    let mut reasoning_effort = "medium".to_string();
    let mut index = 0;

    while index < args.len() {
        let flag = &args[index];
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("Missing value for `{flag}`"))?;

        match flag.as_str() {
            "--base-url" => base_url = Some(value.clone()),
            "--model" => model = Some(value.clone()),
            "--api-key" => api_key = Some(value.clone()),
            "--thinking" => {
                thinking_enabled = match value.trim().to_ascii_lowercase().as_str() {
                    "enabled" | "true" | "1" | "yes" => true,
                    "disabled" | "false" | "0" | "no" => false,
                    _ => {
                        return Err("DeepSeek `--thinking` must be enabled or disabled".to_string());
                    }
                }
            }
            "--reasoning-effort" => reasoning_effort = value.clone(),
            other => return Err(format!("Unknown embedded ACP DeepSeek flag `{other}`")),
        }

        index += 2;
    }

    let model = model
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty())
        .ok_or_else(|| "Missing `--model` for embedded ACP DeepSeek agent".to_string())?;
    let api_key = api_key
        .map(|api_key| api_key.trim().to_string())
        .filter(|api_key| !api_key.is_empty())
        .ok_or_else(|| "Missing DeepSeek API key".to_string())?;

    Ok(EmbeddedDeepSeekAgentConfig {
        base_url: base_url.unwrap_or_else(|| "https://api.deepseek.com".to_string()),
        model,
        api_key,
        thinking_enabled,
        reasoning_effort,
    })
}

fn parse_ollama_agent_args(args: &[String]) -> Result<EmbeddedOllamaAgentConfig, String> {
    let mut base_url = None;
    let mut model = None;
    let mut api_key = None;
    let mut index = 0;

    while index < args.len() {
        let flag = &args[index];
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("Missing value for `{flag}`"))?;

        match flag.as_str() {
            "--base-url" => base_url = Some(value.clone()),
            "--model" => model = Some(value.clone()),
            "--api-key" => api_key = Some(value.clone()),
            other => return Err(format!("Unknown embedded ACP Ollama flag `{other}`")),
        }

        index += 2;
    }

    let model = model
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty())
        .ok_or_else(|| "Missing `--model` for embedded ACP Ollama agent".to_string())?;

    Ok(EmbeddedOllamaAgentConfig {
        base_url: base_url.unwrap_or_else(|| "http://localhost:11434/api".to_string()),
        model,
        api_key: api_key.filter(|value| !value.trim().is_empty()),
    })
}
