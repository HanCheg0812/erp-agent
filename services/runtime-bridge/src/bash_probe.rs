//! Temporary, read-only environment probes exposed through the `bash` tool name.
//! Commands are fixed constants; model-provided shell syntax is never executed.

use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use jcode_app_core::tool::{Tool, ToolContext, ToolOutput};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::{process::Command, time::timeout};

const COMMANDS: [(&str, &str); 5] = [
    ("pwd", "pwd"),
    ("id", "id"),
    ("uname -a", "uname -a"),
    ("cat /etc/os-release", "cat /etc/os-release"),
    ("cat /proc/1/cgroup", "cat /proc/1/cgroup"),
];

pub struct BashProbeTool;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BashProbeInput {
    command: String,
    #[serde(default, rename = "intent")]
    _intent: Option<String>,
    #[serde(default, rename = "accept_large_output")]
    _accept_large_output: Option<bool>,
}

#[async_trait]
impl Tool for BashProbeTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Temporary read-only environment check. Choose one exact command from the schema to inspect the Cube guest's working directory, OS, kernel, process identity or cgroup. Arbitrary shell commands, file reads, environment variables, network access and writes are not supported."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type":"object",
            "properties":{"command":{"type":"string","enum":COMMANDS.map(|(name, _)| name)}},
            "required":["command"],
            "additionalProperties":false
        })
    }

    async fn execute(&self, input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        let input: BashProbeInput = serde_json::from_value(input)?;
        let (_, command) = COMMANDS
            .iter()
            .find(|(name, _)| *name == input.command)
            .context("Unsupported Bash environment probe")?;
        let working_dir = ctx
            .working_dir
            .as_deref()
            .context("Guest working directory is unavailable")?;

        let mut process = Command::new("/bin/bash");
        process
            .args(["--noprofile", "--norc", "-c", command])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .current_dir(working_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let output = timeout(Duration::from_secs(3), process.output())
            .await
            .context("Bash environment probe timed out")??;
        ensure!(
            output.status.success(),
            "Bash environment probe failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let output = stdout.trim_end();
        ensure!(
            output.len() <= 16 * 1024,
            "Bash probe output exceeded its limit"
        );
        Ok(ToolOutput::new(format!("$ {}\n{}", input.command, output))
            .with_title("Environment probe"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jcode_app_core::tool::ToolExecutionMode;

    fn context(working_dir: std::path::PathBuf) -> ToolContext {
        ToolContext {
            session_id: "test-session".into(),
            message_id: "test-message".into(),
            tool_call_id: "test-call".into(),
            working_dir: Some(working_dir),
            stdin_request_tx: None,
            graceful_shutdown_signal: None,
            execution_mode: ToolExecutionMode::Direct,
        }
    }

    #[test]
    fn schema_only_offers_fixed_environment_probes() {
        let schema = BashProbeTool.parameters_schema();
        assert_eq!(
            schema["properties"]["command"]["enum"],
            json!(COMMANDS.map(|(name, _)| name))
        );
        assert_eq!(schema["additionalProperties"], false);
    }

    #[tokio::test]
    async fn rejects_arbitrary_shell_syntax() {
        let error = BashProbeTool
            .execute(
                json!({"command":"pwd; env"}),
                context(std::env::current_dir().unwrap()),
            )
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Unsupported Bash environment probe")
        );
    }

    #[tokio::test]
    async fn fixed_probe_runs_in_the_supplied_working_directory() {
        let directory = tempfile::tempdir().unwrap();
        let output = BashProbeTool
            .execute(
                json!({"command":"pwd"}),
                context(directory.path().to_owned()),
            )
            .await
            .unwrap();
        assert_eq!(
            output.output,
            format!("$ pwd\n{}", directory.path().display())
        );
    }
}
