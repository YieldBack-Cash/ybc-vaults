# blend-vault

A SEP-56 tokenized vault over a [Blend](https://www.blend.capital/) pool supply
position on Stellar Soroban, built to back YBC PT/YT markets.

Forked from Script3's `fee-vault-v2` and carried in this workspace with its
history. The fee modes, signer gate and hand-rolled share token that fork had
are gone; the bToken ratio maths and its property tests are theirs and stay.

## The idea in one paragraph

Blend represents a supply position as bTokens whose value in the underlying
grows with the reserve's `b_rate`. This vault holds one bToken position in one
reserve and issues shares against it through its own `total_shares /
total_b_tokens` ratio. Shares are *not* 1:1 with bTokens, because
`claim_emissions` harvests the position's BLND, swaps it for the underlying and
supplies it back, growing every holder's claim without a second token. The rate
YBC reads is therefore two conversions deep, shares → bTokens → underlying, and
both round down.

## Surface

Four functions are what YBC actually calls:

```rust
query_asset() -> Address
convert_to_assets(shares: i128) -> i128
deposit(assets: i128, receiver: Address, from: Address, operator: Address) -> i128
redeem(shares: i128, receiver: Address, owner: Address, operator: Address) -> i128
```

Plus the full SEP-41 token surface on the same address, `total_assets`,
`max_deposit`, `max_withdraw`, and `get_protocol() -> Address` (the Blend
pool). `get_protocol` is the one view beyond SEP-56 that every adapter in this
workspace exposes; it is informational, read by the YBC indexer so a curator
can confirm the protocol behind a vault, and nothing on chain calls it.
Blend-specific views are `get_vault` (the ratio state) and `get_b_tokens`.

Operations: `set_admin`, `set_router` (admin) and `claim_emissions` (anyone;
the caller sets the swap's slippage floor). Rewards are protocol yield:
`claim_emissions` swaps BLND into the underlying and supplies it back, so
there is no admin token-rescue function and nothing an admin can move.

## What is shared and what is Blend's

The share token (OpenZeppelin `Base`), the operator-allowance rule on `redeem`,
the positive-amount guard, the TTL policy, the `Deposit`/`Redeem` events and
the widening multiply come from `vault-common`. `tests/conformance.rs` binds
this crate's fixture to `vault-testkit`, which runs the same 18 properties
against every adapter, here on a real Blend pool.

This crate owns the pool client (`pool.rs`, generated from the vendored
`wasm/blend/pool.wasm`), the ratio maths (`vault.rs`), emissions harvesting
(`swap.rs`, `claim_emissions`), and its own errors (`200`–`205`; shared codes
are 10–49, OpenZeppelin's 100–199).

## Build and test

From the workspace root:

```bash
make test    # cargo test --workspace
make build   # stellar contract build --optimize
```

The tests deploy a real Blend stack from `wasm/blend/`; see `MANIFEST.md`
there for provenance. `blend-contract-sdk` is not a dependency because it pins
its own `soroban-sdk`, but the generated clients are identical.

## Deployment

```bash
stellar contract deploy --wasm target/wasm32v1-none/release/blend_vault.wasm \
  -- --admin <ADMIN> --pool <BLEND_POOL> --asset <RESERVE_SAC> --blnd-token <BLND> \
     --name "Blend USDC Vault" --symbol bvUSDC
```

Then `set_router <SOROSWAP_ROUTER>` before the first `claim_emissions`.

## Notes for anyone extending this

**Redeem accounting uses the pool's rounding.** Blend rounds the bTokens it
burns *up* from the underlying requested, so `redeem` recomputes
`underlying_to_b_tokens_up` before debiting `total_b_tokens`, or the vault's
figure drifts above the real position. `tests/test_redeem.rs` pins this
against the pool.

**A default is reported, not hidden.** A falling `b_rate` lowers every
holder's `max_withdraw` immediately (`tests/test_default.rs`). The consumer's
high-water mark decides how to treat it; the vault's job is to tell the truth.

**`claim_emissions` is unprivileged.** Its `amount_out_min` is caller-chosen,
so a caller passing 0 accepts whatever the Soroswap leg returns. Flagged in
the YBC threat model.
