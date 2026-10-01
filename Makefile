default: build

# `stellar contract build`, not `cargo build`: soroban-sdk 26's spec shaking
# needs the CLI wrapper (v25.2.0+), and a bare cargo build fails in the SDK's
# build script. Builds every cdylib member of the workspace; the shared and
# test crates are rlib-only and are skipped.
build:
	stellar contract build --optimize
	@ls -l target/wasm32v1-none/release/*.wasm

test:
	cargo test --workspace

fmt:
	cargo fmt --all

clean:
	cargo clean

.PHONY: default build test fmt lint clean

# What CI gates: formatting, and the linter with the three lints the code
# disagrees with allowed (argument counts on the API, the 1_0000000 stroop
# notation).
lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings -A clippy::too-many-arguments -A clippy::inconsistent-digit-grouping -A clippy::zero-prefixed-literal
