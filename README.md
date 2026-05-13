# Tequio

> In México, the task or collective work that each person owes to their community is known as tequio.

A task orchestration CLI that runs shell commands with dependency resolution and displays live output in an interactive terminal UI.

## Features

- **INI-based configuration** — define tasks, dependencies, and readiness checks in a simple config file
- **Dependency resolution** — tasks are topologically sorted and wait for their dependencies before starting
- **Ready checks** — a task can declare a substring pattern that signals when it's ready, so dependents don't have to wait for full completion
- **Interactive TUI** — real-time output from all tasks displayed in a terminal interface powered by a vendored fork of turborepo-ui
- **Graceful shutdown** — press `Ctrl+C` to kill all running processes and exit cleanly
- **Preflight checks** — run lint/test/build checks before pushing or opening a PR
- **Install commands** — seed shared dependencies in the base repo, ready to be symlinked into worktrees
- **asdf support** — `~/.asdf/shims` is always prepended to `PATH` for all child processes

## Usage

```bash
tequio                        # run all tasks
tequio api database           # run specific tasks (and their dependencies)
tequio --stop                 # kill orphan processes from a previous run
tequio --preflight            # run preflight checks for all tasks
tequio --preflight api        # run preflight checks for a specific task
tequio --install              # run install commands for all tasks
tequio --install front        # run install command for a specific task
```

Override the working directory without editing `tequio.ini`:

```bash
tequio --preflight front --work_dir .worktrees/some-branch
tequio --install front --repo_dir regulix-frontend
```

If no config file is given, it defaults to `tequio.ini` in the current directory. Use `-c` / `--config` to specify a different path.

### Keybindings

| Key | Action |
|-----|--------|
| `Up` / `Down` | Switch between tasks |
| `q` | Stop all tasks and exit |

## Configuration

Tasks are defined in an INI file. Each section is a task:

```ini
[database]
command     = make pgup
ready_check = database is ready

[api]
repo_dir    = my-api
work_dir    = .worktrees/feat-123
command     = pnpm run dev
install     = pnpm install
preflight   = pnpm run lint && pnpm run test && pnpm run build
depends_on  = database
ready_check = listening on port
```

### Fields

| Field | Required | Description |
|-------|----------|-------------|
| `command` | yes | Shell command to run (via `bash -c`) |
| `repo_dir` | no | Base repo directory — source of truth for installs and worktree symlinking |
| `work_dir` | no | Active working directory for running the process (can be a worktree path) |
| `depends_on` | no | Task(s) that must be ready first (comma-separated) |
| `ready_check` | no | Substring in stdout that signals readiness. If omitted, task is ready immediately |
| `preflight` | no | Command to run with `--preflight` (e.g. lint, test, build) |
| `install` | no | Command to run with `--install` (e.g. `pnpm install`) |

### `repo_dir` vs `work_dir`

Use both when working with git worktrees:

- **`repo_dir`** — the base/main repository. Used by `--install` and `worktree-setup` to seed shared dependencies.
- **`work_dir`** — the active directory tequio runs the process from. Can be a worktree path.

When no worktree is active, `work_dir` alone is sufficient.

```ini
[front]
repo_dir  = regulix-frontend                          # base repo, never changes
work_dir  = .worktrees/feat-456                       # active worktree, swap as needed
install   = pnpm install
command   = pnpm run dev
preflight = pnpm run lint && pnpm run build
```

## Building

Requires Rust nightly (`nightly-2025-12-05`, configured in `rust-toolchain.toml`).

```
make build     # cargo build --release
make run       # cargo run -- tequio.ini
make install   # copy binary to ~/.local/bin/tequio
```

## License

MIT
