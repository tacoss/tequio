use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

pub struct PidFile {
    path: PathBuf,
    pids: HashSet<u32>,
}

impl PidFile {
    pub fn new() -> Self {
        let path = std::env::temp_dir().join("tequio-pids.txt");
        Self { path, pids: HashSet::new() }
    }

    pub async fn load_and_kill_existing(&mut self) {
        if !self.path.exists() {
            return;
        }
        let contents = match fs::read_to_string(&self.path) {
            Ok(c) => c,
            Err(_) => return,
        };
        for line in contents.lines() {
            if let Ok(pid) = line.trim().parse::<u32>() {
                Self::kill_pid_tree(pid).await;
            }
        }
        let _ = fs::remove_file(&self.path);
    }

    pub fn register(&mut self, pid: u32) {
        self.pids.insert(pid);
        self.write();
    }

    pub fn unregister(&mut self, pid: u32) {
        self.pids.remove(&pid);
        if self.pids.is_empty() {
            let _ = fs::remove_file(&self.path);
        } else {
            self.write();
        }
    }

    pub async fn cleanup(&mut self) {
        for pid in self.pids.drain() {
            Self::kill_pid_tree(pid).await;
        }
        let _ = fs::remove_file(&self.path);
    }

    fn write(&self) {
        if let Ok(mut file) = fs::File::create(&self.path) {
            for pid in &self.pids {
                let _ = writeln!(file, "{}", pid);
            }
        }
    }

    async fn kill_pid_tree(pid: u32) {
        let _ = kill_tree::tokio::kill_tree(pid).await;
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        let pids: Vec<u32> = self.pids.drain().collect();
        if !pids.is_empty() {
            let rt = tokio::runtime::Handle::try_current();
            for pid in pids {
                if let Some(handle) = rt.as_ref().ok() {
                    let _ = handle.spawn(async move {
                        let _ = kill_tree::tokio::kill_tree(pid).await;
                    });
                }
            }
        }
        let _ = fs::remove_file(&self.path);
    }
}
