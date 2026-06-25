use super::BrowserAction;
use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

const RUNNER_JS: &str = include_str!("playwright_runner.mjs");

#[derive(Default)]
pub struct PlaywrightBrowserState {
    daemon: Option<PlaywrightDaemon>,
    next_id: u64,
}

struct PlaywrightDaemon {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

#[derive(Debug, Deserialize)]
struct PlaywrightResponse {
    success: bool,
    #[serde(default)]
    data: Option<Value>,
    #[serde(default)]
    error: Option<String>,
}

impl Drop for PlaywrightDaemon {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

impl PlaywrightBrowserState {
    pub async fn is_available() -> bool {
        let mut command = node_command();
        command
            .args([
                "-e",
                "try { require('playwright'); process.exit(0); } catch (_) { try { require('@playwright/test'); process.exit(0); } catch (_) { process.exit(1); } }",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        apply_node_cwd(&mut command);

        match command.status().await {
            Ok(status) => status.success(),
            Err(_) => false,
        }
    }

    pub async fn execute_action(&mut self, action: BrowserAction, headless: bool) -> Result<Value> {
        let args = action_to_args(action);
        self.execute_args(args, headless).await
    }

    async fn execute_args(&mut self, args: Value, headless: bool) -> Result<Value> {
        if self.daemon.is_none() {
            tracing::debug!("[browser::playwright] starting playwright backend daemon");
            self.daemon = Some(start_daemon(headless).await?);
        }

        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);

        let request = json!({
            "id": id,
            "args": args,
        });
        let line = serde_json::to_vec(&request).context("Failed to encode Playwright request")?;

        let daemon = self.daemon.as_mut().expect("daemon just initialized");
        if let Err(error) = write_request(daemon, &line).await {
            tracing::debug!(
                error = %error,
                "[browser::playwright] daemon write failed; restarting once"
            );
            self.daemon = Some(start_daemon(headless).await?);
            let daemon = self.daemon.as_mut().expect("daemon restarted");
            write_request(daemon, &line).await?;
        }

        let daemon = self.daemon.as_mut().expect("daemon available");
        let response = read_response(daemon)
            .await
            .context("Failed to read Playwright response")?;

        if response.success {
            Ok(response.data.unwrap_or_else(|| json!({ "ok": true })))
        } else {
            anyhow::bail!(
                "{}",
                response
                    .error
                    .unwrap_or_else(|| "Playwright backend failed".to_string())
            )
        }
    }
}

async fn write_request(daemon: &mut PlaywrightDaemon, line: &[u8]) -> Result<()> {
    daemon
        .stdin
        .write_all(line)
        .await
        .context("Failed to write Playwright request")?;
    daemon
        .stdin
        .write_all(b"\n")
        .await
        .context("Failed to terminate Playwright request")?;
    daemon
        .stdin
        .flush()
        .await
        .context("Failed to flush Playwright request")?;
    Ok(())
}

async fn read_response(daemon: &mut PlaywrightDaemon) -> Result<PlaywrightResponse> {
    let mut line = String::new();
    let read = daemon
        .stdout
        .read_line(&mut line)
        .await
        .context("Failed to read Playwright stdout")?;
    if read == 0 {
        anyhow::bail!("Playwright daemon exited without a response");
    }
    serde_json::from_str(&line).context("Playwright daemon returned invalid JSON")
}

async fn start_daemon(headless: bool) -> Result<PlaywrightDaemon> {
    let mut command = node_command();
    command
        .arg("-e")
        .arg(RUNNER_JS)
        .env(
            "OPENHUMAN_PLAYWRIGHT_HEADLESS",
            if headless { "1" } else { "0" },
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_node_cwd(&mut command);

    let mut child = command.spawn().context(
        "Failed to start Playwright backend. Ensure Node.js and the Playwright package are installed.",
    )?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("Playwright daemon stdin unavailable"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("Playwright daemon stdout unavailable"))?;

    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!("[browser::playwright] stderr: {line}");
            }
        });
    }

    Ok(PlaywrightDaemon {
        child,
        stdin,
        stdout: BufReader::new(stdout),
    })
}

fn node_command() -> Command {
    let binary = std::env::var("OPENHUMAN_PLAYWRIGHT_NODE").unwrap_or_else(|_| "node".to_string());
    Command::new(binary)
}

fn apply_node_cwd(command: &mut Command) {
    if let Some(cwd) = playwright_node_cwd() {
        command.current_dir(cwd);
    }
}

fn playwright_node_cwd() -> Option<PathBuf> {
    if let Ok(raw) = std::env::var("OPENHUMAN_PLAYWRIGHT_CWD") {
        let path = PathBuf::from(raw);
        if path.exists() {
            return Some(path);
        }
    }

    let app = Path::new("app");
    if app.join("node_modules").exists() {
        return Some(app.to_path_buf());
    }

    None
}

fn action_to_args(action: BrowserAction) -> Value {
    match action {
        BrowserAction::Open { url } => json!({ "action": "open", "url": url }),
        BrowserAction::Snapshot {
            interactive_only,
            compact,
            depth,
        } => json!({
            "action": "snapshot",
            "interactive_only": interactive_only,
            "compact": compact,
            "depth": depth,
        }),
        BrowserAction::Click { selector } => json!({ "action": "click", "selector": selector }),
        BrowserAction::Fill { selector, value } => {
            json!({ "action": "fill", "selector": selector, "value": value })
        }
        BrowserAction::Type { selector, text } => {
            json!({ "action": "type", "selector": selector, "text": text })
        }
        BrowserAction::GetText { selector } => {
            json!({ "action": "get_text", "selector": selector })
        }
        BrowserAction::GetTitle => json!({ "action": "get_title" }),
        BrowserAction::GetUrl => json!({ "action": "get_url" }),
        BrowserAction::Screenshot { path, full_page } => {
            json!({ "action": "screenshot", "path": path, "full_page": full_page })
        }
        BrowserAction::Wait { selector, ms, text } => {
            json!({ "action": "wait", "selector": selector, "ms": ms, "text": text })
        }
        BrowserAction::Press { key } => json!({ "action": "press", "key": key }),
        BrowserAction::Hover { selector } => json!({ "action": "hover", "selector": selector }),
        BrowserAction::Scroll { direction, pixels } => {
            json!({ "action": "scroll", "direction": direction, "pixels": pixels })
        }
        BrowserAction::IsVisible { selector } => {
            json!({ "action": "is_visible", "selector": selector })
        }
        BrowserAction::Close => json!({ "action": "close" }),
        BrowserAction::Find {
            by,
            value,
            action,
            fill_value,
        } => json!({
            "action": "find",
            "by": by,
            "value": value,
            "find_action": action,
            "fill_value": fill_value,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_to_args_preserves_find_shape() {
        let args = action_to_args(BrowserAction::Find {
            by: "label".into(),
            value: "Email".into(),
            action: "fill".into(),
            fill_value: Some("a@example.com".into()),
        });

        assert_eq!(args["action"], "find");
        assert_eq!(args["find_action"], "fill");
        assert_eq!(args["fill_value"], "a@example.com");
    }
}
