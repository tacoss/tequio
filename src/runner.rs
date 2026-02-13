use std::io::Write;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::watch;
use turborepo_ui::tui::{self, TuiSender, event::OutputLogs};

/// Spawn a real command, piping its stdout/stderr into the TUI task pane.
///
/// If `dep_rx` is provided, waits for the dependency to become ready before
/// spawning. If `ready_check` is set, scans stdout for a matching line and
/// signals `ready_tx` on match; otherwise signals ready immediately after spawn.
pub async fn run_task(
    sender: TuiSender,
    name: String,
    command: String,
    ready_check: Option<String>,
    ready_tx: watch::Sender<bool>,
    dep_rx: Option<watch::Receiver<bool>>,
) {
    let mut task = sender.task(name.clone());
    task.start(OutputLogs::Full);

    // Wait for dependency to become ready.
    if let Some(mut rx) = dep_rx {
        sender.status(
            name.clone(),
            "waiting".into(),
            tui::event::CacheResult::Miss,
        );
        rx.wait_for(|&ready| ready).await.ok();
    }

    sender.status(
        name.clone(),
        "running".into(),
        tui::event::CacheResult::Miss,
    );

    let child = Command::new("sh")
        .args(["-c", &command])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            writeln!(task, "failed to spawn command: {e}").ok();
            task.failed();
            ready_tx.send(true).ok();
            return;
        }
    };

    // If there is no ready_check, the task is ready as soon as it starts.
    if ready_check.is_none() {
        ready_tx.send(true).ok();
    }

    let ready_tx = Arc::new(ready_tx);
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    // Read stdout and stderr concurrently, writing lines to the TUI.
    let stdout_task = {
        let mut task = sender.task(name.clone());
        let ready_tx = ready_tx.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if let Some(ref check) = ready_check {
                    if line.trim().contains(check.as_str()) {
                        ready_tx.send(true).ok();
                    }
                }
                writeln!(task, "{line}").ok();
            }
        })
    };

    let stderr_task = {
        let mut task = sender.task(name.clone());
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                writeln!(task, "{line}").ok();
            }
        })
    };

    stdout_task.await.ok();
    stderr_task.await.ok();

    // Ensure dependents are unblocked even if ready_check was never matched.
    ready_tx.send(true).ok();

    let status = child.wait().await;
    match status {
        Ok(s) if s.success() => {
            task.succeeded(false);
        }
        Ok(s) => {
            let code = s.code().unwrap_or(-1);
            writeln!(task, "process exited with code {code}").ok();
            task.failed();
        }
        Err(e) => {
            writeln!(task, "error waiting for process: {e}").ok();
            task.failed();
        }
    }
}
