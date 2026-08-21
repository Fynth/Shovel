#![cfg_attr(
    all(feature = "bundle", target_os = "windows"),
    windows_subsystem = "windows"
)]

use acp_core::{
    EmbeddedDeepSeekAgentConfig, EmbeddedOllamaAgentConfig, run_embedded_deepseek_agent,
    run_embedded_ollama_agent,
};
use dioxus::{
    LaunchBuilder,
    desktop::{
        Config, LogicalSize, WindowBuilder,
        tao::{event_loop::EventLoopBuilder, window::Icon as TaoIcon},
    },
    prelude::*,
};
use rfd::{MessageButtons, MessageDialog, MessageLevel};
use std::{
    backtrace::Backtrace,
    fs,
    panic::{self, PanicHookInfo},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use ui::App as UiApp;

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
use dioxus::desktop::tao::platform::unix::EventLoopBuilderExtUnix;
#[cfg(target_os = "windows")]
use dioxus::desktop::tao::platform::windows::WindowBuilderExtWindows;

const APP_ICON_RGBA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/app_icon.rgba"));
const APP_CSS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/app.css"));

fn main() {
    // Ensure crash backtraces are always captured, even without RUST_BACKTRACE=1.
    // SAFETY: called at program entry, before any threads are spawned.
    unsafe {
        std::env::set_var("RUST_BACKTRACE", "full");
    }

    install_crash_reporter();

    // Handle embedded ACP agent mode: `shovel acp-agent deepseek|ollama ...`
    // This is used when the desktop app spawns child processes for AI agents.
    if let Some(result) = try_run_embedded_acp_agent() {
        if let Err(err) = result {
            eprintln!("{err}");
            std::process::exit(1);
        }
        return;
    }

    // Do not wrap `launch_app` in `panic::catch_unwind`: the crash-reporter hook
    // installed above already logs panics to disk and shows a native error
    // dialog. Catching here would swallow the unwind and prevent the default
    // panic handler from running normally.
    launch_app();
}

fn launch_app() {
    let mut event_loop_builder = EventLoopBuilder::with_user_event();
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    event_loop_builder.with_app_id("dev.shovel.app");
    let event_loop = event_loop_builder.build();

    LaunchBuilder::desktop()
        .with_cfg(
            Config::new()
                .with_event_loop(event_loop)
                .with_menu(None)
                .with_disable_context_menu(true)
                .with_disable_drag_drop_handler(true)
                // Prefer the native Wayland DMA-BUF path instead of forcing an X11 fallback.
                // This is the only practical GPU-backed improvement available in the current
                // Dioxus desktop/webview renderer without rewriting the app around WGPU/Freya.
                .with_disable_dma_buf_on_wayland(should_disable_wayland_dma_buf())
                .with_window(main_window_builder()),
        )
        .launch(Root);
}

fn main_window_builder() -> WindowBuilder {
    let window = WindowBuilder::new()
        .with_title("Shovel")
        .with_inner_size(LogicalSize::new(1440.0, 920.0))
        .with_min_inner_size(LogicalSize::new(720.0, 480.0))
        .with_always_on_top(false)
        .with_resizable(true)
        .with_decorations(false)
        .with_window_icon(Some(load_app_icon()));

    #[cfg(target_os = "windows")]
    let window = window.with_taskbar_icon(Some(load_app_icon()));

    window
}

fn env_flag(name: &str) -> bool {
    matches!(
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_ascii_lowercase()),
        Some(value)
            if matches!(value.as_str(), "1" | "true" | "yes" | "on")
    )
}

fn should_disable_wayland_dma_buf() -> bool {
    #[cfg(target_os = "linux")]
    {
        env_flag("SHOVEL_DISABLE_WAYLAND_GPU")
    }

    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

fn load_app_icon() -> TaoIcon {
    let width = env!("SHOVEL_ICON_WIDTH")
        .parse::<u32>()
        .expect("invalid icon width");
    let height = env!("SHOVEL_ICON_HEIGHT")
        .parse::<u32>()
        .expect("invalid icon height");

    TaoIcon::from_rgba(APP_ICON_RGBA.to_vec(), width, height).expect("failed to create app icon")
}

#[component]
fn Root() -> Element {
    rsx! {
        document::Style {
            "{APP_CSS}"
        }
        UiApp {}
    }
}

fn install_crash_reporter() {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let report = build_panic_report(info);
        let log_path = write_crash_report(&report);

        default_hook(info);
        eprintln!("{report}");

        let description = match log_path {
            Some(path) => format!("{report}\n\nCrash log:\n{}", path.display()),
            None => report,
        };

        show_error_dialog("Shovel failed to start", &description);
    }));
}

