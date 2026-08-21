use models::{AcpLaunchRequest, AcpRegistryAgent, AgentSpecialist};
use reqwest::header::{ACCEPT, USER_AGENT};
use serde::Deserialize;
use std::{
    collections::HashMap,
    io::Cursor,
    path::{Path, PathBuf},
    time::Duration,
};

const ACP_REGISTRY_URL: &str =
    "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";
const ACP_REGISTRY_ACCEPT: &str = "application/json";
const ACP_REGISTRY_USER_AGENT: &str = concat!("shovel/", env!("CARGO_PKG_VERSION"));
const ACP_REGISTRY_CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const ACP_REGISTRY_REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const ACP_ARCHIVE_CONNECT_TIMEOUT: Duration = Duration::from_secs(6);
const ACP_ARCHIVE_REQUEST_TIMEOUT: Duration = Duration::from_secs(180);

#[allow(dead_code)]
struct SpecialistCapability {
    keywords: &'static [&'static str],
    description: &'static str,
    example_queries: &'static [&'static str],
}

const SPECIALIST_REGISTRY: &[(&AgentSpecialist, SpecialistCapability)] = &[
    (
        &AgentSpecialist::SqlExpert,
        SpecialistCapability {
            keywords: &[
                "SELECT", "INSERT", "UPDATE", "DELETE", "FROM", "WHERE", "JOIN", "GROUP BY",
                "ORDER BY",
            ],
            description: "SQL query optimization and generation expert",
            example_queries: &["Optimize this query", "How do I join these tables?"],
        },
    ),
    (
        &AgentSpecialist::DataAnalyst,
        SpecialistCapability {
            keywords: &[
                "analyze",
                "trend",
                "average",
                "sum",
                "count",
                "group by",
                "chart",
                "graph",
                "statistics",
            ],
            description: "Data analysis and visualization expert",
            example_queries: &["Show me sales trends", "What's the average order value?"],
        },
    ),
    (
        &AgentSpecialist::SchemaArchitect,
        SpecialistCapability {
            keywords: &[
                "create table",
                "alter table",
                "migration",
                "constraint",
                "index",
                "schema",
                "design",
            ],
            description: "Database schema design and migration expert",
            example_queries: &["Design a table for users", "How do I add an index?"],
        },
    ),
];

/// Match an intent string against specialist keywords and return the best match.
pub fn get_specialist_for_intent(intent: &str) -> Option<AgentSpecialist> {
    let intent_lower = intent.to_lowercase();
    let mut best_match: Option<AgentSpecialist> = None;
    let mut best_score: usize = 0;

    for (specialist, capability) in SPECIALIST_REGISTRY {
        let score = capability
            .keywords
            .iter()
            .filter(|kw| intent_lower.contains(&kw.to_lowercase()))
            .count();
        if score > best_score {
            best_score = score;
            best_match = Some(*(*specialist));
        }
    }

    best_match
}

#[cfg(test)]
mod specialist_tests {
    use super::*;

    #[test]
    fn test_sql_expert_intent() {
        assert_eq!(
            get_specialist_for_intent("How do I join these tables?"),
            Some(AgentSpecialist::SqlExpert)
        );
        assert_eq!(
            get_specialist_for_intent("SELECT * FROM users WHERE id = 1"),
            Some(AgentSpecialist::SqlExpert)
        );
    }

    #[test]
    fn test_data_analyst_intent() {
        assert_eq!(
            get_specialist_for_intent("Show me sales trends"),
            Some(AgentSpecialist::DataAnalyst)
        );
        assert_eq!(
            get_specialist_for_intent("What's the average order value?"),
            Some(AgentSpecialist::DataAnalyst)
        );
    }

    #[test]
    fn test_schema_architect_intent() {
        assert_eq!(
            get_specialist_for_intent("Design a table for users"),
            Some(AgentSpecialist::SchemaArchitect)
        );
        assert_eq!(
            get_specialist_for_intent("How do I add an index?"),
            Some(AgentSpecialist::SchemaArchitect)
        );
    }

    #[test]
    fn test_no_match() {
        assert_eq!(get_specialist_for_intent("Hello world"), None);
    }
}

#[derive(Debug, Deserialize)]
struct RegistryDocument {
    agents: Vec<RegistryAgentRecord>,
}

