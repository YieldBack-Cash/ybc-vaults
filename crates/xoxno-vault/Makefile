default: build

# `stellar contract build`, not `cargo build`: soroban-sdk 26's
# experimental_spec_shaking_v2 feature requires the CLI wrapper (v25.2.0+) and
# a bare cargo build fails in the SDK's build script.
build:
	stellar contract build --optimize
	@ls -l target/wasm32v1-none/release/xoxno_vault.wasm

test:
	cargo test

fmt:
	cargo fmt --all

clean:
	cargo clean

.PHONY: default build test fmt clean
