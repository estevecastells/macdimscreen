.PHONY: build test lint fmt app install uninstall run-app ci clean help

build:            ## Build the daemon and CLI (release)
	cargo build --release --locked

test:             ## Rust unit tests + Swift protocol checks
	cargo test --locked
	cd app && swift run -c debug KitChecks

lint:             ## Formatting and lints, exactly as CI runs them
	cargo fmt --all --check
	cargo clippy --locked --all-targets -- -D warnings
	shellcheck scripts/*.sh

fmt:
	cargo fmt --all

app:              ## Build build/MacDimScreen.app
	scripts/build-app.sh

install: build    ## Install the daemon as a per-user launchd agent (no sudo)
	scripts/install.sh --bin-dir target/release

uninstall:        ## Remove the daemon and restore Night Shift
	scripts/uninstall.sh

run-app: app      ## Build and launch the menu bar app
	open build/MacDimScreen.app

ci: lint test app ## Everything a pull request must pass

clean:
	cargo clean
	rm -rf build app/.build

help:
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F':.*## ' '{printf "  %-12s %s\n", $$1, $$2}'