#[derive(Debug, Deserialize)]
struct RegistryAgentRecord {
    id: String,
    name: String,
    version: String,
    description: String,
    distribution: RegistryDistribution,
}

#[derive(Debug, Deserialize)]
struct RegistryDistribution {
    #[serde(default)]
    binary: std::collections::HashMap<String, RegistryBinaryTarget>,
}

#[derive(Clone, Debug, Deserialize)]
struct RegistryBinaryTarget {
    archive: String,
    cmd: String,
    #[serde(default)]
    args: Vec<String>,
}

pub async fn load_acp_registry_agents() -> Result<Vec<AcpRegistryAgent>, String> {
    let registry = merge_with_built_in_registry(fetch_registry().await?);
    let platform_key = current_platform_key()?;

    let mut agents = registry
        .agents
        .into_iter()
        .filter_map(|agent| {
            let target = agent.distribution.binary.get(&platform_key)?;
            let install_root = install_root(&agent.id, &agent.version).ok()?;
            let command_path = resolve_installed_command(&install_root, &target.cmd);
            Some(AcpRegistryAgent {
                id: agent.id,
                name: agent.name,
                version: agent.version,
                description: agent.description,
                installed: command_path.is_some(),
            })
        })
        .collect::<Vec<_>>();

    agents.sort_by(|left, right| {
        registry_agent_priority(&left.id)
            .cmp(&registry_agent_priority(&right.id))
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(agents)
}

fn registry_agent_priority(agent_id: &str) -> usize {
    match agent_id {
        "codex-acp" => 0,
        "opencode" => 1,
        _ => 2,
    }
}

pub async fn install_acp_registry_agent(
    agent_id: String,
    cwd: String,
) -> Result<AcpLaunchRequest, String> {
    let registry = merge_with_built_in_registry(fetch_registry().await?);
    let platform_key = current_platform_key()?;
    let agent = registry
        .agents
        .into_iter()
        .find(|agent| agent.id == agent_id)
        .ok_or_else(|| format!("ACP registry agent `{agent_id}` not found"))?;
    let target = agent
        .distribution
        .binary
        .get(&platform_key)
        .cloned()
        .ok_or_else(|| {
            format!(
                "ACP agent `{}` is not available for {platform_key}",
                agent.id
            )
        })?;

    let install_root = install_root(&agent.id, &agent.version)?;
    let command_path = match resolve_installed_command(&install_root, &target.cmd) {
        Some(command_path) => command_path,
        None => {
            download_and_extract(&target.archive, &install_root).await?;
            let command_path =
                resolve_installed_command(&install_root, &target.cmd).ok_or_else(|| {
                    format!(
                        "Installed ACP agent `{}` but command was not found",
                        agent.id
                    )
                })?;
            ensure_executable(&command_path)?;
            command_path
        }
    };

    Ok(AcpLaunchRequest {
        command: command_path.display().to_string(),
        args: join_args(&target.args),
        cwd,
        env: Vec::new(),
    })
}

async fn fetch_registry() -> Result<RegistryDocument, String> {
    let client = reqwest::Client::builder()
        .connect_timeout(ACP_REGISTRY_CONNECT_TIMEOUT)
        .timeout(ACP_REGISTRY_REQUEST_TIMEOUT)
        .build()
        .map_err(|err| format!("Failed to build ACP registry HTTP client: {err}"))?;

    let response = client
        .get(ACP_REGISTRY_URL)
        .header(USER_AGENT, ACP_REGISTRY_USER_AGENT)
        .header(ACCEPT, ACP_REGISTRY_ACCEPT)
        .send()
        .await;

    let response = match response {
        Ok(response) => response,
        Err(err) => {
            return fallback_registry_document(format!("Failed to fetch ACP registry: {err}"));
        }
    };

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("Failed to read ACP registry response: {err}"))?;

    if !status.is_success() {
        return fallback_registry_document(format!(
            "ACP registry returned HTTP {}",
            status.as_u16()
        ));
    }

    serde_json::from_str::<RegistryDocument>(&body).or_else(|err| {
        fallback_registry_document(format!(
            "Failed to parse ACP registry: {err}. Response starts with: {}",
            response_excerpt(&body)
        ))
    })
}

fn fallback_registry_document(reason: String) -> Result<RegistryDocument, String> {
    let fallback = built_in_registry();
    if fallback.agents.is_empty() {
        Err(reason)
    } else {
        Ok(fallback)
    }
}

fn merge_with_built_in_registry(remote: RegistryDocument) -> RegistryDocument {
    // Keep built-ins authoritative for curated agents we actively support.
    // Remote registry entries can add new agents, but should not silently
    // change the versions or launch targets of built-in records.
    let mut merged: HashMap<String, RegistryAgentRecord> = built_in_registry()
        .agents
        .into_iter()
        .map(|agent| (agent.id.clone(), agent))
        .collect();

    for agent in remote.agents {
        merged.entry(agent.id.clone()).or_insert(agent);
    }

    RegistryDocument {
        agents: merged.into_values().collect(),
    }
}

fn response_excerpt(body: &str) -> String {
    let trimmed = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let excerpt = trimmed.chars().take(120).collect::<String>();
    if excerpt.is_empty() {
        "<empty response>".to_string()
    } else if trimmed.chars().count() > 120 {
        format!("{excerpt}...")
    } else {
        excerpt
    }
}

fn built_in_registry() -> RegistryDocument {
    RegistryDocument {
        agents: vec![built_in_codex_agent(), built_in_opencode_agent()],
    }
}

fn built_in_codex_agent() -> RegistryAgentRecord {
    RegistryAgentRecord {
        id: "codex-acp".to_string(),
        name: "Codex CLI".to_string(),
        version: "0.10.0".to_string(),
        description: "ACP adapter for OpenAI's coding assistant".to_string(),
        distribution: RegistryDistribution {
            binary: HashMap::from([
                (
                    "darwin-aarch64".to_string(),
                    registry_binary_target(
                        "https://github.com/zed-industries/codex-acp/releases/download/v0.10.0/codex-acp-0.10.0-aarch64-apple-darwin.tar.gz",
                        "./codex-acp",
                        &[],
                    ),
                ),
                (
                    "darwin-x86_64".to_string(),
                    registry_binary_target(
                        "https://github.com/zed-industries/codex-acp/releases/download/v0.10.0/codex-acp-0.10.0-x86_64-apple-darwin.tar.gz",
                        "./codex-acp",
                        &[],
                    ),
                ),
                (
                    "linux-aarch64".to_string(),
                    registry_binary_target(
                        "https://github.com/zed-industries/codex-acp/releases/download/v0.10.0/codex-acp-0.10.0-aarch64-unknown-linux-gnu.tar.gz",
                        "./codex-acp",
                        &[],
                    ),
                ),
                (
                    "linux-x86_64".to_string(),
                    registry_binary_target(
                        "https://github.com/zed-industries/codex-acp/releases/download/v0.10.0/codex-acp-0.10.0-x86_64-unknown-linux-gnu.tar.gz",
                        "./codex-acp",
                        &[],
                    ),
                ),
                (
                    "windows-aarch64".to_string(),
                    registry_binary_target(
                        "https://github.com/zed-industries/codex-acp/releases/download/v0.10.0/codex-acp-0.10.0-aarch64-pc-windows-msvc.zip",
                        "./codex-acp.exe",
                        &[],
                    ),
                ),
                (
                    "windows-x86_64".to_string(),
                    registry_binary_target(
                        "https://github.com/zed-industries/codex-acp/releases/download/v0.10.0/codex-acp-0.10.0-x86_64-pc-windows-msvc.zip",
                        "./codex-acp.exe",
                        &[],
                    ),
                ),
            ]),
        },
    }
}

fn built_in_opencode_agent() -> RegistryAgentRecord {
    RegistryAgentRecord {
        id: "opencode".to_string(),
        name: "OpenCode".to_string(),
        version: "1.2.27".to_string(),
        description: "The open source coding agent".to_string(),
        distribution: RegistryDistribution {
            binary: HashMap::from([
                (
                    "darwin-aarch64".to_string(),
                    registry_binary_target(
                        "https://github.com/anomalyco/opencode/releases/download/v1.2.27/opencode-darwin-arm64.zip",
                        "./opencode",
                        &["acp"],
                    ),
                ),
                (
                    "darwin-x86_64".to_string(),
                    registry_binary_target(
                        "https://github.com/anomalyco/opencode/releases/download/v1.2.27/opencode-darwin-x64.zip",
                        "./opencode",
                        &["acp"],
                    ),
                ),
                (
                    "linux-aarch64".to_string(),
                    registry_binary_target(
                        "https://github.com/anomalyco/opencode/releases/download/v1.2.27/opencode-linux-arm64.tar.gz",
                        "./opencode",
                        &["acp"],
                    ),
                ),
                (
                    "linux-x86_64".to_string(),
                    registry_binary_target(
                        "https://github.com/anomalyco/opencode/releases/download/v1.2.27/opencode-linux-x64.tar.gz",
                        "./opencode",
                        &["acp"],
                    ),
                ),
                (
                    "windows-x86_64".to_string(),
                    registry_binary_target(
                        "https://github.com/anomalyco/opencode/releases/download/v1.2.27/opencode-windows-x64.zip",
                        "./opencode.exe",
                        &["acp"],
                    ),
                ),
            ]),
        },
    }
}

fn registry_binary_target(archive: &str, cmd: &str, args: &[&str]) -> RegistryBinaryTarget {
    RegistryBinaryTarget {
        archive: archive.to_string(),
        cmd: cmd.to_string(),
        args: args.iter().map(|arg| (*arg).to_string()).collect(),
    }
}

fn current_platform_key() -> Result<String, String> {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "windows",
        other => return Err(format!("ACP registry does not support OS `{other}`")),
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "aarch64",
        "x86_64" => "x86_64",
        other => {
            return Err(format!(
                "ACP registry does not support architecture `{other}`"
            ));
        }
    };
    Ok(format!("{os}-{arch}"))
}

