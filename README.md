# Tequio

> In México, the task or collective work that each person owes to their community is known as tequio.

A task orchestration CLI that runs shell commands with dependency resolution and displays live output in an interactive terminal UI.

## Features

- **INI-based configuration** — define tasks, dependencies, and readiness checks in a simple config file
- **Dependency resolution** — tasks are topologically sorted and wait for their dependencies before starting
- **Ready checks** — a task can declare a substring pattern that signals when it's ready, so dependents don't have to wait for full completion
- **Interactive TUI** — real-time output from all tasks displayed in a terminal interface powered by a vendored fork of turborepo-ui
- **Graceful shutdown** — press `Ctrl+C` to kill all running processes and exit cleanly

## Usage

```
tequio <your-tasks.ini>
```

If no config file is given, it defaults to `tequio.ini` in the current directory.

### Keybindings

| Key | Action |
|-----|--------|
| `Up` / `Down` | Switch between tasks |
| `q` | Stop all tasks and exit |

## Configuration

Tasks are defined in an INI file. Each section is a task:

```ini
[build]
command = cargo build --release

[serve]
command = ./target/release/myapp
depends_on = build
ready_check = listening on port

[test]
command = cargo test
depends_on = build
```

### Fields

| Field | Required | Description |
|-------|----------|-------------|
| `command` | yes | Shell command to execute (run via `sh -c`) |
| `work_dir` | no | Set the working directory for the executed task |
| `depends_on` | no | Name of another task(s) that must be ready first (comma-separated list for one or more tasks) |
| `ready_check` | no | Substring to look for in stdout to signal readiness. If omitted, the task is considered ready as soon as it starts |

## Building

Requires Rust nightly (`nightly-2025-12-05`, configured in `rust-toolchain.toml`).

```
make build     # cargo build --release
make run       # cargo run -- tequio.ini
make install   # copy binary to ~/.local/bin/tequio
```

## License

MIT
