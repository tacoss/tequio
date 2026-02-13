run:
	@cargo run -- tasks.ini
build:
	@cargo build --release 2>&1
install:
	@cp target/release/task-runner-tui $(HOME)/.local/bin/tui
