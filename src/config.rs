use std::collections::{HashMap, VecDeque};

use ini::Ini;

/// A parsed task entry from the INI file.
pub struct TaskEntry {
    pub name: String,
    pub command: String,
    pub work_dir: Option<String>,
    pub depends_on: Vec<String>,
    pub ready_check: Option<String>,
}

/// Parse an INI file into task entries.
///
/// Each named section becomes a task. The section name is the task name,
/// and `command`, `depends_on`, and `ready_check` are read from the section's keys.
pub fn parse_ini(path: &str) -> Vec<TaskEntry> {
    let ini = Ini::load_from_file(path)
        .unwrap_or_else(|e| panic!("failed to read config file '{path}': {e}"));

    ini.iter()
        .filter_map(|(section, props)| {
            let name = section?.to_string();
            let command = props.get("command")?.to_string();
            let work_dir = props.get("work_dir").map(|s| s.to_string());
            let depends_on: Vec<String> = props
                .get("depends_on")
                .map(|s| s.split(',').map(|d| d.trim().to_string()).collect())
                .unwrap_or_default();
            let ready_check = props.get("ready_check").map(|s| s.to_string());
            Some(TaskEntry {
                name,
                command,
                work_dir,
                depends_on,
                ready_check,
            })
        })
        .collect()
}

/// Topological sort so dependencies come before dependents.
/// Panics on cycles or missing dependency names.
pub fn topo_sort(entries: Vec<TaskEntry>) -> Vec<TaskEntry> {
    let index_of: HashMap<&str, usize> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.name.as_str(), i))
        .collect();

    let n = entries.len();
    let mut in_degree = vec![0usize; n];
    let mut adj: Vec<Vec<usize>> = vec![vec![]; n];

    for (i, entry) in entries.iter().enumerate() {
        for dep in &entry.depends_on {
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
