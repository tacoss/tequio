dev:
	@npx sirv-cli . --dev
run:
	@cargo run -- tequio.ini
build:
	@cargo build --release 2>&1
install:
	@cp target/release/tequio $(HOME)/.local/bin/tequio

VENDOR_DIR = vendor/turborepo-ui
TMP_DIR = /tmp/turborepo-ui-merge

update-vendor:
	@echo "Downloading latest turborepo-ui from upstream..."
	@rm -rf $(TMP_DIR)
	@mkdir -p $(TMP_DIR)/upstream
	@git clone --depth 1 https://github.com/vercel/turborepo $(TMP_DIR)/upstream/turborepo
	@cd $(TMP_DIR)/upstream/turborepo/crates/turborepo-ui && \
		git init && git add -A && git commit -m "upstream" --allow-empty
	@cd $(VENDOR_DIR) && git init 2>/dev/null || true
	@cd $(VENDOR_DIR) && git add -A && git commit -m "local changes" 2>/dev/null || true
	@cd $(VENDOR_DIR) && git remote add upstream $(TMP_DIR)/upstream/turborepo/crates/turborepo-ui/.git 2>/dev/null || true
	@cd $(VENDOR_DIR) && git fetch upstream
	@cd $(VENDOR_DIR) && git merge upstream/master --allow-unrelated-histories --no-edit || (echo "" && echo "Merge conflict! Resolve manually then:" && echo "  cd $(VENDOR_DIR) && git add -A && git commit -m 'merge'" && false)
	@rm -rf $(TMP_DIR)