fn install_root(agent_id: &str, version: &str) -> Result<PathBuf, String> {
    let base_dir = dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| std::env::temp_dir().join("shovel"));
    Ok(base_dir
        .join("shovel")
        .join("acp")
        .join("registry")
        .join(agent_id)
        .join(version))
}

fn resolve_installed_command(install_root: &Path, cmd: &str) -> Option<PathBuf> {
    let relative = cmd.strip_prefix("./").unwrap_or(cmd);
    let direct = install_root.join(relative);
    if direct.is_file() {
        return Some(direct);
    }

    let file_name = Path::new(relative).file_name()?.to_owned();
    walkdir::WalkDir::new(install_root)
        .into_iter()
        .filter_map(Result::ok)
        .find_map(|entry| {
            if entry.file_type().is_file() && entry.file_name() == file_name {
                Some(entry.into_path())
            } else {
                None
            }
        })
}

async fn download_and_extract(archive_url: &str, install_root: &Path) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .connect_timeout(ACP_ARCHIVE_CONNECT_TIMEOUT)
        .timeout(ACP_ARCHIVE_REQUEST_TIMEOUT)
        .build()
        .map_err(|err| format!("Failed to build ACP download HTTP client: {err}"))?;

    let bytes = client
        .get(archive_url)
        .header(USER_AGENT, ACP_REGISTRY_USER_AGENT)
        .header(ACCEPT, "*/*")
        .send()
        .await
        .map_err(|err| format!("Failed to download ACP agent archive: {err}"))?
        .error_for_status()
        .map_err(|err| format!("ACP agent download failed: {err}"))?
        .bytes()
        .await
        .map_err(|err| format!("Failed to read ACP agent archive: {err}"))?;

    let install_root = install_root.to_path_buf();
    let archive_url = archive_url.to_string();
    tokio::task::spawn_blocking(move || extract_archive(&archive_url, &bytes, &install_root))
        .await
        .map_err(|err| format!("ACP agent installation task failed: {err}"))?
}