fn build_panic_report(info: &PanicHookInfo<'_>) -> String {
    let location = info
        .location()
        .map(|location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        })
        .unwrap_or_else(|| "unknown location".to_string());
    let backtrace = Backtrace::force_capture();

    format!(
        "Shovel panicked.\n\nMessage: {}\nLocation: {location}\n\nBacktrace:\n{backtrace}",
        panic_message(info)
    )
}

fn panic_message(info: &PanicHookInfo<'_>) -> String {
    if let Some(message) = info.payload().downcast_ref::<&'static str>() {
        (*message).to_string()
    } else if let Some(message) = info.payload().downcast_ref::<String>() {
        message.clone()
    } else {
        "panic payload is not a string".to_string()
    }
}

fn write_crash_report(report: &str) -> Option<PathBuf> {
    let mut log_dir = std::env::temp_dir();
    log_dir.push("shovel");
    fs::create_dir_all(&log_dir).ok()?;

    // Use the process start time as fallback, so concurrent panics produce
    // distinct log file names.
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_else(|_| {
            // System clock is before 1970; fall back to a pseudo-random suffix.
            std::process::id() as u64
        });
    let path = log_dir.join(format!("crash-{timestamp}.log"));
    fs::write(&path, sanitize_crash_report(report)).ok()?;
    Some(path)
}

/// Redact sensitive credentials from a crash report before writing it to disk.
///
/// Strips:
/// - `password=...` key-value pairs
/// - `://user:password@host` URL credentials
fn sanitize_crash_report(report: &str) -> String {
    // Replace `password=<value>` (case-insensitive key, captures until whitespace/quote/end)
    let re_password =
        regex::Regex::new(r"(?i)(password\s*=\s*)(\S+)").expect("failed to compile password regex");
    let report = re_password.replace_all(report, "${1}***REDACTED***");

    // Replace `://user:secret@host` patterns in URLs
    let re_url_creds = regex::Regex::new(r"(://[^:?\s@]+:)([^@?\s]+)(@)")
        .expect("failed to compile URL creds regex");
    let report = re_url_creds.replace_all(&report, "${1}***REDACTED***${3}");

    report.into_owned()
}

fn show_error_dialog(title: &str, description: &str) {
    let _ = MessageDialog::new()
        .set_title(title)
        .set_description(description)
        .set_level(MessageLevel::Error)
        .set_buttons(MessageButtons::Ok)
        .show();
}

// ── Embedded ACP agent mode ───────────────────────────────────────
// When the desktop app spawns child processes for AI agents (DeepSeek,
// Ollama), the child process re-enters main() with "acp-agent" args.
// We detect this early and run the agent in a headless tokio runtime
// instead of launching the Dioxus desktop window.

fn try_run_embedded_acp_agent() -> Option<Result<(), String>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        return None;
    }

    if args.first().map(String::as_str) != Some("acp-agent") {
        return None;
    }

    Some(match args.get(1).map(String::as_str) {
        Some("deepseek") => {
            parse_deepseek_agent_args(&args[2..]).and_then(run_embedded_deepseek_agent)
        }
        Some("ollama") => parse_ollama_agent_args(&args[2..]).and_then(run_embedded_ollama_agent),
        Some(other) => Err(format!("Unsupported embedded ACP agent `{other}`")),
        None => Err("Missing embedded ACP agent name".to_string()),
    })
}

fn parse_deepseek_agent_args(args: &[String]) -> Result<EmbeddedDeepSeekAgentConfig, String> {
    let mut base_url = None;
    let mut model = None;
    let mut api_key = std::env::var("DEEPSEEK_API_KEY").ok();
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
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .ok_or_else(|| "Missing `--model` for embedded ACP DeepSeek agent".to_string())?;
    let api_key = api_key
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
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
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .ok_or_else(|| "Missing `--model` for embedded ACP Ollama agent".to_string())?;

    Ok(EmbeddedOllamaAgentConfig {
        base_url: base_url.unwrap_or_else(|| "http://localhost:11434/api".to_string()),
        model,
        api_key: api_key.filter(|v| !v.trim().is_empty()),
    })
}
