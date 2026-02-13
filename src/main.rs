//! Spawn real processes from an INI config and display them in a TUI.
//!
//! The INI file uses a simple `name = command` format (one task per line).
//! Lines starting with `#` and blank lines are ignored.
//!
//! Run with:
//!   cargo run -- tasks.ini
//!
//! Use arrow keys to switch between tasks, 'q' to quit.

use std::io::Write;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::sleep;
use turbopath::AbsoluteSystemPathBuf;
use turborepo_ui::{
    ColorConfig,
    tui::{self, TuiSender, event::OutputLogs},
};

/// A parsed task entry from the INI file.
struct TaskEntry {
    name: String,
    command: String,
}

/// Parse a simple INI file into task entries.
///
/// Each non-empty, non-comment line is expected to be `name = command`.
fn parse_ini(path: &str) -> Vec<TaskEntry> {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read config file '{path}': {e}"));

    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (name, command) = line.split_once('=')?;
            Some(TaskEntry {
                name: name.trim().to_string(),
                command: command.trim().to_string(),
            })
        })
        .collect()
}

/// Spawn a real command, piping its stdout/stderr into the TUI task pane.
async fn run_task(sender: TuiSender, name: String, command: String) {
    let mut task = sender.task(name.clone());
    task.start(OutputLogs::Full);
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
            return;
        }
    };

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    // Read stdout and stderr concurrently, writing lines to the TUI.
    let stdout_task = {
        let mut task = sender.task(name.clone());
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
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

#[tokio::main]
async fn main() -> Result<(), turborepo_ui::Error> {
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "tasks.ini".into());

    let entries = parse_ini(&config_path);
    if entries.is_empty() {
        eprintln!("no tasks found in '{config_path}'");
        std::process::exit(1);
    }

    let task_names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
    let color_config = ColorConfig::infer();
    let repo_root = AbsoluteSystemPathBuf::new(std::env::current_dir().unwrap().to_str().unwrap())
        .expect("cwd is absolute");

    let (sender, receiver) = TuiSender::new();
    let stop_sender = sender.clone();

    // Spawn the TUI render loop.
    let tui_handle = tokio::spawn(async move {
        tui::run_app(task_names, receiver, color_config, &repo_root, 1000).await
    });

    // Spawn all tasks concurrently.
    let handles: Vec<_> = entries
        .into_iter()
        .map(|entry| {
            let s = sender.clone();
            tokio::spawn(async move {
                run_task(s, entry.name, entry.command).await;
            })
        })
        .collect();

    for handle in handles {
        handle.await.unwrap();
    }

    // Give the user a moment to see the final state, then stop.
    sleep(Duration::from_secs(2)).await;
    stop_sender.stop().await;

    tui_handle.await.unwrap()?;
    Ok(())
}