fn extract_archive(archive_url: &str, bytes: &[u8], install_root: &Path) -> Result<(), String> {
    if install_root.exists() {
        std::fs::remove_dir_all(install_root)
            .map_err(|err| format!("Failed to reset ACP install directory: {err}"))?;
    }
    std::fs::create_dir_all(install_root)
        .map_err(|err| format!("Failed to create ACP install directory: {err}"))?;

    if archive_url.ends_with(".tar.gz") {
        let decoder = flate2::read::GzDecoder::new(Cursor::new(bytes));
        let mut archive = tar::Archive::new(decoder);
        for entry in archive
            .entries()
            .map_err(|err| format!("Failed to read tar archive: {err}"))?
        {
            let mut entry = entry.map_err(|err| format!("Failed to read tar entry: {err}"))?;
            entry
                .unpack_in(install_root)
                .map_err(|err| format!("Failed to extract tar entry: {err}"))?;
        }
        return Ok(());
    }

    if archive_url.ends_with(".zip") {
        let reader = Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(reader)
            .map_err(|err| format!("Failed to read zip archive: {err}"))?;

        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .map_err(|err| format!("Failed to read zip entry: {err}"))?;
            let Some(path) = entry.enclosed_name().map(|path| path.to_path_buf()) else {
                continue;
            };
            let target = install_root.join(path);

            if entry.is_dir() {
                std::fs::create_dir_all(&target)
                    .map_err(|err| format!("Failed to create zip directory: {err}"))?;
                continue;
            }

            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|err| format!("Failed to create zip parent directory: {err}"))?;
            }

            let mut file = std::fs::File::create(&target)
                .map_err(|err| format!("Failed to create extracted file: {err}"))?;
            std::io::copy(&mut entry, &mut file)
                .map_err(|err| format!("Failed to extract zip file: {err}"))?;
        }
        return Ok(());
    }

    Err(format!("Unsupported ACP archive format: {archive_url}"))
}

