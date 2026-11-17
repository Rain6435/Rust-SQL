.PHONY: all build release test fmt fmt-check clippy doc run-gui run-tui run-web clean

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

run-gui:
	cargo run -- --gui

run-tui:
	cargo run -- --tui

run-web:
	cargo run -- --web --port 8080

clean:
	cargo clean
