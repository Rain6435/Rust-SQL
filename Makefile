.PHONY: all build release test fmt fmt-check clippy doc install-hooks run-gui run-tui run-web clean

all: fmt-check clippy test build

build:
	cargo build

release:
	cargo build --release

test:
	cargo test

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

clippy:
	cargo clippy --all-targets -- -D warnings

doc:
	cargo doc --no-deps --open

install-hooks:
	git config core.hooksPath .githooks
	chmod +x .githooks/pre-commit .githooks/commit-msg .githooks/pre-push

run-gui:
	cargo run -- --gui

run-tui:
	cargo run -- --tui

run-web:
	cargo run -- --web --port 8080

clean:
	cargo clean
