//! Spawn real processes from an INI config and display them in a TUI.
//! Use arrow keys to switch between tasks, 'q' to quit.

mod config;
mod runner;

use std::collections::HashMap;
use std::time::Duration;
use std::env;

use tokio::sync::watch;
use tokio::time::sleep;
use turbopath::AbsoluteSystemPathBuf;
use turborepo_ui::{
    ColorConfig,
    tui::{self, TuiSender},
};

use config::{parse_ini, topo_sort};
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
            let dep_rx = entry
                .depends_on
                .as_ref()
                .map(|dep| ready_rxs.get(dep).expect("dep must exist").clone());
            let shutdown = shutdown_rx.clone();

            // Normalize the working directory of every task
            let current_dir_pathbuf = env::current_dir().expect("Failed to get current directory");
            let current_dir_string: String = current_dir_pathbuf
                .into_os_string()
                .into_string()
                .expect("Path is not valid UTF-8");
            let current_dir = entry.work_dir.unwrap_or(current_dir_string).to_string();

            tokio::spawn(async move {
                run_task(s, entry.name, entry.command, current_dir, entry.ready_check, ready_tx, dep_rx, shutdown).await;
            })
        })
        .collect();

    let all_tasks = async move {
        for handle in handles {
            handle.await.ok();
        }
    };

    // Race between all tasks completing and TUI exiting (user pressed "q" or Ctrl+C).
    tokio::select! {
        _ = all_tasks => {
            // All tasks finished naturally — give user a moment to see final state.
            sleep(Duration::from_secs(2)).await;
            stop_sender.stop().await;
        }
        _ = &mut tui_handle => {
            // TUI exited (user pressed "q") — signal all tasks to shut down.
            shutdown_tx.send(true).ok();
            return Ok(());
        }
    }

    tui_handle.await.unwrap()?;
    Ok(())
}
