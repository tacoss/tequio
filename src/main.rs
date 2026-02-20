//! Spawn real processes from an INI config and display them in a TUI.
//! Use arrow keys to switch between tasks.

mod config;
mod pidfile;
mod runner;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use std::env;

use tokio::sync::{Mutex, watch};
use tokio::time::sleep;
use turbopath::AbsoluteSystemPathBuf;
use turborepo_ui::{
    ColorConfig,
    tui::{self, TuiSender},
};

use config::{parse_ini, topo_sort};
use pidfile::PidFile;
use runner::run_task;

#[tokio::main]
async fn main() -> Result<(), turborepo_ui::Error> {
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "tequio.ini".into());

    let entries = parse_ini(&config_path);
    if entries.is_empty() {
        eprintln!("no tasks found in '{config_path}'");
        std::process::exit(1);
    }

    let mut pidfile = PidFile::new();
    pidfile.load_and_kill_existing().await;
    let pidfile = Arc::new(Mutex::new(pidfile));

    let entries = topo_sort(entries);
    let task_names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
    let color_config = ColorConfig::infer();
    let repo_root = AbsoluteSystemPathBuf::new(std::env::current_dir().unwrap().to_str().unwrap())
        .expect("cwd is absolute");

    let (sender, receiver) = TuiSender::new();
    let stop_sender = sender.clone();

    // Spawn the TUI render loop.
    let mut tui_handle = tokio::spawn(async move {
        tui::run_app(task_names, receiver, color_config, &repo_root, 1000).await
    });

    // Shutdown signal: when true, all tasks should kill their children and exit.
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

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
            let dep_rxs: Vec<watch::Receiver<bool>> = entry
                .depends_on
                .iter()
                .map(|dep| ready_rxs.get(dep).expect("dep must exist").clone())
                .collect();
            let shutdown = shutdown_rx.clone();
            let pf = pidfile.clone();

            // Normalize the working directory of every task
            let current_dir = resolve_work_dir(entry.work_dir.as_deref());

            tokio::spawn(async move {
                run_task(s, entry.name, entry.command, current_dir, entry.ready_check, ready_tx, dep_rxs, shutdown, pf).await;
            })
        })
        .collect();

    let ctrl_c = tokio::signal::ctrl_c();

    #[cfg(unix)]
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();

    let all_tasks = async move {
        for handle in handles {
            handle.await.ok();
        }
    };

    #[cfg(unix)]
    let sigterm_fut = async {
        if let Some(ref mut sig) = sigterm {
            sig.recv().await;
        } else {
            std::future::pending::<()>().await;
        }
    };

    #[cfg(not(unix))]
    let sigterm_fut = std::future::pending::<()>();

    // Race between all tasks completing, TUI exit, Ctrl+C, and SIGTERM.
    tokio::select! {
        _ = all_tasks => {
            sleep(Duration::from_secs(2)).await;
            stop_sender.stop().await;
        }
        _ = &mut tui_handle => {
            shutdown_tx.send(true).ok();
        }
        _ = ctrl_c => {
            shutdown_tx.send(true).ok();
            stop_sender.stop().await;
        }
        _ = sigterm_fut => {
            shutdown_tx.send(true).ok();
            stop_sender.stop().await;
        }
    }

    // Wait for all processes to be killed before cleanup.
    sleep(Duration::from_millis(500)).await;

    // Clean up pidfile (processes should be gone by now).
    if let Some(pf) = Arc::try_unwrap(pidfile).ok() {
        pf.into_inner().cleanup().await;
    }

    let _ = tui_handle.await;
    Ok(())
}

fn resolve_work_dir(entry_work_dir: Option<&str>) -> String {
    let current_dir_pathbuf = env::current_dir().expect("Failed to get current directory");
    let current_dir_string: String = current_dir_pathbuf
        .into_os_string()
        .into_string()
        .expect("Path is not valid UTF-8");
    entry_work_dir.unwrap_or(&current_dir_string).to_string()
}