fn join_args(args: &[String]) -> String {
    args.iter()
        .map(|arg| shell_escape(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::{
        RegistryAgentRecord, RegistryBinaryTarget, RegistryDistribution, RegistryDocument,
        built_in_registry, merge_with_built_in_registry, response_excerpt,
    };
    use std::collections::HashMap;

    #[test]
    fn built_in_registry_contains_known_agents() {
        let registry = built_in_registry();
        assert!(registry.agents.iter().any(|agent| agent.id == "opencode"));
        assert!(registry.agents.iter().any(|agent| agent.id == "codex-acp"));
    }

    #[test]
    fn response_excerpt_flattens_html_like_bodies() {
        let excerpt = response_excerpt("<html>\n  <body>blocked</body>\n</html>");
        assert_eq!(excerpt, "<html> <body>blocked</body> </html>");
    }

    #[test]
    fn merge_keeps_built_in_agent_versions_pinned() {
        let merged = merge_with_built_in_registry(RegistryDocument {
            agents: vec![
                RegistryAgentRecord {
                    id: "opencode".to_string(),
                    name: "OpenCode".to_string(),
                    version: "9.9.9".to_string(),
                    description: "Remote override".to_string(),
                    distribution: RegistryDistribution {
                        binary: HashMap::from([(
                            "linux-x86_64".to_string(),
                            RegistryBinaryTarget {
                                archive: "https://example.com/opencode.tar.gz".to_string(),
                                cmd: "./broken-opencode".to_string(),
                                args: vec!["acp".to_string()],
                            },
                        )]),
                    },
                },
                RegistryAgentRecord {
                    id: "custom-agent".to_string(),
                    name: "Custom".to_string(),
                    version: "1.0.0".to_string(),
                    description: "Remote only".to_string(),
                    distribution: RegistryDistribution {
                        binary: HashMap::new(),
                    },
                },
            ],
        });

        let opencode = merged
            .agents
            .iter()
            .find(|agent| agent.id == "opencode")
            .expect("built-in opencode agent should remain present");
        assert_eq!(opencode.version, "1.2.27");

        let custom = merged
            .agents
            .iter()
            .find(|agent| agent.id == "custom-agent")
            .expect("remote-only agent should still be included");
        assert_eq!(custom.version, "1.0.0");
    }
}

fn shell_escape(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }
    if arg
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '/' | '.'))
    {
        return arg.to_string();
    }

    format!("'{}'", arg.replace('\'', "'\"'\"'"))
}

fn ensure_executable(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let metadata = std::fs::metadata(path)
            .map_err(|err| format!("Failed to inspect installed ACP command: {err}"))?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions)
            .map_err(|err| format!("Failed to mark ACP command executable: {err}"))?;
    }

    Ok(())
}
