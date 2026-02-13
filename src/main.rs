//! Spawn real processes from an INI config and display them in a TUI.
//!
//! The INI file uses a simple `name = command` format (one task per line).
//! Lines starting with `#` and blank lines are ignored.
//!
//! Run with:
//!   cargo run -- tasks.ini
//!
//! Use arrow keys to switch between tasks, 'q' to quit.

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use ini::Ini;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::watch;
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
    depends_on: Option<String>,
    ready_check: Option<String>,
}

/// Parse an INI file into task entries.
///
/// Each named section becomes a task. The section name is the task name,
/// and `command`, `depends_on`, and `ready_check` are read from the section's keys.
fn parse_ini(path: &str) -> Vec<TaskEntry> {
    let ini = Ini::load_from_file(path)
        .unwrap_or_else(|e| panic!("failed to read config file '{path}': {e}"));

    ini.iter()
        .filter_map(|(section, props)| {
            let name = section?.to_string();
            let command = props.get("command")?.to_string();
            let depends_on = props.get("depends_on").map(|s| s.to_string());
            let ready_check = props.get("ready_check").map(|s| s.to_string());
            Some(TaskEntry {
                name,
                command,
                depends_on,
                ready_check,
            })
        })
        .collect()
}

/// Topological sort so dependencies come before dependents.
/// Panics on cycles or missing dependency names.
fn topo_sort(entries: Vec<TaskEntry>) -> Vec<TaskEntry> {
    let index_of: HashMap<&str, usize> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.name.as_str(), i))
        .collect();

    let n = entries.len();
    let mut in_degree = vec![0usize; n];
    let mut adj: Vec<Vec<usize>> = vec![vec![]; n];

    for (i, entry) in entries.iter().enumerate() {
        if let Some(dep) = &entry.depends_on {
            let &dep_idx = index_of.get(dep.as_str()).unwrap_or_else(|| {
                panic!("task '{}' depends on unknown task '{}'", entry.name, dep)
            });
            adj[dep_idx].push(i);
            in_degree[i] += 1;
        }
    }

    let mut queue: VecDeque<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
    let mut order = Vec::with_capacity(n);

    while let Some(idx) = queue.pop_front() {
        order.push(idx);
        for &next in &adj[idx] {
            in_degree[next] -= 1;
            if in_degree[next] == 0 {
                queue.push_back(next);
            }
        }
    }

    if order.len() != n {
        panic!("dependency cycle detected among tasks");
    }

    let mut slots: Vec<Option<TaskEntry>> = entries.into_iter().map(Some).collect();
    order.into_iter().map(|i| slots[i].take().unwrap()).collect()
}

/// Spawn a real command, piping its stdout/stderr into the TUI task pane.
///
/// If `dep_rx` is provided, waits for the dependency to become ready before
/// spawning. If `ready_check` is set, scans stdout for a matching line and
/// signals `ready_tx` on match; otherwise signals ready immediately after spawn.
async fn run_task(
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
                    if line.trim() == check.as_str() {
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

    let entries = topo_sort(entries);
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

    // Build ready-signal channels for each task.
    let mut ready_txs: HashMap<String, watch::Sender<bool>> = HashMap::new();
    let mut ready_rxs: HashMap<String, watch::Receiver<bool>> = HashMap::new();
    for entry in &entries {
        let (tx, rx) = watch::channel(false);
        ready_txs.insert(entry.name.clone(), tx);
        ready_rxs.insert(entry.name.clone(), rx);
    }

    // Spawn all tasks concurrently (dependency waiting happens inside run_task).
    let handles: Vec<_> = entries
        .into_iter()
        .map(|entry| {
            let s = sender.clone();
            let ready_tx = ready_txs.remove(&entry.name).unwrap();
            let dep_rx = entry
                .depends_on
                .as_ref()
                .map(|dep| ready_rxs.get(dep).expect("dep must exist").clone());
            tokio::spawn(async move {
                run_task(s, entry.name, entry.command, entry.ready_check, ready_tx, dep_rx).await;
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
