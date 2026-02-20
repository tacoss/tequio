use std::io::Write;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{Mutex, watch};
use turborepo_ui::tui::{self, TuiSender, event::OutputLogs};

use crate::pidfile::PidFile;

pub async fn run_task(
    sender: TuiSender,
    name: String,
    command: String,
    work_dir: String,
    ready_check: Option<String>,
    ready_tx: watch::Sender<bool>,
    dep_rxs: Vec<watch::Receiver<bool>>,
    mut shutdown_rx: watch::Receiver<bool>,
    pidfile: Arc<Mutex<PidFile>>,
) {
    let mut task = sender.task(name.clone());
    task.start(OutputLogs::Full);

    if !dep_rxs.is_empty() {
        sender.status(
            name.clone(),
            "waiting".into(),
            tui::event::CacheResult::Miss,
        );
        let wait_all = async {
            for mut rx in dep_rxs {
                rx.wait_for(|&ready| ready).await.ok();
            }
        };
        tokio::select! {
            _ = wait_all => {}
            _ = shutdown_rx.wait_for(|&v| v) => {
                ready_tx.send(true).ok();
                task.failed();
                return;
            }
        }
    }

    if *shutdown_rx.borrow() {
        ready_tx.send(true).ok();
        task.failed();
        return;
    }

    sender.status(
        name.clone(),
        "running".into(),
        tui::event::CacheResult::Miss,
    );

    let child = Command::new("sh")
        .args(["-c", &command])
        .current_dir(work_dir)
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

    let pid = child.id().unwrap_or(0);
    if pid > 0 {
        pidfile.lock().await.register(pid);
    }

    if ready_check.is_none() {
        ready_tx.send(true).ok();
    }

    let ready_tx = Arc::new(ready_tx);
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

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

    let shutdown_fut = async {
        loop {
            if shutdown_rx.changed().await.is_err() {
                std::future::pending::<()>().await;
            }
            if *shutdown_rx.borrow() {
                break;
            }
        }
    };

    tokio::select! {
        status = child.wait() => {
            stdout_task.await.ok();
            stderr_task.await.ok();
            ready_tx.send(true).ok();
            if pid > 0 {
                pidfile.lock().await.unregister(pid);
            }
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
        _ = shutdown_fut => {
            if pid > 0 {
                let _ = kill_tree::tokio::kill_tree(pid).await;
                pidfile.lock().await.unregister(pid);
            } else {
                child.kill().await.ok();
            }
            stdout_task.abort();
            stderr_task.abort();
            ready_tx.send(true).ok();
            task.failed();
        }
    }
}
