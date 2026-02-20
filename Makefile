dev:
	@npx sirv-cli . --dev
run:
	@cargo run -- tequio.ini
build:
	@cargo build --release 2>&1
install:
	@cp target/release/tequio $(HOME)/.local/bin/tequio-dev
