run:
	@cargo run -p turborepo-ui --example two_processes -- crates/turborepo-ui/examples/tasks.ini
build:
	@cargo build -p turborepo-ui --example two_processes --release 2>&1
install:
	@sudo cp target/release/examples/two_processes /usr/local/bin/two_processes
