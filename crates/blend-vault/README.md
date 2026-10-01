# blend-vault

A SEP-56 tokenized vault over a [Blend](https://www.blend.capital/) pool supply
position on Stellar Soroban, built to back YBC PT/YT markets.

Forked from Script3's `fee-vault-v2` and carried in this workspace with its
history. The fee modes, signer gate and hand-rolled share token that fork had
are gone; the bToken ratio maths and its property tests are theirs and stay.
That original `fee-vault-v2` code is MIT under Script3's copyright, preserved
in `LICENSE-MIT-Script3`; everything else in this crate is GPL-3.0 under the
workspace's root `LICENSE.md`.

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

The full SEP-56 interface, exactly as the standard declares it:

```rust
total_supply() -> i128                         query_asset() -> Address
total_assets() -> i128
convert_to_shares(assets) -> i128              convert_to_assets(shares) -> i128
max_deposit(receiver) -> i128                  preview_deposit(assets) -> i128
deposit(assets, receiver, from, operator) -> shares
max_mint(receiver) -> i128                     preview_mint(shares) -> i128
mint(shares, receiver, from, operator) -> assets
max_withdraw(owner) -> i128                    preview_withdraw(assets) -> i128
withdraw(assets, receiver, owner, operator) -> shares
max_redeem(owner) -> i128                      preview_redeem(shares) -> i128
redeem(shares, receiver, owner, operator) -> assets
```

with the standard's `Deposit` and `Withdraw` events, and the full SEP-41 token
surface on the same address. YBC itself calls only `query_asset`,
`convert_to_assets`, `deposit` and `redeem`. There are no fees, so every
`preview_*` equals the matching conversion with the standard's rounding:
`deposit` and `redeem` round down, `mint` and `withdraw` round up, and every
rounding falls in the vault's favour (`src/tests/conformance.rs`).

`max_withdraw` and `max_redeem` are additionally capped by `withdraw_limit()`:
the reserve's supplied value less its borrowed value, the most the pool can pay
right now. It is a snapshot of liquidity shared with every supplier, not a
reservation.

Beyond the standard: `get_protocol() -> Address` (the Blend pool), the one
view every adapter in this workspace exposes; it is informational, read by the
YBC indexer so a curator can confirm the protocol behind a vault, and nothing
on chain calls it. Blend-specific views are `get_vault` (the ratio state),
`get_b_tokens` and `withdraw_limit`.

Operations, all admin: `set_admin`, `set_router`, `set_swap_path` and
`claim_emissions` (the admin sets the swap's slippage floor). Rewards are
protocol yield: `claim_emissions` swaps BLND into the underlying and supplies
it back, so there is no admin token-rescue function and nothing an admin can
move out of the vault.

## What is shared and what is Blend's

The share token (OpenZeppelin `Base`), the SEP-56 declaration
(`impl_sep56!` writes `preview_deposit`, `preview_redeem`, `max_redeem`,
`max_withdraw` and `max_mint`, and compile-checks the rest), the
operator-allowance rule on `redeem` and `withdraw`, the positive-amount guard,
the TTL policy, the `Deposit`/`Withdraw` events and the widening multiply come
from `vault-common`. `src/tests/conformance.rs` binds this crate's fixture to
`vault-testkit`, which runs the same 36 properties against every adapter,
here on a real Blend pool.

This crate owns the pool client (`pool.rs`, generated from the vendored
`wasm/blend/pool.wasm`), the ratio maths (`vault.rs`), emissions harvesting
(`swap.rs`, `claim_emissions`), and its own errors (`200`–`208`; shared codes
are 10–49, OpenZeppelin's 100–199).

## Build and test

From the workspace root:

```bash
make test    # cargo test --workspace
make build   # stellar contract build --optimize
```

The tests deploy a real Blend stack from `wasm/blend/` through
`vault_testkit::protocols::blend`; see `MANIFEST.md` there for provenance.
`blend-contract-sdk` is not a dependency because it pins its own
`soroban-sdk`, but the generated clients are identical. The crate's own tests
cover only what is Blend's (the bToken ratio, emissions, admin, pool status);
the standard's behaviour is the conformance suite's job.

## Deployment

```bash
stellar contract deploy --wasm target/wasm32v1-none/release/blend_vault.wasm \
  -- --admin <ADMIN> --pool <BLEND_POOL> --asset <RESERVE_SAC> --blnd-token <BLND> \
     --name "Blend USDC Vault" --symbol bvUSDC
```

Then `set_router <SOROSWAP_ROUTER>` before the first `claim_emissions`, and
`set_swap_path` if the harvest should route through an intermediate asset
(for an XLM vault, `[BLND, USDC, XLM]` where the BLND:USDC pool is deepest).

## Notes for anyone extending this

**Redeem accounting uses the pool's rounding.** Blend rounds the bTokens it
burns *up* from the underlying requested, so `redeem` recomputes
`underlying_to_b_tokens_up` before debiting `total_b_tokens`, or the vault's
figure drifts above the real position. The `test_redeem*` cases and
`deposit_then_full_redeem_never_profits` in `src/vault.rs`, and
`src/tests/test_happy_path.rs`, pin this against the pool.

**A default is reported, not hidden.** A falling `b_rate` lowers every
holder's `max_withdraw` immediately (`src/tests/test_default.rs`). The consumer's
high-water mark decides how to treat it; the vault's job is to tell the truth.

**`claim_emissions` is admin-only.** There is no on-chain price for BLND
(none of Reflector, Pyth, Band or DIA publishes one), so a fair floor cannot
be derived on chain and has to come from a party the vault trusts; the admin
already picks the router. An open call with a caller-chosen floor would let
anyone move the BLND price, harvest at floor zero and move it back, taking
that harvest's yield. The floor the admin passes is enforced by the vault
against the asset balance it measures either side of the swap
(`SwapBelowMinimum`, 207), not by the router and not from the figure the
router reports; a swap that delivers nothing is refused whatever the floor
(`SwapNoOutput`, 206). The BLND leaves the vault by a `transfer` the router
makes from the vault to the first pair, which the vault authorises for that
one call (`vault_common::auth::authorize_transfer_as_current`); an allowance
would cover only `transfer_from`, which the router never uses. The route is
`set_swap_path`, checked to start at BLND and end at the underlying.
